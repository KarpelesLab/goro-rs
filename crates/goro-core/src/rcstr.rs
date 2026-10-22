//! A shared, immutable string: cloning it copies a pointer (and bumps a
//! count) rather than the text.
//!
//! The executing file's name is copied into every call's backtrace frame
//! (and saved and restored around includes, callbacks and the like): as a
//! `String`, each copy was an allocation-free but still per-call memcpy of
//! the whole path, whose wide stores the following narrow loads could not
//! forward from. As an `RcStr`, it is a pointer copy.

use std::borrow::{Borrow, Cow};
use std::fmt;
use std::ops::Deref;
use std::rc::Rc;

/// A shared, immutable string (see the module documentation). The empty
/// string holds no allocation.
#[derive(Clone, Default)]
pub struct RcStr(Option<Rc<str>>);

impl RcStr {
    /// The empty string (no allocation).
    #[inline]
    pub const fn new() -> RcStr {
        RcStr(None)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Some(s) => s,
            None => "",
        }
    }

    /// Both are the same allocation (or both empty).
    #[inline]
    pub fn ptr_eq(&self, other: &RcStr) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Deref for RcStr {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for RcStr {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for RcStr {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl AsRef<std::path::Path> for RcStr {
    #[inline]
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(self.as_str())
    }
}

impl AsRef<std::ffi::OsStr> for RcStr {
    #[inline]
    fn as_ref(&self) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(self.as_str())
    }
}

impl Borrow<str> for RcStr {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for RcStr {
    #[inline]
    fn from(s: &str) -> RcStr {
        if s.is_empty() {
            RcStr(None)
        } else {
            RcStr(Some(Rc::from(s)))
        }
    }
}

impl From<String> for RcStr {
    #[inline]
    fn from(s: String) -> RcStr {
        if s.is_empty() {
            RcStr(None)
        } else {
            RcStr(Some(Rc::from(s)))
        }
    }
}

impl From<&String> for RcStr {
    #[inline]
    fn from(s: &String) -> RcStr {
        RcStr::from(s.as_str())
    }
}

impl From<&RcStr> for RcStr {
    #[inline]
    fn from(s: &RcStr) -> RcStr {
        s.clone()
    }
}

impl From<Cow<'_, str>> for RcStr {
    #[inline]
    fn from(s: Cow<'_, str>) -> RcStr {
        RcStr::from(s.as_ref())
    }
}

impl From<RcStr> for String {
    #[inline]
    fn from(s: RcStr) -> String {
        s.as_str().to_owned()
    }
}

impl From<&RcStr> for String {
    #[inline]
    fn from(s: &RcStr) -> String {
        s.as_str().to_owned()
    }
}

impl PartialEq for RcStr {
    #[inline]
    fn eq(&self, other: &RcStr) -> bool {
        self.ptr_eq(other) || self.as_str() == other.as_str()
    }
}

impl Eq for RcStr {}

impl PartialEq<str> for RcStr {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for RcStr {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for RcStr {
    #[inline]
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<RcStr> for str {
    #[inline]
    fn eq(&self, other: &RcStr) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<RcStr> for &str {
    #[inline]
    fn eq(&self, other: &RcStr) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<RcStr> for String {
    #[inline]
    fn eq(&self, other: &RcStr) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialOrd for RcStr {
    #[inline]
    fn partial_cmp(&self, other: &RcStr) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RcStr {
    #[inline]
    fn cmp(&self, other: &RcStr) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl std::hash::Hash for RcStr {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl fmt::Display for RcStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl fmt::Debug for RcStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::RcStr;

    #[test]
    fn basics() {
        let a = RcStr::from("/x/y.php");
        let b = a.clone();
        assert!(a.ptr_eq(&b));
        assert_eq!(a, "/x/y.php");
        assert_eq!(a, RcStr::from(String::from("/x/y.php")));
        assert!(RcStr::from("").is_empty());
        assert_eq!(RcStr::default(), RcStr::from(""));
        assert_eq!(format!("{a}"), "/x/y.php");
        assert_eq!(String::from(a), "/x/y.php");
    }
}
