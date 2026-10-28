//! Character set conversion and Unicode text operations for mbstring and iconv.
//!
//! The crate works on byte strings. [`Encoding`] lists every encoding PHP
//! 8.5's mbstring knows; [`convert`] converts between them with mbstring's
//! handling of invalid input ([`Subst`]); the other modules implement the
//! operations behind the `mb_*` functions on top of the codecs. [`iconv`]
//! is the separate glibc-flavoured converter behind the `iconv*` functions.

pub mod codec;
pub mod convert;
pub mod encoding;
pub mod iconv;
pub mod ops;
mod tables;

pub use codec::{ERR, Subst, SubstMode};
pub use convert::{Converted, check_encoding, convert, decode, encode, scrub};
pub use encoding::Encoding;
