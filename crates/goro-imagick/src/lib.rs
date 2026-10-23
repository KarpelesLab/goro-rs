//! goro-imagick: a pure-Rust image engine with the semantics of
//! ImageMagick 7's MagickWand API (Q16, non-HDRI), for goro-rs's Imagick
//! extension. Format I/O and most filters come from the OxideAV crates;
//! the image list, iterator, geometry strings, colours, compositing and
//! the geometry operations live here.
//!
//! Clean-room: the behaviour follows ImageMagick's public documentation
//! and black-box observation of the `magick` tool; no ImageMagick source
//! code is used.

// Pixel loops index several planes / channels at once; 4-sample chunks
// read better than as_chunks::<4>() here.
#![allow(
    clippy::chunks_exact_to_as_chunks,
    clippy::needless_range_loop,
    clippy::manual_range_contains
)]

pub mod cipher;
pub mod codec;
pub mod color;
mod color_names;
pub mod colorspace;
pub mod composite;
pub mod distort;
pub mod dither;
pub mod draw;
pub mod error;
pub mod filters;
pub mod fonts;
pub mod fourier;
pub mod fx;
pub mod geometry;
pub mod image;
pub mod pixels;
pub mod properties;
pub mod pseudo;
pub mod resize;
pub mod statistics;
pub mod svg;
pub mod text;
pub mod transform;
pub mod types;
pub mod wand;

pub use color::PixelInfo;
pub use error::{MResult, MagickError};
pub use geometry::{Geometry, Rect};
pub use image::Image;
pub use wand::Wand;

/// The filter crate, for callers that run its filters directly.
pub use oxideav_image_filter;

/// QuantumRange of a Q16 build.
pub const QUANTUM_RANGE: f64 = 65535.0;
/// 1 / QuantumRange.
pub const QUANTUM_SCALE: f64 = 1.0 / 65535.0;

/// The ImageMagick release goro-imagick models.
pub const LIB_VERSION: u32 = 0x712;
pub const LIB_VERSION_TEXT: &str = "7.1.2";
pub const LIB_ADDENDUM: &str = "-18";
pub const RELEASE_DATE: &str = "2026-03-22";
pub const VERSION_STRING: &str =
    "ImageMagick 7.1.2-18 Q16 x86_64 goro-imagick https://imagemagick.org";
