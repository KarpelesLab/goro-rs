use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::value::{memory_alloc_reporting, memory_free};

/// Accounted size of a `len`-byte string: a zend_string (24-byte header,
/// the bytes and a NUL) as the Zend allocator sizes it.
#[inline]
fn zstr_size(len: usize) -> usize {
    crate::value::zend_mm_size(len + 25)
}

/// Whether `b` holds the byte `c`, eight bytes at a time (a name or a
/// key: short).
#[inline]
pub fn has_byte(b: &[u8], c: u8) -> bool {
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    let pat = LO.wrapping_mul(u64::from(c));
    let (words, tail) = b.as_chunks::<8>();
    for w in words {
        let x = u64::from_ne_bytes(*w) ^ pat;
        if x.wrapping_sub(LO) & !x & HI != 0 {
            return true;
        }
    }
    tail.contains(&c)
}

/// `a == b` of short byte strings without a call of memcmp: a few word
/// loads (a name, an array key or a property name is usually short).
#[inline]
pub fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    let n = a.len();
    if n != b.len() {
        return false;
    }
    let w = |s: &[u8], at: usize| u64::from_ne_bytes(s[at..at + 8].try_into().unwrap());
    if (8..=16).contains(&n) {
        return w(a, 0) == w(b, 0) && w(a, n - 8) == w(b, n - 8);
    }
    if (17..=32).contains(&n) {
        return w(a, 0) == w(b, 0)
            && w(a, 8) == w(b, 8)
            && w(a, n - 16) == w(b, n - 16)
            && w(a, n - 8) == w(b, n - 8);
    }
    if (4..8).contains(&n) {
        let w = |s: &[u8], at: usize| u32::from_ne_bytes(s[at..at + 4].try_into().unwrap());
        return w(a, 0) == w(b, 0) && w(a, n - 4) == w(b, n - 4);
    }
    if n < 4 {
        return n == 0 || (a[0] == b[0] && a[n / 2] == b[n / 2] && a[n - 1] == b[n - 1]);
    }
    a == b
}

/// PHP binary-safe string with cached hash
#[derive(Clone)]
pub struct PhpString {
    inner: Rc<PhpStringInner>,
}

struct PhpStringInner {
    data: Vec<u8>,
    hash: std::cell::Cell<Option<u64>>,
    /// IS_STR_VALID_UTF8: set once the string is known to be valid UTF-8.
    valid_utf8: std::cell::Cell<bool>,
    /// An interned string (IS_STR_INTERNED, not refcounted in PHP): a
    /// compiled literal or a ZSTR_CHAR() one-byte string. Only reported by
    /// debug_zval_dump().
    interned: std::cell::Cell<bool>,
    tracked_bytes: usize,
}

impl Drop for PhpStringInner {
    fn drop(&mut self) {
        if self.tracked_bytes > 0 {
            memory_free(self.tracked_bytes);
        }
    }
}

impl PhpString {
    /// Whether both are the same string (not just equal ones).
    #[inline]
    pub fn ptr_eq(&self, other: &PhpString) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// The empty string (interned: one shared instance, like
    /// ZSTR_EMPTY_ALLOC()).
    pub fn empty() -> Self {
        thread_local! {
            static EMPTY: PhpString = PhpString {
                inner: Rc::new(PhpStringInner {
                    data: Vec::new(),
                    hash: std::cell::Cell::new(None),
                    valid_utf8: std::cell::Cell::new(false),
                    interned: std::cell::Cell::new(false),
                    tracked_bytes: 0, // empty strings don't track
                }),
            };
        }
        EMPTY.with(|e| e.clone())
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        let bytes = zstr_size(data.len());
        if !memory_alloc_reporting(bytes, (data.len() + 25).next_multiple_of(8)) {
            return Self::empty();
        }
        Self {
            inner: Rc::new(PhpStringInner {
                data: data.to_vec(),
                hash: std::cell::Cell::new(None),
                valid_utf8: std::cell::Cell::new(false),
                interned: std::cell::Cell::new(false),
                tracked_bytes: bytes,
            }),
        }
    }

    /// A string not counted by memory_get_usage() (like PHP's interned
    /// strings, e.g. compiled variable names).
    pub fn from_bytes_untracked(data: &[u8]) -> Self {
        Self {
            inner: Rc::new(PhpStringInner {
                data: data.to_vec(),
                hash: std::cell::Cell::new(None),
                valid_utf8: std::cell::Cell::new(false),
                interned: std::cell::Cell::new(false),
                tracked_bytes: 0,
            }),
        }
    }

    /// ZSTR_CHAR(c): the interned one-byte string (zend_one_char_string),
    /// shared and not counted by memory_get_usage(). String offset reads and
    /// chr() return these without allocating.
    pub fn one_char(c: u8) -> Self {
        thread_local! {
            static ONE_CHAR: Vec<PhpString> =
                (0..=255u8)
                    .map(|c| {
                        let s = PhpString::from_bytes_untracked(&[c]);
                        s.mark_interned();
                        s
                    })
                    .collect();
        }
        ONE_CHAR.with(|t| t[c as usize].clone())
    }

    /// Mark the string interned (see `is_interned`).
    pub fn mark_interned(&self) {
        self.inner.interned.set(true);
    }

    /// Clear the interned mark: the string is refcounted after all (e.g.
    /// the elements of a compile-time constant array; one-byte strings stay
    /// interned).
    pub fn unmark_interned(&self) {
        if self.inner.data.len() > 1 {
            self.inner.interned.set(false);
        }
    }

    /// ZSTR_IS_INTERNED(): a compiled literal, a one-byte string or the
    /// empty string.
    pub fn is_interned(&self) -> bool {
        self.inner.interned.get() || self.inner.data.is_empty()
    }

    /// The number of holders of this string (an approximation of its PHP
    /// refcount, for debug_zval_dump()).
    pub fn refcount(&self) -> usize {
        Rc::strong_count(&self.inner)
    }

    pub fn from_vec(data: Vec<u8>) -> Self {
        let bytes = zstr_size(data.len());
        if !memory_alloc_reporting(bytes, (data.len() + 25).next_multiple_of(8)) {
            return Self::empty();
        }
        Self {
            inner: Rc::new(PhpStringInner {
                data,
                hash: std::cell::Cell::new(None),
                valid_utf8: std::cell::Cell::new(false),
                interned: std::cell::Cell::new(false),
                tracked_bytes: bytes,
            }),
        }
    }

    pub fn from_string(s: String) -> Self {
        Self::from_vec(s.into_bytes())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.inner.data
    }

    pub fn len(&self) -> usize {
        self.inner.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.data.is_empty()
    }

    /// ZSTR_IS_VALID_UTF8(): the string was already validated as UTF-8.
    pub fn is_known_valid_utf8(&self) -> bool {
        self.inner.valid_utf8.get()
    }

    /// GC_ADD_FLAGS(str, IS_STR_VALID_UTF8)
    pub fn mark_valid_utf8(&self) {
        self.inner.valid_utf8.set(true);
    }

    /// The hash of the string as an array key (see PhpArray), computed on
    /// first use and kept with the string (zend_string's h): a key looked
    /// up again, or a table rebuilt, does not hash it again.
    #[inline]
    pub fn hash_value(&self) -> u64 {
        match self.inner.hash.get() {
            Some(h) => h,
            None => self.hash_slow(),
        }
    }

    #[inline(never)]
    fn hash_slow(&self) -> u64 {
        let h = crate::array::hash_bytes(&self.inner.data);
        self.inner.hash.set(Some(h));
        h
    }

    /// `$s .= $extra` without copying: when this is the only handle on the
    /// string (refcount 1, like zend_string_extend() on a non-interned
    /// string) the bytes are appended in place, so building a string with
    /// repeated `.=` is amortized O(1) per append instead of O(n).
    ///
    /// Returns false, leaving the string untouched, when it is shared; the
    /// caller then builds a new string as before.
    ///
    /// Memory accounting ends up exactly as if the result had been built
    /// with `from_vec(old + extra)`.  Going over memory_limit leaves the
    /// string unchanged and records the pending fatal (reported with the
    /// size of the whole new string, like the copying path), which the VM
    /// raises before the next opcode.
    pub fn append_in_place(&mut self, extra: &[u8]) -> bool {
        let Some(inner) = Rc::get_mut(&mut self.inner) else {
            return false;
        };
        if extra.is_empty() {
            return true;
        }
        let new_len = inner.data.len() + extra.len();
        // An untracked string (empty / interned) becomes a tracked one, as
        // a freshly built string would be.
        // zend_string_extend(): erealloc() to the new size.
        let new_size = zstr_size(new_len);
        let charge = new_size.saturating_sub(inner.tracked_bytes);
        if charge > 0
            && !crate::value::memory_alloc_reporting(charge, (new_len + 25).next_multiple_of(8))
        {
            return true;
        }
        inner.tracked_bytes += charge;
        inner.data.extend_from_slice(extra);
        inner.hash.set(None);
        // The result is a new (refcounted) string, not known to be UTF-8.
        inner.valid_utf8.set(false);
        inner.interned.set(false);
        true
    }

    /// Lossy conversion to a Rust String (for display purposes)
    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.inner.data).into_owned()
    }

    /// The string for a diagnostic message (see [`msg_str`]).
    pub fn to_msg_string(&self) -> String {
        msg_str(&self.inner.data).into_owned()
    }

    /// Write the raw bytes to a writer
    pub fn write_to(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        writer.write_all(&self.inner.data)
    }
}

impl PartialEq for PhpString {
    fn eq(&self, other: &Self) -> bool {
        self.inner.data == other.inner.data
    }
}

impl Eq for PhpString {}

impl Hash for PhpString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.data.hash(state);
    }
}

impl std::fmt::Debug for PhpString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PhpString({:?})", self.to_string_lossy())
    }
}

impl std::fmt::Display for PhpString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_lossy())
    }
}

/// Text of `b` for a diagnostic (warning or exception) message. Messages
/// travel as Rust strings: bytes that are not UTF-8 are kept as the
/// plane-16 private-use characters U+10FF00 + byte, which the error display,
/// error handlers and exception messages turn back into the raw bytes
/// (`errdisplay::raw_error_bytes`), as php's messages carry them unchanged.
pub fn msg_str(b: &[u8]) -> std::borrow::Cow<'_, str> {
    let mut rest = match std::str::from_utf8(b) {
        Ok(s) => return std::borrow::Cow::Borrowed(s),
        Err(_) => b,
    };
    let mut out = String::with_capacity(b.len() + 8);
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return std::borrow::Cow::Owned(out);
            }
            Err(e) => {
                let (ok, bad) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(ok).unwrap_or_default());
                let n = e.error_len().unwrap_or(bad.len()).max(1);
                for &c in &bad[..n] {
                    out.push(char::from_u32(0x10FF00 + c as u32).unwrap_or('?'));
                }
                rest = &bad[n..];
            }
        }
    }
}

/// One byte of a diagnostic message, kept raw (see [`msg_str`]).
pub fn msg_char(b: u8) -> char {
    if b < 0x80 {
        b as char
    } else {
        char::from_u32(0x10FF00 + b as u32).unwrap_or('?')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes() {
        let s = PhpString::from_bytes(b"hello");
        assert_eq!(s.as_bytes(), b"hello");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn test_binary_safe() {
        let s = PhpString::from_bytes(b"hello\x00world");
        assert_eq!(s.len(), 11);
        assert_eq!(s.as_bytes(), b"hello\x00world");
    }

    #[test]
    fn test_hash_caching() {
        let s = PhpString::from_bytes(b"test");
        let h1 = s.hash_value();
        let h2 = s.hash_value();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_equality() {
        let a = PhpString::from_bytes(b"hello");
        let b = PhpString::from_bytes(b"hello");
        let c = PhpString::from_bytes(b"world");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
