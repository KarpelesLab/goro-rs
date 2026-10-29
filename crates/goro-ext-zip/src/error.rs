//! Error codes in libzip's numbering, which PHP exposes as the
//! `ZipArchive::ER_*` constants (PHP manual: ZipArchive constants), with
//! the descriptions `ZipArchive::getStatusString()` returns (libzip manual:
//! zip_errors(3); tests ext/zip/tests/oo_getstatusstring.phpt and others).

/// The `ER_*` codes.
pub mod er {
    pub const OK: i32 = 0;
    pub const MULTIDISK: i32 = 1;
    pub const RENAME: i32 = 2;
    pub const CLOSE: i32 = 3;
    pub const SEEK: i32 = 4;
    pub const READ: i32 = 5;
    pub const WRITE: i32 = 6;
    pub const CRC: i32 = 7;
    pub const ZIPCLOSED: i32 = 8;
    pub const NOENT: i32 = 9;
    pub const EXISTS: i32 = 10;
    pub const OPEN: i32 = 11;
    pub const TMPOPEN: i32 = 12;
    pub const ZLIB: i32 = 13;
    pub const MEMORY: i32 = 14;
    pub const CHANGED: i32 = 15;
    pub const COMPNOTSUPP: i32 = 16;
    pub const EOF: i32 = 17;
    pub const INVAL: i32 = 18;
    pub const NOZIP: i32 = 19;
    pub const INTERNAL: i32 = 20;
    pub const INCONS: i32 = 21;
    pub const REMOVE: i32 = 22;
    pub const DELETED: i32 = 23;
    pub const ENCRNOTSUPP: i32 = 24;
    pub const RDONLY: i32 = 25;
    pub const NOPASSWD: i32 = 26;
    pub const WRONGPASSWD: i32 = 27;
    pub const OPNOTSUPP: i32 = 28;
    pub const INUSE: i32 = 29;
    pub const TELL: i32 = 30;
    pub const COMPRESSED_DATA: i32 = 31;
    pub const CANCELLED: i32 = 32;
    pub const DATA_LENGTH: i32 = 33;
    pub const NOT_ALLOWED: i32 = 34;
    pub const TRUNCATED_ZIP: i32 = 35;
}

/// An error: the `ER_*` code and, for system errors, the `errno` (PHP's
/// `ZipArchive::$statusSys`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Error {
    pub code: i32,
    pub sys: i32,
}

impl Error {
    pub const fn new(code: i32) -> Error {
        Error { code, sys: 0 }
    }

    pub const fn with_sys(code: i32, sys: i32) -> Error {
        Error { code, sys }
    }

    /// From an I/O error: `code` with its `errno`.
    pub fn io(code: i32, e: &std::io::Error) -> Error {
        Error {
            code,
            sys: e.raw_os_error().unwrap_or(0),
        }
    }

    /// The description, with the system error appended for the codes that
    /// carry one ("Can't open file: No such file or directory").
    pub fn message(&self) -> String {
        message(self.code, self.sys)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Error {}

/// The description of a code alone.
pub fn description(code: i32) -> &'static str {
    match code {
        er::OK => "No error",
        er::MULTIDISK => "Multi-disk zip archives not supported",
        er::RENAME => "Renaming temporary file failed",
        er::CLOSE => "Closing zip archive failed",
        er::SEEK => "Seek error",
        er::READ => "Read error",
        er::WRITE => "Write error",
        er::CRC => "CRC error",
        er::ZIPCLOSED => "Containing zip archive was closed",
        er::NOENT => "No such file",
        er::EXISTS => "File already exists",
        er::OPEN => "Can't open file",
        er::TMPOPEN => "Failure to create temporary file",
        er::ZLIB => "Zlib error",
        er::MEMORY => "Malloc failure",
        er::CHANGED => "Entry has been changed",
        er::COMPNOTSUPP => "Compression method not supported",
        er::EOF => "Premature end of file",
        er::INVAL => "Invalid argument",
        er::NOZIP => "Not a zip archive",
        er::INTERNAL => "Internal error",
        er::INCONS => "Zip archive inconsistent",
        er::REMOVE => "Can't remove file",
        er::DELETED => "Entry has been deleted",
        er::ENCRNOTSUPP => "Encryption method not supported",
        er::RDONLY => "Read-only archive",
        er::NOPASSWD => "No password provided",
        er::WRONGPASSWD => "Wrong password provided",
        er::OPNOTSUPP => "Operation not supported",
        er::INUSE => "Resource still in use",
        er::TELL => "Tell error",
        er::COMPRESSED_DATA => "Compressed data invalid",
        er::CANCELLED => "Operation cancelled",
        er::DATA_LENGTH => "Unexpected length of data",
        er::NOT_ALLOWED => "Not allowed in torrentzip",
        er::TRUNCATED_ZIP => "Possibly truncated or corrupted zip archive",
        _ => "Unknown error",
    }
}

/// Whether the code's `sys` part is an `errno`.
pub fn has_errno(code: i32) -> bool {
    matches!(
        code,
        er::RENAME
            | er::CLOSE
            | er::SEEK
            | er::READ
            | er::WRITE
            | er::OPEN
            | er::TMPOPEN
            | er::REMOVE
            | er::TELL
    )
}

/// The description of `code` with `sys`.
pub fn message(code: i32, sys: i32) -> String {
    let base = description(code);
    if has_errno(code) && sys != 0 {
        format!("{base}: {}", strerror(sys))
    } else if code == er::ZLIB && sys != 0 {
        let z = match sys {
            -2 => "stream error",
            -3 => "data error",
            -4 => "insufficient memory",
            -5 => "buffer error",
            -6 => "incompatible version",
            _ => "unknown error",
        };
        format!("{base}: {z}")
    } else {
        base.to_string()
    }
}

/// The C library's text for `errno` (without Rust's " (os error N)").
pub fn strerror(errno: i32) -> String {
    let s = std::io::Error::from_raw_os_error(errno).to_string();
    match s.rfind(" (os error ") {
        Some(p) => s[..p].to_string(),
        None => s,
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        assert_eq!(Error::new(er::OK).message(), "No error");
        assert_eq!(Error::new(er::INVAL).message(), "Invalid argument");
        assert_eq!(
            Error::with_sys(er::OPEN, 2).message(),
            "Can't open file: No such file or directory"
        );
        assert_eq!(Error::new(er::EXISTS).message(), "File already exists");
    }
}
