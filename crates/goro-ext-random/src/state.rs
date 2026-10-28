//! Engine state for `__serialize()`, `__unserialize()` and `__debugInfo()`.
//!
//! The native engines serialize as `[[], <state>]`: an empty property array
//! then the state list, whose items are hex strings of the state words'
//! little-endian bytes (plus two ints for Mt19937). `__debugInfo()` shows
//! the same list as `["__states" => <state>]` (tests
//! 02_engine/*_serialize.phpt, pcgoneseq128xslrr64_seed.phpt,
//! xoshiro256starstar_seed.phpt).
//!
//! # Unserializing
//!
//! Observed on PHP 8.5 (tests/fixtures/serial.txt), every failure being
//! [`crate::invalid_serialization`] for the engine's class:
//!
//! - the data array must have exactly two elements, with keys 0 and 1 (in
//!   any order);
//! - element 0 must be an array of properties, and for the native engines
//!   it must be empty;
//! - element 1 must be an array: the wiring passes its values for keys
//!   `0..count` to `from_state` as [`StateValue`]s, using
//!   [`StateValue::Missing`] for an absent key, where `count` is the
//!   array's element count.
//!
//! `Random\Randomizer`'s data is `[["engine" => <engine object>]]`: exactly
//! one element (key 0), an array with exactly the one key `"engine"`, whose
//! value is an object implementing `Random\Engine`; anything else is
//! "Invalid serialization data for Random\Randomizer object", as is
//! calling `__unserialize()` on an already constructed Randomizer (tests
//! 03_randomizer/gh_9186_unserialize.phpt, gh_19765_unserialize.phpt).

/// One item of a serialized state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateItem {
    /// A string of lowercase hex digits.
    Hex(String),
    /// An int.
    Int(i64),
}

impl StateItem {
    pub(crate) fn hex(bytes: &[u8]) -> StateItem {
        const D: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            s.push(D[(b >> 4) as usize] as char);
            s.push(D[(b & 15) as usize] as char);
        }
        StateItem::Hex(s)
    }
}

/// One value of the state array handed to `__unserialize()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateValue<'a> {
    /// A string.
    Str(&'a [u8]),
    /// An int.
    Int(i64),
    /// Any other type.
    Other,
    /// No element with this key.
    Missing,
}

/// Exactly `2 * N` hex digits (either case) to `N` bytes.
pub(crate) fn parse_hex<const N: usize>(s: &[u8]) -> Option<[u8; N]> {
    if s.len() != 2 * N {
        return None;
    }
    let digit = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (digit(s[2 * i])? << 4) | digit(s[2 * i + 1])?;
    }
    Some(out)
}

/// Parses `K` 64-bit words (each 16 hex digits of little-endian bytes).
pub(crate) fn parse_words<const K: usize>(items: &[StateValue<'_>]) -> Option<[u64; K]> {
    if items.len() != K {
        return None;
    }
    let mut out = [0u64; K];
    for (o, item) in out.iter_mut().zip(items) {
        let StateValue::Str(s) = item else {
            return None;
        };
        *o = u64::from_le_bytes(parse_hex::<8>(s)?);
    }
    Some(out)
}
