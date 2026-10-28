//! PHP object support shared by both classes: `__serialize()`,
//! `__unserialize()` and `__debugInfo()` arrays.

use goro_runtime::{Key, KeyRef, PhpArray, PhpStr, Value};

/// `__serialize()`: `[["uri" => $uri], []]` (test 031.phpt).
pub fn serialize_array(uri: &[u8]) -> PhpArray {
    let first = PhpArray::from_pairs([(Key::Str(PhpStr::from("uri")), Value::from(uri))]);
    PhpArray::from_values([Value::from(first), Value::from(PhpArray::new())])
}

/// `__unserialize(array $data)`: the URI string of well-formed data, `None`
/// otherwise. Well-formed is exactly two entries with the integer keys 0
/// and 1 (in any order), entry 0 an array holding only the string entry
/// `"uri"`, and entry 1 an empty array (test 031.phpt; observed on
/// PHP 8.5). The caller then parses the string without a base; data that
/// is not well-formed, a string that does not parse, and a call on an
/// already initialized object all throw `Exception("Invalid serialization
/// data for Uri\Rfc3986\Uri object")` (resp. `Uri\WhatWg\Url`; test
/// 065.phpt).
pub fn unserialize_uri(data: &PhpArray) -> Option<PhpStr> {
    if data.len() != 2 {
        return None;
    }
    let first = data.get(KeyRef::Int(0))?.deref();
    let second = data.get(KeyRef::Int(1))?.deref();
    let first = first.as_array()?;
    if !second.as_array()?.is_empty() || first.len() != 1 {
        return None;
    }
    let uri = first.get_str(&PhpStr::from("uri"))?.deref();
    uri.as_str().cloned()
}

/// A `__debugInfo()` entry value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugValue<'a> {
    Null,
    Str(&'a [u8]),
    Int(i64),
}

/// Builds the `__debugInfo()` array from `(name, value)` entries.
pub fn debug_array<'a>(
    entries: impl IntoIterator<Item = (&'static str, DebugValue<'a>)>,
) -> PhpArray {
    PhpArray::from_pairs(entries.into_iter().map(|(k, v)| {
        (
            Key::Str(PhpStr::from(k)),
            match v {
                DebugValue::Null => Value::Null,
                DebugValue::Str(s) => Value::from(s),
                DebugValue::Int(i) => Value::from(i),
            },
        )
    }))
}
