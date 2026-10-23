//! Exceptions (ExceptionInfo): a severity (ExceptionType) and the message
//! as ImageMagick words it.

/// ExceptionType values of ImageMagick 7.
pub mod severity {
    pub const UNDEFINED: i32 = 0;
    pub const RESOURCE_LIMIT_WARNING: i32 = 300;
    pub const TYPE_WARNING: i32 = 305;
    pub const OPTION_WARNING: i32 = 310;
    pub const DELEGATE_WARNING: i32 = 315;
    pub const MISSING_DELEGATE_WARNING: i32 = 320;
    pub const CORRUPT_IMAGE_WARNING: i32 = 325;
    pub const FILE_OPEN_WARNING: i32 = 330;
    pub const BLOB_WARNING: i32 = 335;
    pub const IMAGE_WARNING: i32 = 365;
    pub const WAND_WARNING: i32 = 370;
    pub const RESOURCE_LIMIT_ERROR: i32 = 400;
    pub const TYPE_ERROR: i32 = 405;
    pub const OPTION_ERROR: i32 = 410;
    pub const DELEGATE_ERROR: i32 = 415;
    pub const MISSING_DELEGATE_ERROR: i32 = 420;
    pub const CORRUPT_IMAGE_ERROR: i32 = 425;
    pub const FILE_OPEN_ERROR: i32 = 430;
    pub const BLOB_ERROR: i32 = 435;
    pub const STREAM_ERROR: i32 = 440;
    pub const CACHE_ERROR: i32 = 445;
    pub const CODER_ERROR: i32 = 450;
    pub const FILTER_ERROR: i32 = 452;
    pub const MODULE_ERROR: i32 = 455;
    pub const DRAW_ERROR: i32 = 460;
    pub const IMAGE_ERROR: i32 = 465;
    pub const WAND_ERROR: i32 = 470;
}

#[derive(Clone, Debug, PartialEq)]
pub struct MagickError {
    pub severity: i32,
    pub message: String,
}

impl MagickError {
    pub fn new(severity: i32, message: impl Into<String>) -> MagickError {
        MagickError {
            severity,
            message: message.into(),
        }
    }

    /// "reason `description' @ location", the way ThrowMagickException
    /// composes messages.
    pub fn with_desc(severity: i32, reason: &str, desc: &str, location: &str) -> MagickError {
        let msg = if desc.is_empty() {
            format!("{} @ {}", reason, location)
        } else {
            format!("{} `{}' @ {}", reason, desc, location)
        };
        MagickError::new(severity, msg)
    }

    pub fn no_decode_delegate(name: &str) -> MagickError {
        MagickError::with_desc(
            severity::MISSING_DELEGATE_ERROR,
            "no decode delegate for this image format",
            name,
            "error/constitute.c/ReadImage/752",
        )
    }

    pub fn no_blob_delegate(magick: &str) -> MagickError {
        MagickError::with_desc(
            severity::MISSING_DELEGATE_ERROR,
            "no decode delegate for this image format",
            magick,
            "error/blob.c/BlobToImage/481",
        )
    }

    pub fn no_encode_delegate(name: &str) -> MagickError {
        MagickError::with_desc(
            severity::MISSING_DELEGATE_ERROR,
            "no encode delegate for this image format",
            name,
            "error/constitute.c/WriteImage/1404",
        )
    }

    pub fn corrupt(format: &str, why: &str) -> MagickError {
        MagickError::with_desc(
            severity::CORRUPT_IMAGE_ERROR,
            "improper image header",
            why,
            &format!(
                "error/{}.c/Read{}Image",
                format.to_ascii_lowercase(),
                format
            ),
        )
    }

    pub fn option(reason: &str, desc: &str) -> MagickError {
        MagickError::with_desc(
            severity::OPTION_ERROR,
            reason,
            desc,
            "error/option.c/ParseCommandOption",
        )
    }

    pub fn image(reason: &str, desc: &str) -> MagickError {
        MagickError::with_desc(severity::IMAGE_ERROR, reason, desc, "error/image.c/Image")
    }
}

impl std::fmt::Display for MagickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MagickError {}

pub type MResult<T> = Result<T, MagickError>;
