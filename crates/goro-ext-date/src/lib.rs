//! PHP date and time: formatting, parsing, time zones, DateTime arithmetic.
//!
//! Everything works on plain Rust values; the engine wires the functions
//! to PHP's classes and functions. Messages are returned as values for the
//! wiring layer to raise.
//!
//! Modules and the PHP features they serve:
//!
//! - [`civil`]: proleptic Gregorian calendar over the whole `i64` range
//!   (days from/to dates, ISO weeks, day of year, leap years).
//! - [`tz`]: the embedded IANA database (tzdata 2026e compiled by
//!   `tools/gen_tzdata.py`), PHP's abbreviation list and the three zone
//!   kinds of DateTimeZone ([`TimeZone::parse`] is the constructor,
//!   [`tz::abbr::name_from_abbr`] is timezone_name_from_abbr,
//!   [`tz::db::php_version`] is timezone_version_get).
//! - [`time`]: [`DateTime`] (Unix time, microseconds, zone) with the
//!   setters (setDate, setISODate, setTime, setTimestamp, setTimezone) and
//!   [`time::date`] (date/gmdate).
//! - [`mod@format`]: DateTimeInterface::format and the DATE_* constants.
//! - [`parse`]: the free-form string parser; [`parse::Parsed::date_parse`]
//!   is date_parse().
//! - [`resolve`]: strtotime ([`resolve::strtotime`]), the DateTime
//!   constructor ([`resolve::create`]) and modify ([`resolve::modify`]).
//! - [`from_format`]: createFromFormat and date_parse_from_format.
//! - [`interval`]: DateInterval (ISO durations, createFromDateString,
//!   format) and diff/add/sub.
//! - [`period`]: DatePeriod iteration.
//! - [`api`]: mktime, checkdate, idate, getdate, localtime, strftime,
//!   listIdentifiers, getTransitions, date_default_timezone_set rules.
//! - [`sun`]: date_sun_info, date_sunrise, date_sunset.

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
