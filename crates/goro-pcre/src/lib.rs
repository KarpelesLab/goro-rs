//! A pure-Rust port of the PCRE2 10.44 regular expression library (8-bit
//! code units, interpreter only), as bundled with php-src.
//!
//! The port follows the C sources closely (pcre2_compile.c, pcre2_match.c,
//! pcre2_study.c, pcre2_auto_possess.c, ...) so that compile errors and
//! their offsets, match results, (*MARK) names and match/depth limit
//! behaviour are identical to PHP's.

#![allow(non_upper_case_globals)]
// The code mirrors the C sources statement by statement; these style lints
// would obscure the correspondence.
#![allow(
    clippy::needless_range_loop,
    clippy::if_same_then_else,
    clippy::needless_late_init,
    clippy::manual_range_patterns
)]

mod auto_possess;
mod cb;
mod code;
mod compile;
mod consts;
#[rustfmt::skip]
mod consts_gen;
#[rustfmt::skip]
mod errtexts;
mod lookbehind;
mod matcher;
mod parse;
mod study;
#[rustfmt::skip]
mod tables;
mod util;

pub use code::{Code, CompileContext, CompileError, CtypeInfo, maketables};
pub use matcher::{MatchContext, MatchData};

/// Public option and error constants (pcre2.h).
pub mod constants {
    pub use crate::consts_gen::*;
    pub const PCRE2_UNSET: usize = usize::MAX;
}

pub use util::error_message;

/// Validate a UTF-8 string: Err((PCRE2 error code, offset)) on failure.
pub fn validate_utf8(s: &[u8]) -> Result<(), (i32, usize)> {
    util::valid_utf(s)
}

/// The PCRE2 version string, as reported by PCRE2_CONFIG_VERSION.
pub const VERSION: &str = "10.44 2024-06-07";

/// The Unicode version of the property tables.
pub const UNICODE_VERSION: &str = tables::UNICODE_VERSION;
