//! Error values shared by the standard-library functions.
//!
//! The functions here run on plain Rust values, so instead of throwing they
//! return these values and the wiring layer raises them.

/// A PHP error raised by a standard-library function, for the wiring layer to
/// throw.
///
/// The message is the full `getMessage()` text, including the `func(): `
/// prefix when PHP prints one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdError {
    /// `ValueError`.
    ValueError(String),
    /// `ArgumentCountError`.
    ArgumentCountError(String),
    /// `TypeError`.
    TypeError(String),
    /// An `E_ERROR` fatal error.
    Fatal(String),
}

/// A diagnostic PHP emits without throwing.
///
/// The message excludes the `func(): ` prefix: the wiring layer adds the
/// current function name, as PHP does for every internal-function warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diag {
    /// `E_WARNING`.
    Warning(String),
    /// `E_NOTICE`.
    Notice(String),
    /// `E_DEPRECATED`.
    Deprecated(String),
}
