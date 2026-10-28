//! Diagnostics and thrown errors produced by runtime operations.
//!
//! Runtime operations never print or throw by themselves. Non-fatal
//! diagnostics (warnings, deprecations, notices) are reported to the
//! engine through [`Ctx::notice`](crate::Ctx::notice) as they happen, in
//! PHP's order; the engine turns them into `E_WARNING`/`E_DEPRECATED`, runs
//! user error handlers, and so on. Fatal conditions that PHP raises as
//! exceptions are returned as `Err(PhpError)` and the engine throws them.

use std::fmt;

/// Severity of a non-fatal diagnostic, with PHP's `E_*` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// `E_WARNING` (2)
    Warning,
    /// `E_NOTICE` (8)
    Notice,
    /// `E_DEPRECATED` (8192)
    Deprecated,
}

impl Level {
    /// PHP's error number for this level.
    pub fn errno(self) -> i64 {
        match self {
            Level::Warning => 2,
            Level::Notice => 8,
            Level::Deprecated => 8192,
        }
    }
}

/// The class of a Throwable raised by a runtime operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// `Error`
    Error,
    /// `TypeError`
    TypeError,
    /// `ValueError`
    ValueError,
    /// `ArithmeticError`
    ArithmeticError,
    /// `DivisionByZeroError`
    DivisionByZero,
}

impl ErrorKind {
    /// The PHP class name.
    pub fn class_name(self) -> &'static str {
        match self {
            ErrorKind::Error => "Error",
            ErrorKind::TypeError => "TypeError",
            ErrorKind::ValueError => "ValueError",
            ErrorKind::ArithmeticError => "ArithmeticError",
            ErrorKind::DivisionByZero => "DivisionByZeroError",
        }
    }
}

/// An error PHP throws (the engine creates the exception object).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhpError {
    pub kind: ErrorKind,
    pub message: String,
}

impl PhpError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> PhpError {
        PhpError {
            kind,
            message: message.into(),
        }
    }
    pub fn type_error(message: impl Into<String>) -> PhpError {
        Self::new(ErrorKind::TypeError, message)
    }
    pub fn error(message: impl Into<String>) -> PhpError {
        Self::new(ErrorKind::Error, message)
    }
}

impl fmt::Display for PhpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.class_name(), self.message)
    }
}

impl std::error::Error for PhpError {}

/// Result of an operation that may throw.
pub type PhpResult<T> = Result<T, PhpError>;

/// A recorded diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub level: Level,
    pub message: String,
}
