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

pub mod breakiter;
pub mod calendar;
pub mod collator;
pub mod datefmt;
pub mod dtpg;
pub mod error;
pub mod grapheme;
pub mod idna;
pub mod locale;
pub mod msgfmt;
pub mod numfmt;
pub mod resbundle;
pub mod spoof;
pub mod timezone;
pub mod translit;
pub mod uconv;
pub mod uset;
pub mod utf16;

/// The intl crate, for the wiring.
pub use intl;

/// Inflates embedded raw-DEFLATE text data, kept for the process.
pub(crate) fn inflate_text(packed: &[u8]) -> &'static str {
    let raw = goro_ext_zlib::gzinflate(packed, 0).unwrap_or_default();
    Box::leak(String::from_utf8(raw).unwrap_or_default().into_boxed_str())
}
