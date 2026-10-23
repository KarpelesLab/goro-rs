//! A pure-Rust port of Oniguruma 6.9.10 (the regex library behind PHP's
//! mb_ereg* functions), restricted to what PHP uses.

pub mod enc;
pub mod enc_tables;
#[cfg(test)]
mod enc_tests;
pub mod unicode_data;
