//! A pure Rust port of the libgd bundled with php-src (ext/gd/libgd).
//!
//! The port follows the C sources closely (same algorithms, same integer
//! and floating point arithmetic) so that images drawn, transformed and
//! encoded here are pixel- and byte-identical to the ones of php-src's
//! bundled libgd. Each module names the C file it ports.
//!
//! Errors and warnings that libgd reports through gd_error() are queued
//! (see [`take_errors`]) for the caller to turn into PHP diagnostics.

// C-port style: indexed loops and explicit counters mirror libgd's code.
#![allow(
    clippy::needless_range_loop,
    clippy::explicit_counter_loop,
    clippy::manual_range_contains
)]

use std::cell::RefCell;
use std::rc::Rc;

pub mod color_match;
pub mod filter;
pub mod fonts;
pub mod gd;
pub mod io;
pub mod png;
pub mod tables;
pub mod topal;

pub use io::{In, Out};

/// gdMaxColors
pub const MAX_COLORS: usize = 256;

pub const ALPHA_MAX: i32 = 127;
pub const ALPHA_OPAQUE: i32 = 0;
pub const ALPHA_TRANSPARENT: i32 = 127;
pub const RED_MAX: i32 = 255;
pub const GREEN_MAX: i32 = 255;
pub const BLUE_MAX: i32 = 255;

pub const EFFECT_REPLACE: i32 = 0;
pub const EFFECT_ALPHA_BLEND: i32 = 1;
pub const EFFECT_NORMAL: i32 = 2;
pub const EFFECT_OVERLAY: i32 = 3;
pub const EFFECT_MULTIPLY: i32 = 4;

/// Special colors
pub const STYLED: i32 = -2;
pub const BRUSHED: i32 = -3;
pub const STYLED_BRUSHED: i32 = -4;
pub const TILED: i32 = -5;
pub const TRANSPARENT: i32 = -6;
pub const ANTI_ALIASED: i32 = -7;

/// gdDashSize
pub const DASH_SIZE: i32 = 4;

/// Arc styles
pub const ARC: i32 = 0;
pub const PIE: i32 = 0;
pub const CHORD: i32 = 1;
pub const NO_FILL: i32 = 2;
pub const EDGED: i32 = 4;

/// GD_RESOLUTION
pub const RESOLUTION: u32 = 96;

pub const GD2_CHUNKSIZE: i32 = 128;
pub const GD2_CHUNKSIZE_MIN: i32 = 64;
pub const GD2_CHUNKSIZE_MAX: i32 = 4096;
pub const GD2_VERS: i32 = 2;
pub const GD2_FMT_RAW: i32 = 1;
pub const GD2_FMT_COMPRESSED: i32 = 2;

pub const GD_CMP_IMAGE: i32 = 1;
pub const GD_CMP_NUM_COLORS: i32 = 2;
pub const GD_CMP_COLOR: i32 = 4;
pub const GD_CMP_SIZE_X: i32 = 8;
pub const GD_CMP_SIZE_Y: i32 = 16;
pub const GD_CMP_TRANSPARENT: i32 = 32;
pub const GD_CMP_BACKGROUND: i32 = 64;
pub const GD_CMP_INTERLACE: i32 = 128;
pub const GD_CMP_TRUECOLOR: i32 = 256;

pub const FLIP_HORIZONTAL: i32 = 1;
pub const FLIP_VERTICAL: i32 = 2;
pub const FLIP_BOTH: i32 = 3;

/// gdInterpolationMethod
pub mod interp {
    pub const DEFAULT: i32 = 0;
    pub const BELL: i32 = 1;
    pub const BESSEL: i32 = 2;
    pub const BILINEAR_FIXED: i32 = 3;
    pub const BICUBIC: i32 = 4;
    pub const BICUBIC_FIXED: i32 = 5;
    pub const BLACKMAN: i32 = 6;
    pub const BOX: i32 = 7;
    pub const BSPLINE: i32 = 8;
    pub const CATMULLROM: i32 = 9;
    pub const GAUSSIAN: i32 = 10;
    pub const GENERALIZED_CUBIC: i32 = 11;
    pub const HERMITE: i32 = 12;
    pub const HAMMING: i32 = 13;
    pub const HANNING: i32 = 14;
    pub const MITCHELL: i32 = 15;
    pub const NEAREST_NEIGHBOUR: i32 = 16;
    pub const POWER: i32 = 17;
    pub const QUADRATIC: i32 = 18;
    pub const SINC: i32 = 19;
    pub const TRIANGLE: i32 = 20;
    pub const WEIGHTED4: i32 = 21;
    pub const LINEAR: i32 = 22;
    pub const METHOD_COUNT: i32 = 22;
}

/// gdCropMode
pub mod crop {
    pub const DEFAULT: i32 = 0;
    pub const TRANSPARENT: i32 = 1;
    pub const BLACK: i32 = 2;
    pub const WHITE: i32 = 3;
    pub const SIDES: i32 = 4;
    pub const THRESHOLD: i32 = 5;
}

/// gd_errors.h priorities (syslog levels)
pub const GD_ERROR: i32 = 3;
pub const GD_WARNING: i32 = 4;
pub const GD_NOTICE: i32 = 5;
pub const GD_INFO: i32 = 6;
pub const GD_DEBUG: i32 = 7;

thread_local! {
    static ERRORS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
}

/// gd_error(): a warning
pub fn gd_error(msg: impl Into<String>) {
    gd_error_ex(GD_WARNING, msg);
}

/// gd_error_ex()
pub fn gd_error_ex(priority: i32, msg: impl Into<String>) {
    let m = msg.into();
    ERRORS.with(|e| e.borrow_mut().push((priority, m)));
}

/// The (priority, message) pairs reported since the last call.
pub fn take_errors() -> Vec<(i32, String)> {
    ERRORS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

/// overflow2() (gd_security.c): true if a * b would overflow an int (or
/// either is not positive).
pub fn overflow2(a: i32, b: i32) -> bool {
    if a <= 0 || b <= 0 {
        gd_error(
            "One parameter to a memory allocation multiplication is negative or zero, failing operation gracefully\n",
        );
        return true;
    }
    if a > i32::MAX / b {
        gd_error(
            "Product of memory allocation multiplication would exceed INT_MAX, failing operation gracefully\n",
        );
        return true;
    }
    false
}

/// overflow2() for sizes computed with a sizeof() factor
pub fn overflow2_size(size: usize, b: i32) -> bool {
    overflow2(size as i32, b)
}

#[inline]
pub fn true_color_get_alpha(c: i32) -> i32 {
    (c & 0x7F000000) >> 24
}
#[inline]
pub fn true_color_get_red(c: i32) -> i32 {
    (c & 0xFF0000) >> 16
}
#[inline]
pub fn true_color_get_green(c: i32) -> i32 {
    (c & 0x00FF00) >> 8
}
#[inline]
pub fn true_color_get_blue(c: i32) -> i32 {
    c & 0x0000FF
}
/// gdTrueColor()
#[inline]
pub fn true_color(r: i32, g: i32, b: i32) -> i32 {
    (r << 16).wrapping_add(g << 8).wrapping_add(b)
}
/// gdTrueColorAlpha()
#[inline]
pub fn true_color_alpha(r: i32, g: i32, b: i32, a: i32) -> i32 {
    (a << 24)
        .wrapping_add(r << 16)
        .wrapping_add(g << 8)
        .wrapping_add(b)
}

/// An interpolation kernel (interpolation_method)
pub type InterpolationFn = fn(f64) -> f64;

/// gdImage
#[derive(Clone)]
pub struct Image {
    /// Palette-based image pixels (rows); empty for truecolor images
    pub pixels: Vec<Vec<u8>>,
    pub sx: i32,
    pub sy: i32,
    pub colors_total: i32,
    pub red: [i32; MAX_COLORS],
    pub green: [i32; MAX_COLORS],
    pub blue: [i32; MAX_COLORS],
    pub open: [i32; MAX_COLORS],
    pub transparent: i32,
    pub poly_ints: Vec<i32>,
    pub brush: Option<Rc<RefCell<Image>>>,
    pub tile: Option<Rc<RefCell<Image>>>,
    pub brush_color_map: [i32; MAX_COLORS],
    pub tile_color_map: [i32; MAX_COLORS],
    pub style: Option<Vec<i32>>,
    pub style_pos: i32,
    pub interlace: i32,
    pub thick: i32,
    pub alpha: [i32; MAX_COLORS],
    pub true_color: bool,
    /// Truecolor pixels (rows); empty for palette images
    pub tpixels: Vec<Vec<i32>>,
    pub alpha_blending_flag: i32,
    pub save_alpha_flag: i32,
    pub aa: i32,
    pub aa_color: i32,
    pub aa_dont_blend: i32,
    pub cx1: i32,
    pub cy1: i32,
    pub cx2: i32,
    pub cy2: i32,
    pub res_x: u32,
    pub res_y: u32,
    pub interpolation_id: i32,
    pub interpolation: Option<InterpolationFn>,
}

/// gdFont
#[derive(Clone, Debug)]
pub struct Font {
    pub nchars: i32,
    pub offset: i32,
    pub w: i32,
    pub h: i32,
    /// One byte per pixel, characters one after another
    pub data: Vec<u8>,
}

/// gdPoint
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// gdPointF
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointF {
    pub x: f64,
    pub y: f64,
}

/// gdRect
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Font {
    fn unpack(nchars: i32, offset: i32, w: i32, h: i32, packed: &[u8]) -> Font {
        let n = (nchars * w * h) as usize;
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            data.push((packed[i >> 3] >> (7 - (i & 7))) & 1);
        }
        Font {
            nchars,
            offset,
            w,
            h,
            data,
        }
    }

    /// The built-in fonts 1..=5 (gdFontTiny, gdFontSmall,
    /// gdFontMediumBold, gdFontLarge, gdFontGiant)
    pub fn builtin(n: i32) -> &'static Font {
        use std::sync::OnceLock;
        static FONTS: OnceLock<[Font; 5]> = OnceLock::new();
        let f = FONTS.get_or_init(|| {
            use fonts::*;
            [
                Font::unpack(TINY_NCHARS, TINY_OFFSET, TINY_W, TINY_H, &TINY_DATA),
                Font::unpack(SMALL_NCHARS, SMALL_OFFSET, SMALL_W, SMALL_H, &SMALL_DATA),
                Font::unpack(
                    MEDIUM_BOLD_NCHARS,
                    MEDIUM_BOLD_OFFSET,
                    MEDIUM_BOLD_W,
                    MEDIUM_BOLD_H,
                    &MEDIUM_BOLD_DATA,
                ),
                Font::unpack(LARGE_NCHARS, LARGE_OFFSET, LARGE_W, LARGE_H, &LARGE_DATA),
                Font::unpack(GIANT_NCHARS, GIANT_OFFSET, GIANT_W, GIANT_H, &GIANT_DATA),
            ]
        });
        &f[(n.clamp(1, 5) - 1) as usize]
    }
}
