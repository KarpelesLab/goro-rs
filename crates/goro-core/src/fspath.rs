//! Byte-exact file names.
//!
//! PHP file names are byte strings. Inside goro a path often travels as a
//! Rust `String` in the diagnostic-message encoding of
//! [`crate::string::msg_str`] (bytes that are not UTF-8 become the
//! private-use characters U+10FF00 + byte), so it can go into a warning
//! unchanged; these helpers turn such a string (or raw bytes) into the OS
//! path with the original bytes, and OS names back into PHP bytes.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The raw bytes of a path in the message encoding.
pub fn msg_bytes(s: &str) -> Cow<'_, [u8]> {
    crate::errdisplay::raw_error_bytes(s)
}

/// The OS path of raw bytes.
#[cfg(unix)]
pub fn bytes_path(b: &[u8]) -> &Path {
    use std::os::unix::ffi::OsStrExt;
    Path::new(OsStr::from_bytes(b))
}

#[cfg(not(unix))]
pub fn bytes_path(b: &[u8]) -> &Path {
    Path::new(std::str::from_utf8(b).unwrap_or("\u{FFFD}"))
}

/// The OS path of an owned byte vector.
#[cfg(unix)]
pub fn vec_path(v: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(v))
}

#[cfg(not(unix))]
pub fn vec_path(v: Vec<u8>) -> PathBuf {
    PathBuf::from(crate::string::lossy(&v).into_owned())
}

/// The OS path of a path in the message encoding (see the module doc).
pub fn msg_path(s: &str) -> Cow<'_, Path> {
    match msg_bytes(s) {
        Cow::Borrowed(b) => Cow::Borrowed(bytes_path(b)),
        Cow::Owned(v) => Cow::Owned(vec_path(v)),
    }
}

/// The bytes of an OS string (a file name read from the system).
#[cfg(unix)]
pub fn os_bytes(o: &OsStr) -> Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    Cow::Borrowed(o.as_bytes())
}

#[cfg(not(unix))]
pub fn os_bytes(o: &OsStr) -> Cow<'_, [u8]> {
    match o.to_string_lossy() {
        Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
        Cow::Owned(s) => Cow::Owned(s.into_bytes()),
    }
}

/// An OS string (file name, path) in the message encoding.
pub fn os_msg(o: &OsStr) -> String {
    crate::string::msg_str(&os_bytes(o)).into_owned()
}

/// A path in the message encoding.
pub fn path_msg(p: &Path) -> String {
    os_msg(p.as_os_str())
}

/// The C string of a path in the message encoding (None when it holds a
/// NUL byte).
pub fn msg_cstring(s: &str) -> Option<std::ffi::CString> {
    std::ffi::CString::new(msg_bytes(s).into_owned()).ok()
}

/// The PHP string of an OS path (its raw bytes).
pub fn path_php(p: &Path) -> crate::string::PhpString {
    crate::string::PhpString::from_vec(os_bytes(p.as_os_str()).into_owned())
}
