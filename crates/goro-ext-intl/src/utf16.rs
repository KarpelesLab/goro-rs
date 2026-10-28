//! UTF-8 / UTF-16 offset helpers: ICU counts positions in UTF-16 code
//! units, PHP strings are UTF-8 bytes.

/// The number of UTF-16 code units of `s`.
pub fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// The byte offset in `s` of UTF-16 offset `u` (clamped to the end; an
/// offset inside a surrogate pair maps to the character's start).
pub fn byte_of_u16(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= u {
            return i;
        }
        n += c.len_utf16();
        if n > u {
            return i;
        }
    }
    s.len()
}

/// The UTF-16 offset of byte offset `b` of `s`.
pub fn u16_of_byte(s: &str, b: usize) -> usize {
    s[..b.min(s.len())].chars().map(char::len_utf16).sum()
}

/// Decodes UTF-16 code units, unpaired surrogates as U+FFFD.
pub fn from_u16(v: &[u16]) -> String {
    char::decode_utf16(v.iter().copied())
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}
