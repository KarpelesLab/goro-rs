//! PHP date and time: formatting, parsing, time zones, DateTime arithmetic.
//!
//! Everything works on plain Rust values; the engine wires the functions
//! to PHP's classes and functions.
//!
//! - [`civil`]: proleptic Gregorian calendar over the whole `i64` range.
//! - [`tz`]: the embedded IANA database (tzdata compiled by
//!   `tools/gen_tzdata.py`), abbreviations and the three zone kinds.
//! - [`time`]: the DateTime value.
//! - [`format`]: `date()` / `DateTime::format()`.

pub mod api;
pub mod civil;
pub mod format;
pub mod from_format;
pub mod interval;
pub mod parse;
pub mod period;
pub mod resolve;
pub mod sun;
pub mod time;
pub mod tz;

pub use time::{DateTime, LocalTime};
pub use tz::TimeZone;
