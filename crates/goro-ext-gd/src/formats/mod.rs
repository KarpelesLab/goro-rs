//! Image file formats. Each decoder returns the image or a
//! [`DecodeError`] holding the diagnostics PHP prints before its "... is
//! not a valid X file" warning.

pub mod avif;
pub mod bmp;
pub mod gd;
pub mod gif;
pub mod jpeg;
pub mod png;
pub mod tga;
pub mod wbmp;
pub mod webp;
pub mod xbm;
pub mod xpm;

use crate::image::Image;

/// Warnings printed before the final "not a valid" one (without the
/// `func(): ` prefix).
#[derive(Debug, Default)]
pub struct DecodeError {
    pub warnings: Vec<String>,
}

impl DecodeError {
    pub fn silent() -> DecodeError {
        DecodeError::default()
    }
    pub fn with(w: &str) -> DecodeError {
        DecodeError {
            warnings: vec![w.to_string()],
        }
    }
}

pub type DecodeResult = Result<Image, DecodeError>;

/// The allocation-overflow warning of GD (bug77479, libgd00101).
pub const OVERFLOW_MSG: &str = "Product of memory allocation multiplication would exceed INT_MAX, failing operation gracefully\n";
pub const NEGATIVE_MSG: &str = "One parameter to a memory allocation multiplication is negative or zero, failing operation gracefully\n";

/// Formats imagecreatefromstring() recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Gif,
    Gd2,
    Wbmp,
    Webp,
    Bmp,
    Avif,
    Gd,
}

/// Sniffs the format of image data (PHP manual, imagecreatefromstring:
/// "JPEG, PNG, GIF, BMP, WBMP, GD2, WEBP and AVIF").
pub fn sniff(d: &[u8]) -> Option<Kind> {
    if d.len() >= 3 && d[..3] == [0xff, 0xd8, 0xff] {
        return Some(Kind::Jpeg);
    }
    if d.len() >= 8 && &d[..8] == png::SIGNATURE {
        return Some(Kind::Png);
    }
    if d.len() >= 6 && (&d[..6] == b"GIF87a" || &d[..6] == b"GIF89a") {
        return Some(Kind::Gif);
    }
    if d.len() >= 4 && &d[..4] == b"gd2\0" {
        return Some(Kind::Gd2);
    }
    if d.len() >= 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        return Some(Kind::Webp);
    }
    if d.len() >= 2 && &d[..2] == b"BM" {
        return Some(Kind::Bmp);
    }
    if avif::is_avif(d) {
        return Some(Kind::Avif);
    }
    if wbmp::looks_like(d) {
        return Some(Kind::Wbmp);
    }
    None
}
