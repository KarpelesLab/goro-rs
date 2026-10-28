//! PHP's json extension (PHP manual: JSON; RFC 8259) over goro-runtime
//! values: `json_encode`, `json_decode`, `json_validate`,
//! `json_last_error` and `json_last_error_msg`.
//!
//! Behaviour follows the PHP manual, the expected output of php-src's
//! `ext/json/tests/*.phpt`, and observations of PHP 8.5 (the fixture in
//! `tests/fixtures` was generated once from the reference binary by
//! `tests/fixtures/json_driver.php`).
//!
//! # Engine wiring
//!
//! The engine implements [`JsonHost`] (object classification,
//! `jsonSerialize()`, property enumeration, `stdClass` creation, and the
//! construction of the `JsonException`/`ValueError` it throws) and calls
//! the PHP-level functions [`json_encode`], [`json_decode`],
//! [`json_validate`], [`json_last_error`] and [`json_last_error_msg`]
//! after its usual argument coercion (`json_decode(null)` deprecation and
//! so on). They keep `json_last_error()`'s state in a thread-local, as a
//! request global. The [`consts`] module lists the `JSON_*` constants.
//!
//! The lower-level [`encode`], [`decode`] and [`validate`] do the work
//! without touching the error state or throwing.
//!
//! Nested calls (`json_encode()` from inside `jsonSerialize()`) are
//! supported: recursion guards are thread-local too, so an object or
//! array being encoded by an outer call is reported as recursion by an
//! inner one, as PHP does (test ext/json/tests/json_encode_recursion_01.phpt).

mod decode;
mod encode;

use goro_runtime::{ObjRef, PhpArray, PhpError, Value};
use std::cell::Cell;

pub use decode::{DecodeOptions, decode, validate};
pub use encode::encode;

/// The `JSON_*` constants (PHP manual: JSON, Predefined Constants; values
/// observed on PHP 8.5).
pub mod consts {
    pub const JSON_HEX_TAG: i64 = 1;
    pub const JSON_HEX_AMP: i64 = 2;
    pub const JSON_HEX_APOS: i64 = 4;
    pub const JSON_HEX_QUOT: i64 = 8;
    pub const JSON_FORCE_OBJECT: i64 = 16;
    pub const JSON_NUMERIC_CHECK: i64 = 32;
    pub const JSON_UNESCAPED_SLASHES: i64 = 64;
    pub const JSON_PRETTY_PRINT: i64 = 128;
    pub const JSON_UNESCAPED_UNICODE: i64 = 256;
    pub const JSON_PARTIAL_OUTPUT_ON_ERROR: i64 = 512;
    pub const JSON_PRESERVE_ZERO_FRACTION: i64 = 1024;
    pub const JSON_UNESCAPED_LINE_TERMINATORS: i64 = 2048;
    pub const JSON_OBJECT_AS_ARRAY: i64 = 1;
    pub const JSON_BIGINT_AS_STRING: i64 = 2;
    pub const JSON_INVALID_UTF8_IGNORE: i64 = 1_048_576;
    pub const JSON_INVALID_UTF8_SUBSTITUTE: i64 = 2_097_152;
    pub const JSON_THROW_ON_ERROR: i64 = 4_194_304;

    pub const JSON_ERROR_NONE: i64 = 0;
    pub const JSON_ERROR_DEPTH: i64 = 1;
    pub const JSON_ERROR_STATE_MISMATCH: i64 = 2;
    pub const JSON_ERROR_CTRL_CHAR: i64 = 3;
    pub const JSON_ERROR_SYNTAX: i64 = 4;
    pub const JSON_ERROR_UTF8: i64 = 5;
    pub const JSON_ERROR_RECURSION: i64 = 6;
    pub const JSON_ERROR_INF_OR_NAN: i64 = 7;
    pub const JSON_ERROR_UNSUPPORTED_TYPE: i64 = 8;
    pub const JSON_ERROR_INVALID_PROPERTY_NAME: i64 = 9;
    pub const JSON_ERROR_UTF16: i64 = 10;
    pub const JSON_ERROR_NON_BACKED_ENUM: i64 = 11;

    /// Every constant with its name, for registration by the engine.
    pub const ALL: [(&str, i64); 29] = [
        ("JSON_HEX_TAG", JSON_HEX_TAG),
        ("JSON_HEX_AMP", JSON_HEX_AMP),
        ("JSON_HEX_APOS", JSON_HEX_APOS),
        ("JSON_HEX_QUOT", JSON_HEX_QUOT),
        ("JSON_FORCE_OBJECT", JSON_FORCE_OBJECT),
        ("JSON_NUMERIC_CHECK", JSON_NUMERIC_CHECK),
        ("JSON_UNESCAPED_SLASHES", JSON_UNESCAPED_SLASHES),
        ("JSON_PRETTY_PRINT", JSON_PRETTY_PRINT),
        ("JSON_UNESCAPED_UNICODE", JSON_UNESCAPED_UNICODE),
        ("JSON_PARTIAL_OUTPUT_ON_ERROR", JSON_PARTIAL_OUTPUT_ON_ERROR),
        ("JSON_PRESERVE_ZERO_FRACTION", JSON_PRESERVE_ZERO_FRACTION),
        (
            "JSON_UNESCAPED_LINE_TERMINATORS",
            JSON_UNESCAPED_LINE_TERMINATORS,
        ),
        ("JSON_OBJECT_AS_ARRAY", JSON_OBJECT_AS_ARRAY),
        ("JSON_BIGINT_AS_STRING", JSON_BIGINT_AS_STRING),
        ("JSON_INVALID_UTF8_IGNORE", JSON_INVALID_UTF8_IGNORE),
        ("JSON_INVALID_UTF8_SUBSTITUTE", JSON_INVALID_UTF8_SUBSTITUTE),
        ("JSON_THROW_ON_ERROR", JSON_THROW_ON_ERROR),
        ("JSON_ERROR_NONE", JSON_ERROR_NONE),
        ("JSON_ERROR_DEPTH", JSON_ERROR_DEPTH),
        ("JSON_ERROR_STATE_MISMATCH", JSON_ERROR_STATE_MISMATCH),
        ("JSON_ERROR_CTRL_CHAR", JSON_ERROR_CTRL_CHAR),
        ("JSON_ERROR_SYNTAX", JSON_ERROR_SYNTAX),
        ("JSON_ERROR_UTF8", JSON_ERROR_UTF8),
        ("JSON_ERROR_RECURSION", JSON_ERROR_RECURSION),
        ("JSON_ERROR_INF_OR_NAN", JSON_ERROR_INF_OR_NAN),
        ("JSON_ERROR_UNSUPPORTED_TYPE", JSON_ERROR_UNSUPPORTED_TYPE),
        (
            "JSON_ERROR_INVALID_PROPERTY_NAME",
            JSON_ERROR_INVALID_PROPERTY_NAME,
        ),
        ("JSON_ERROR_UTF16", JSON_ERROR_UTF16),
        ("JSON_ERROR_NON_BACKED_ENUM", JSON_ERROR_NON_BACKED_ENUM),
    ];
}

use consts::*;

/// A json error code, as `json_last_error()` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum JsonError {
    #[default]
    None = 0,
    Depth = 1,
    StateMismatch = 2,
    CtrlChar = 3,
    Syntax = 4,
    Utf8 = 5,
    Recursion = 6,
    InfOrNan = 7,
    UnsupportedType = 8,
    InvalidPropertyName = 9,
    Utf16 = 10,
    NonBackedEnum = 11,
}

impl JsonError {
    /// The `JSON_ERROR_*` value.
    pub fn code(self) -> i64 {
        self as i64
    }

    /// `json_last_error_msg()` and the `JsonException` message (observed on
    /// PHP 8.5; tests ext/json/tests/007.phpt, bug62010.phpt,
    /// bug68546.phpt, inf_nan_error.phpt, unsupported_type_error.phpt).
    pub fn message(self) -> &'static str {
        match self {
            JsonError::None => "No error",
            JsonError::Depth => "Maximum stack depth exceeded",
            JsonError::StateMismatch => "State mismatch (invalid or malformed JSON)",
            JsonError::CtrlChar => "Control character error, possibly incorrectly encoded",
            JsonError::Syntax => "Syntax error",
            JsonError::Utf8 => "Malformed UTF-8 characters, possibly incorrectly encoded",
            JsonError::Recursion => "Recursion detected",
            JsonError::InfOrNan => "Inf and NaN cannot be JSON encoded",
            JsonError::UnsupportedType => "Type is not supported",
            JsonError::InvalidPropertyName => "The decoded property name is invalid",
            JsonError::Utf16 => "Single unpaired UTF-16 surrogate in unicode escape",
            JsonError::NonBackedEnum => "Non-backed enums have no default serialization",
        }
    }

    /// The error for a `JSON_ERROR_*` value.
    pub fn from_code(code: i64) -> Option<JsonError> {
        Some(match code {
            0 => JsonError::None,
            1 => JsonError::Depth,
            2 => JsonError::StateMismatch,
            3 => JsonError::CtrlChar,
            4 => JsonError::Syntax,
            5 => JsonError::Utf8,
            6 => JsonError::Recursion,
            7 => JsonError::InfOrNan,
            8 => JsonError::UnsupportedType,
            9 => JsonError::InvalidPropertyName,
            10 => JsonError::Utf16,
            11 => JsonError::NonBackedEnum,
            _ => return None,
        })
    }
}

/// How `json_encode` treats an object (PHP manual: json_encode,
/// JsonSerializable, Enumerations "Enumerations and JSON").
pub enum ObjectKind {
    /// Its public properties are encoded as a JSON object.
    Plain,
    /// The class implements `JsonSerializable`: the return value of
    /// `jsonSerialize()` is encoded instead.
    Serializable,
    /// A pure (non-backed) enum case: `JSON_ERROR_NON_BACKED_ENUM`.
    UnitEnum,
    /// A backed enum case: its backing value (int or string) is encoded.
    BackedEnum(Value),
}

/// What the json functions need from the engine.
pub trait JsonHost {
    /// What the engine throws: an exception object, or whatever it uses to
    /// carry one while unwinding.
    type Exception;

    /// ini `serialize_precision` (default -1: shortest round-trip).
    fn serialize_precision(&self) -> i32 {
        -1
    }

    /// Classifies an object for encoding.
    fn object_kind(&mut self, obj: &ObjRef) -> ObjectKind;

    /// Calls `$obj->jsonSerialize()`. An exception it throws propagates out
    /// of `json_encode` as is (tests ext/json/tests/bug68992.phpt,
    /// bug73113.phpt).
    fn json_serialize(&mut self, obj: &ObjRef) -> Result<Value, Self::Exception>;

    /// The properties `json_encode` shows for a plain object, in order:
    /// the public ones, initialised typed properties only (observed on PHP
    /// 8.5: an unset or uninitialised property is left out, private and
    /// protected ones are left out even when `json_encode` is called from
    /// inside the class). Internal classes may supply their own table, as
    /// they do for `(array)` casts (`ArrayObject` shows its storage,
    /// `DateTime` its date, timezone_type and timezone). Keys starting
    /// with a NUL byte (mangled private/protected names) are skipped, so
    /// returning the raw property table is fine. Values may be references.
    /// Returning a clone of the object's own table is cheap (copy on
    /// write) and safe: the encoder holds the table it iterates.
    fn object_properties(&mut self, obj: &ObjRef) -> Result<PhpArray, Self::Exception>;

    /// Creates a `stdClass` object with `props` as its properties, for
    /// `json_decode` without `$associative`. Keys follow PHP array key
    /// normalisation (`"0"` is the int key 0), as in the object's
    /// property table.
    fn new_object(&mut self, props: PhpArray) -> Value;

    /// The `JsonException` to throw for `JSON_THROW_ON_ERROR`: message
    /// [`JsonError::message`], code [`JsonError::code`].
    fn json_exception(&mut self, err: JsonError) -> Self::Exception;

    /// A `ValueError` for invalid arguments (`$depth`, `$flags`).
    fn throw_error(&mut self, err: PhpError) -> Self::Exception;

    /// Maximum nesting of arrays and objects `json_encode` accepts before
    /// failing with `JSON_ERROR_DEPTH` whatever `$depth` says, standing for
    /// PHP's stack limit (test ext/json/tests/gh15168.phpt: 30000 nested
    /// objects fail with `zend.max_allowed_stack_size=512K`). The encoder
    /// and decoder do not recurse on the Rust stack, so the default is no
    /// limit.
    fn max_encode_nesting(&self) -> Option<usize> {
        None
    }
}

thread_local! {
    static LAST_ERROR: Cell<JsonError> = const { Cell::new(JsonError::None) };
}

/// `json_last_error()`.
pub fn json_last_error() -> i64 {
    last_error().code()
}

/// `json_last_error_msg()`.
pub fn json_last_error_msg() -> &'static str {
    last_error().message()
}

/// The error recorded by the last json function call.
pub fn last_error() -> JsonError {
    LAST_ERROR.with(|e| e.get())
}

/// Sets the recorded error (the engine resets it to `None` at request
/// start).
pub fn set_last_error(err: JsonError) {
    LAST_ERROR.with(|e| e.set(err));
}

/// `json_encode(mixed $value, int $flags = 0, int $depth = 512): string|false`.
///
/// Returns `Value::Str` or `Value::Bool(false)`. Observed on PHP 8.5:
/// `$depth` is truncated to a 32-bit int (`PHP_INT_MAX` acts as -1) and
/// never raises a `ValueError`; `JSON_PARTIAL_OUTPUT_ON_ERROR` overrides
/// `JSON_THROW_ON_ERROR` (test ext/json/tests/json_encode_exceptions.phpt);
/// with `JSON_THROW_ON_ERROR` the error state is left untouched (test
/// ext/json/tests/json_exceptions_error_clearing.phpt).
pub fn json_encode<H: JsonHost + ?Sized>(
    host: &mut H,
    value: &Value,
    flags: i64,
    depth: i64,
) -> Result<Value, H::Exception> {
    let throw = flags & JSON_THROW_ON_ERROR != 0 && flags & JSON_PARTIAL_OUTPUT_ON_ERROR == 0;
    if !throw {
        set_last_error(JsonError::None);
    }
    let (out, err) = encode(host, value, flags, depth as i32)?;
    if !throw {
        // Set again at the end: a nested json_encode() in jsonSerialize()
        // must not leave its error behind (observed on PHP 8.5).
        set_last_error(err);
    }
    match err {
        JsonError::None => Ok(Value::Str(out.into())),
        e if throw => Err(host.json_exception(e)),
        _ => {
            if flags & JSON_PARTIAL_OUTPUT_ON_ERROR != 0 {
                Ok(Value::Str(out.into()))
            } else {
                Ok(Value::Bool(false))
            }
        }
    }
}

/// `json_decode(string $json, ?bool $associative = null, int $depth = 512,
/// int $flags = 0): mixed`.
///
/// Observed on PHP 8.5: the error state is reset first (unless
/// `JSON_THROW_ON_ERROR`), then an empty `$json` is a syntax error before
/// `$depth` is validated, then `$depth` must be in `1..=2147483647`
/// (ValueError). `$associative` `null` defers to `JSON_OBJECT_AS_ARRAY`;
/// `true`/`false` override it (test ext/json/tests/bug73991.phpt). Unknown
/// flags are ignored.
pub fn json_decode<H: JsonHost + ?Sized>(
    host: &mut H,
    json: &[u8],
    associative: Option<bool>,
    depth: i64,
    flags: i64,
) -> Result<Value, H::Exception> {
    let throw = flags & JSON_THROW_ON_ERROR != 0;
    if !throw {
        set_last_error(JsonError::None);
    }
    let fail = |host: &mut H, e: JsonError| {
        if throw {
            Err(host.json_exception(e))
        } else {
            set_last_error(e);
            Ok(Value::Null)
        }
    };
    if json.is_empty() {
        return fail(host, JsonError::Syntax);
    }
    if depth <= 0 {
        return Err(host.throw_error(PhpError::new(
            goro_runtime::ErrorKind::ValueError,
            "json_decode(): Argument #3 ($depth) must be greater than 0",
        )));
    }
    if depth > i32::MAX as i64 {
        return Err(host.throw_error(PhpError::new(
            goro_runtime::ErrorKind::ValueError,
            "json_decode(): Argument #3 ($depth) must be less than 2147483647",
        )));
    }
    let assoc = associative.unwrap_or(flags & JSON_OBJECT_AS_ARRAY != 0);
    let opts = DecodeOptions {
        assoc,
        depth: depth as u32,
        bigint_as_string: flags & JSON_BIGINT_AS_STRING != 0,
        invalid_utf8: Utf8Mode::from_decode_flags(flags),
    };
    match decode(host, json, &opts) {
        Ok(v) => Ok(v),
        Err(e) => fail(host, e),
    }
}

/// `json_validate(string $json, int $depth = 512, int $flags = 0): bool`
/// (PHP 8.3 RFC "json_validate").
///
/// Observed on PHP 8.5: the error state is reset first; `$flags` may only
/// contain `JSON_INVALID_UTF8_IGNORE` (ValueError); then an empty string is
/// a syntax error before `$depth` is checked; then `$depth` must be in
/// `1..=2147483647` (ValueError). Invalid property names are not
/// an error here (they only matter when building objects).
pub fn json_validate<H: JsonHost + ?Sized>(
    host: &mut H,
    json: &[u8],
    depth: i64,
    flags: i64,
) -> Result<bool, H::Exception> {
    set_last_error(JsonError::None);
    if flags & !JSON_INVALID_UTF8_IGNORE != 0 {
        return Err(host.throw_error(PhpError::new(
            goro_runtime::ErrorKind::ValueError,
            "json_validate(): Argument #3 ($flags) must be a valid flag (allowed flags: JSON_INVALID_UTF8_IGNORE)",
        )));
    }
    if json.is_empty() {
        set_last_error(JsonError::Syntax);
        return Ok(false);
    }
    if depth <= 0 {
        return Err(host.throw_error(PhpError::new(
            goro_runtime::ErrorKind::ValueError,
            "json_validate(): Argument #2 ($depth) must be greater than 0",
        )));
    }
    if depth > i32::MAX as i64 {
        return Err(host.throw_error(PhpError::new(
            goro_runtime::ErrorKind::ValueError,
            "json_validate(): Argument #2 ($depth) must be less than 2147483647",
        )));
    }
    let mode = if flags & JSON_INVALID_UTF8_IGNORE != 0 {
        Utf8Mode::Ignore
    } else {
        Utf8Mode::Error
    };
    match validate(json, depth as u32, mode) {
        JsonError::None => Ok(true),
        e => {
            set_last_error(e);
            Ok(false)
        }
    }
}

/// What to do with malformed UTF-8 in strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Utf8Mode {
    /// `JSON_ERROR_UTF8`.
    Error,
    /// `JSON_INVALID_UTF8_IGNORE`: drop the bytes.
    Ignore,
    /// `JSON_INVALID_UTF8_SUBSTITUTE`: U+FFFD.
    Substitute,
}

impl Utf8Mode {
    /// Observed on PHP 8.5: when both flags are given, `json_decode`
    /// substitutes and `json_encode` ignores.
    pub fn from_decode_flags(flags: i64) -> Utf8Mode {
        if flags & JSON_INVALID_UTF8_SUBSTITUTE != 0 {
            Utf8Mode::Substitute
        } else if flags & JSON_INVALID_UTF8_IGNORE != 0 {
            Utf8Mode::Ignore
        } else {
            Utf8Mode::Error
        }
    }

    pub fn from_encode_flags(flags: i64) -> Utf8Mode {
        if flags & JSON_INVALID_UTF8_IGNORE != 0 {
            Utf8Mode::Ignore
        } else if flags & JSON_INVALID_UTF8_SUBSTITUTE != 0 {
            Utf8Mode::Substitute
        } else {
            Utf8Mode::Error
        }
    }
}
