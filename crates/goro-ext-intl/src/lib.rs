//! PHP's intl extension, on the `intl` crate (a pure-Rust ICU analog).
//!
//! Each module implements one part of the extension as the PHP manual
//! and the expected output of php-src's ext/intl tests describe it, with
//! ICU's documented semantics (the ICU user guide and API reference) where
//! PHP defers to ICU. The engine wiring lives in goro-engine
//! (`builtins/intl`).
//!
//! Strings are UTF-8 at the API; positions PHP reports in UTF-16 code
//! units (as ICU counts them) are converted where the PHP API exposes
//! them.

pub mod collator;
pub mod error;
pub mod locale;
pub mod msgfmt;
pub mod numfmt;
pub mod utf16;
