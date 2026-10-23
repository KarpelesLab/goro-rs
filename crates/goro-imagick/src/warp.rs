//! ImplodeImage(), SwirlImage() and WaveImage(): the warps that map each
//! destination pixel back to an interpolated source position.
//!
//! Implode and swirl work on the ellipse inscribed in the image (centre
//! (w/2, h/2) in pixel-index coordinates, the shorter axis stretched to a
//! circle of radius max(w, h)/2); pixels outside it are copied.

use crate::geometry::Rect;
use crate::image::Image;
use crate::interp::{interpolate, to_q16};
use crate::types::{PixelInterpolateMethod, VirtualPixelMethod};

struct Ellipse {
    cx: f64,
    cy: f64,
    sx: f64,
    sy: f64,
    radius: f64,
}

fn ellipse(img: &Image) -> Ellipse {
    let (cx, cy) = (img.columns as f64 / 2.0, img.rows as f64 / 2.0);
    let (mut sx, mut sy) = (1.0, 1.0);
    if img.columns > img.rows {
        sy = img.columns as f64 / img.rows as f64;
    } else if img.rows > img.columns {
        sx = img.rows as f64 / img.columns as f64;
    }
    Ellipse {
        cx,
        cy,
        sx,
        sy,
        radius: cx.max(cy),
    }
}

/// Maps each pixel inside the ellipse through `f(dx, dy, distance)`
/// (offsets in the circular frame), returning scaled source offsets.
fn warp_ellipse(
    img: &Image,
    method: PixelInterpolateMethod,
    f: impl Fn(f64, f64, f64) -> (f64, f64),
) -> Image {
    let e = ellipse(img);
    let mut out = img.clone();
    let vp = img.virtual_pixel;
    for y in 0..img.rows {
        let dy = e.sy * (y as f64 - e.cy);
        for x in 0..img.columns {
            let dx = e.sx * (x as f64 - e.cx);
            let d2 = dx * dx + dy * dy;
            if d2 >= e.radius * e.radius {
                continue;
            }
            let (ux, uy) = f(dx, dy, d2.sqrt());
            let p = interpolate(img, method, ux / e.sx + e.cx, uy / e.sy + e.cy, vp);
            out.set_px(x, y, to_q16(p));
        }
    }
    out.colors = None;
    out
}

/// ImplodeImage(): pixels pulled toward (amount > 0) or pushed from the
/// centre by sin(π·r/2R)^-amount.
pub fn implode(img: &Image, amount: f64, method: PixelInterpolateMethod) -> Image {
    let radius = ellipse(img).radius;
    warp_ellipse(img, method, |dx, dy, r| {
        let factor = if r > 0.0 {
            (std::f64::consts::FRAC_PI_2 * r / radius)
                .sin()
                .powf(-amount)
        } else {
            1.0
        };
        (factor * dx, factor * dy)
    })
}

/// SwirlImage(): a rotation by degrees·(1 - r/R)².
pub fn swirl(img: &Image, degrees: f64, method: PixelInterpolateMethod) -> Image {
    let radius = ellipse(img).radius;
    let rad = degrees.to_radians();
    warp_ellipse(img, method, |dx, dy, r| {
        let t = 1.0 - r / radius;
        let (s, c) = (rad * t * t).sin_cos();
        (c * dx - s * dy, s * dx + c * dy)
    })
}

/// WaveImage(): columns shifted by a sine wave; the image grows by twice
/// the amplitude, sampled with background virtual pixels.
pub fn wave(
    img: &Image,
    amplitude: f64,
    wave_length: f64,
    method: PixelInterpolateMethod,
) -> Image {
    let extra = (2.0 * amplitude.abs()) as usize;
    let mut out = img.clone_attributes(img.columns, img.rows + extra);
    let len = if wave_length.abs() < 1e-12 {
        1.0
    } else {
        wave_length
    };
    let bg = img.background_color.to_rgba16();
    if bg[3] != 65535 {
        out.alpha = true;
    }
    for x in 0..out.columns {
        let off = amplitude.abs() + amplitude * (2.0 * std::f64::consts::PI * x as f64 / len).sin();
        for y in 0..out.rows {
            let p = interpolate(
                img,
                method,
                x as f64,
                y as f64 - off,
                VirtualPixelMethod::Background,
            );
            out.set_px(x, y, to_q16(p));
        }
    }
    out.page = Rect::new(out.columns, out.rows, 0, 0);
    out.colors = None;
    out
}
