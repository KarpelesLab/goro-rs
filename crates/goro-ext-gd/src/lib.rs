//! PHP's gd extension on plain images (PHP manual: GD and Image
//! Functions): the GD image model (palette and truecolor images with GD's
//! 7-bit alpha), color allocation, drawing primitives, copying and
//! resampling, transformations, filters, bitmap and TrueType text, and the
//! image formats (PNG, JPEG, GIF, WebP, BMP, WBMP, XBM, XPM, TGA, AVIF,
//! GD and GD2).
//!
//! Behaviour sources: the PHP manual, libgd's published API
//! documentation (semantics only), the format specifications cited in
//! each module, and the expected output of php-src's ext/gd tests (which
//! compare pixels with reference PNGs).
//!
//! The engine module (`builtins/gd.rs`) wires these to PHP values: it
//! keeps a [`image::Shared`] image in each `GdImage` object, checks
//! arguments with PHP's messages, and does the file and output I/O.

pub mod copy;
pub mod draw;
pub mod filter;
pub mod font;
mod font_data;
pub mod formats;
pub mod image;
pub mod quant;
pub mod transform;
pub mod ttf;

pub use image::{Image, Shared};
