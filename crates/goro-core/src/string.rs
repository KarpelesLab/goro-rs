use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::value::{memory_alloc, memory_free};

/// Overhead per PhpString allocation (Rc + Cell + Vec header)
/// (zend_string: 24-byte header + NUL, rounded)
const STRING_OVERHEAD: usize = 32;

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
    pub fn empty() -> Self {
        Self {
            inner: Rc::new(PhpStringInner {
                data: Vec::new(),
                hash: std::cell::Cell::new(None),
                valid_utf8: std::cell::Cell::new(false),
                interned: std::cell::Cell::new(false),
                tracked_bytes: 0, // empty strings don't track
            }),
        }
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        let bytes = data.len() + STRING_OVERHEAD;
        if !memory_alloc(bytes) {
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
        let bytes = data.len() + STRING_OVERHEAD;
        if !memory_alloc(bytes) {
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

    /// Get the hash, computing and caching it on first access
    pub fn hash_value(&self) -> u64 {
        if let Some(h) = self.inner.hash.get() {
            return h;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.data.hash(&mut hasher);
        let h = hasher.finish();
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
        let charge = if inner.tracked_bytes == 0 {
            new_len + STRING_OVERHEAD
        } else {
            extra.len()
        };
        if !crate::value::memory_alloc_reporting(charge, new_len + STRING_OVERHEAD) {
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
