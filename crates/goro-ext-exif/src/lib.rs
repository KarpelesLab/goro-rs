//! PHP's exif extension (PHP manual: Exchangeable image information) and
//! the image functions of ext/standard (PHP manual: Image Processing and
//! GD, "GD and Image Functions" that work without GD: `getimagesize`,
//! `image_type_to_mime_type`, `iptcparse`, `iptcembed`), on bytes.
//!
//! Behaviour follows the PHP manual, the JPEG (ITU T.81, JFIF), TIFF 6.0,
//! Exif 2.3 (CIPA DC-008), PNG, GIF89a, BMP, WebP, ISO BMFF/HEIF/AVIF,
//! JPEG 2000 (ISO/IEC 15444-1) and IPTC-NAA IIM specifications, the
//! expected output of php-src's `ext/exif/tests` and
//! `ext/standard/tests/image` tests, and observations of PHP 8.5.
//!
//! The engine wiring (streams, ini settings, PHP values) lives in the
//! engine's `builtins/exif.rs` and `builtins/image.rs`; this crate only
//! sees byte buffers and returns plain data and diagnostics.

pub mod exif;
pub mod imageinfo;
pub mod iptc;
pub mod makernote;
pub mod tags;

/// A diagnostic raised while reading an image, in the order PHP raises
/// them.
#[derive(Debug, Clone, PartialEq)]
pub enum Diag {
    /// `E_NOTICE`: "Error reading from %s!" (the caller supplies the name).
    ReadError,
    /// `E_WARNING` with the message after the function prefix.
    Warning(String),
    /// `E_NOTICE` with the message after the function prefix.
    Notice(String),
    /// `E_WARNING` prefixed with the bare function name (`f(): msg`)
    /// even when the file name is known.
    WarningBare(String),
    /// `E_NOTICE` prefixed with the bare function name.
    NoticeBare(String),
}
