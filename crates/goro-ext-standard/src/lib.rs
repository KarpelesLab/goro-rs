//! PHP standard extension: string, math, array and other core functions.
//!
//! Everything here works on plain Rust values (`&[u8]` strings, `i64`,
//! `f64`); the engine wires the functions to PHP values.

pub mod argon2;
pub mod crypt;
pub mod encoding;
pub mod error;
pub mod float;
pub mod html;
pub mod ini;
pub mod math;
/// `mt_rand()` and friends live in goro-ext-random; re-exported here for
/// the string and array functions that draw from the global generator.
pub mod mt_rand {
    pub use goro_ext_random::mt19937::*;
}
pub mod natcmp;
pub mod phonetic;
pub mod printf;
pub mod sort;
pub mod strings;
pub mod url;
