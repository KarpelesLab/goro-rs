use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::value::{memory_alloc, memory_free};

/// Threshold for tracking string memory (only track large strings)
const STRING_TRACK_THRESHOLD: usize = 1024;

/// PHP binary-safe string with cached hash
#[derive(Clone)]
pub struct PhpString {
    inner: Rc<PhpStringInner>,
}

struct PhpStringInner {
    data: Vec<u8>,
    hash: std::cell::Cell<Option<u64>>,
    tracked_bytes: usize, // bytes tracked in memory_alloc (0 if below threshold)
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
                tracked_bytes: 0,
            }),
        }
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        let tracked = if data.len() >= STRING_TRACK_THRESHOLD {
            let bytes = data.len();
            if !memory_alloc(bytes) {
                // Over memory limit - return empty string
                return Self::empty();
            }
            bytes
        } else {
            0
        };
        Self {
            inner: Rc::new(PhpStringInner {
                data: data.to_vec(),
                hash: std::cell::Cell::new(None),
                tracked_bytes: tracked,
            }),
        }
    }

    pub fn from_vec(data: Vec<u8>) -> Self {
        let tracked = if data.len() >= STRING_TRACK_THRESHOLD {
            let bytes = data.len();
            if !memory_alloc(bytes) {
                return Self::empty();
            }
            bytes
        } else {
            0
        };
        Self {
            inner: Rc::new(PhpStringInner {
                data,
                hash: std::cell::Cell::new(None),
                tracked_bytes: tracked,
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
