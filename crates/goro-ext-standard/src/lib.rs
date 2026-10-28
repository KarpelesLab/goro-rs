//! PHP standard extension: string, math, array and other core functions.
//!
//! Everything here works on plain Rust values (`&[u8]` strings, `i64`,
//! `f64`); the engine wires the functions to PHP values.

pub mod error;
pub mod float;
pub mod math;
pub mod mt_rand;
pub mod printf;
