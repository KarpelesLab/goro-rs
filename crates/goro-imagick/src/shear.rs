//! ShearImage(): a row shear then a column shear, each shifting whole
//! rows (columns) by a fractional amount with linear blending against the
//! background, then a crop to the sheared parallelogram.
//!
//! Black-box geometry (ImageMagick 7.1.2), pixel-index coordinates:
//! - tx = tan(x°), ty = tan(y°); ox = round(|tx|·h/2); W = w + round(|tx|·h);
//! - rows: x' = x + tx·(h/2 − y) + ox;
//! - columns: y' = y + ty·(x' − cX) − crop, cX = W/2 − max(W − w − 2·ox, 0);
//! - crop and height from the parallelogram of the image's edges:
//!   crop = round(min y'), H = round(max y' − min y').

use crate::color::to_q16;
use crate::geometry::Rect;
use crate::image::Image;
use crate::{MResult, PixelInfo};

/// Linear blend of two RGBA samples, colour alpha-weighted.
fn blend(p0: [u16; 4], p1: [u16; 4], f: f64) -> [u16; 4] {
    let (w0, w1) = (1.0 - f, f);
    let a0 = p0[3] as f64 / 65535.0;
    let a1 = p1[3] as f64 / 65535.0;
    let wa = w0 * a0 + w1 * a1;
    let mut o = [0u16; 4];
    for c in 0..3 {
        o[c] = if wa > 0.0 {
            to_q16((w0 * a0 * p0[c] as f64 + w1 * a1 * p1[c] as f64) / wa)
        } else {
            0
        };
    }
    o[3] = to_q16(w0 * p0[3] as f64 + w1 * p1[3] as f64);
    o
}

/// `p` composited over `b`.
fn over(p: [u16; 4], b: [u16; 4]) -> [u16; 4] {
    let sa = p[3] as f64 / 65535.0;
    let ba = b[3] as f64 / 65535.0;
    let ao = sa + ba * (1.0 - sa);
    let mut o = [0u16; 4];
    for c in 0..3 {
        o[c] = if ao > 0.0 {
            to_q16((p[c] as f64 * sa + b[c] as f64 * ba * (1.0 - sa)) / ao)
        } else {
            0
        };
    }
    o[3] = to_q16(ao * 65535.0);
    o
}

/// One line resampled at `pos - shift`, the background outside.
fn shift_line(line: &[[u16; 4]], out: &mut [[u16; 4]], shift: f64, bg: [u16; 4]) {
    let n = line.len() as i64;
    let at = |i: i64| {
        if i >= 0 && i < n {
            line[i as usize]
        } else {
            bg
        }
    };
    for (k, o) in out.iter_mut().enumerate() {
        let s = k as f64 - shift;
        let i = s.floor();
        let f = s - i;
        let i = i as i64;
        *o = if f == 0.0 {
            at(i)
        } else {
            blend(at(i), at(i + 1), f)
        };
    }
}

pub fn shear(img: &Image, x_deg: f64, y_deg: f64, background: &PixelInfo) -> MResult<Image> {
    let tx = x_deg.to_radians().tan();
    let ty = y_deg.to_radians().tan();
    let (w, h) = (img.columns, img.rows);
    let (wf, hf) = (w as f64, h as f64);
    let ox = (tx.abs() * hf / 2.0).round();
    let nw = w + (tx.abs() * hf).round() as usize;
    let d = nw as f64 - wf - 2.0 * ox;
    // the column shear centres on W/2, one column left when the rounded
    // growth is odd
    let cx = nw as f64 / 2.0 - d.max(0.0);
    // the parallelogram of the image's edges after both shears
    let mut ys = [0f64; 4];
    for (k, (xc, yc)) in [(0.0, 0.0), (wf, 0.0), (0.0, hf), (wf, hf)]
        .into_iter()
        .enumerate()
    {
        let x1 = xc + tx * (hf / 2.0 - yc + 0.5) + ox;
        ys[k] = yc + ty * (x1 - 0.5 - cx);
    }
    let ymin = ys.iter().copied().fold(f64::MAX, f64::min);
    let ymax = ys.iter().copied().fold(f64::MIN, f64::max);
    let crop = ymin.round();
    let nh = (ymax - ymin).round().max(1.0) as usize;

    let mut src = img.clone();
    src.cmyk_to_rgb();
    let bg = background.to_rgba16();
    // rows; an image with alpha shows over the background
    let mut mid = vec![[0u16; 4]; nw * h];
    let mut line = vec![[0u16; 4]; w];
    for y in 0..h {
        for x in 0..w {
            let p = src.px(x, y);
            line[x] = if src.alpha { over(p, bg) } else { p };
        }
        let shift = tx * (hf / 2.0 - y as f64) + ox;
        shift_line(&line, &mut mid[y * nw..(y + 1) * nw], shift, bg);
    }
    // columns
    let mut out = img.clone_attributes(nw, nh);
    out.colorspace = src.colorspace;
    out.black = None;
    let mut col = vec![[0u16; 4]; h];
    let mut res = vec![[0u16; 4]; nh];
    for x in 0..nw {
        for y in 0..h {
            col[y] = mid[y * nw + x];
        }
        // the column shear spans w + 2·ox columns; an odd extra one
        // keeps its rows
        let shift = if (x as f64) < wf + 2.0 * ox {
            ty * (x as f64 - cx) - crop
        } else {
            -crop
        };
        shift_line(&col, &mut res, shift, bg);
        for (y, p) in res.iter().enumerate() {
            out.set_px(x, y, *p);
        }
    }
    if src.alpha {
        out.alpha = true;
    } else {
        // blended against the background with its alpha, which then goes
        // with the channel
        out.alpha = false;
        for p in out.pixels.chunks_exact_mut(4) {
            p[3] = 65535;
        }
    }
    out.page = Rect::new(nw, nh, 0, 0);
    out.colors = None;
    Ok(out)
}

/// RotateImage(): quarter turns are exact; any other angle renders as an
/// SRT distortion (EWA) about the image's centre, with background virtual
/// pixels, into a window of
/// W = w' + 2·round((w'|cos θ| + h'|sin θ| − w')/2 + 1) (likewise H), where
/// w'×h' is the image after the nearest quarter turn and θ what is left;
/// the page moves by half the growth over the original size.
pub fn rotate(img: &Image, degrees: f64, background: &PixelInfo) -> Image {
    let mut deg = degrees % 360.0;
    if deg < 0.0 {
        deg += 360.0;
    }
    let quarters = (deg / 90.0).round();
    let theta = deg - 90.0 * quarters;
    if theta.abs() < 1e-12 {
        let mut r = crate::transform::rotate90(img, quarters as i32);
        r.page = Rect::new(r.columns, r.rows, img.page.x, img.page.y);
        return r;
    }
    let (w, h) = if (quarters as i64).rem_euclid(2) == 1 {
        (img.rows as f64, img.columns as f64)
    } else {
        (img.columns as f64, img.rows as f64)
    };
    let th = theta.to_radians();
    let grow = |a: f64, b: f64| -> f64 {
        let bound = a * th.cos().abs() + b * th.sin().abs();
        2.0 * ((bound - a) / 2.0 + 1.5).floor().max(0.0)
    };
    let nw = (w + grow(w, h)) as i64;
    let nh = (h + grow(h, w)) as i64;
    let ox = (nw - img.columns as i64).div_euclid(2);
    let oy = (nh - img.rows as i64).div_euclid(2);
    let mut src = img.clone();
    src.background_color = background.clone();
    src.virtual_pixel = crate::types::VirtualPixelMethod::Background;
    src.artifacts.set(
        "distort:viewport",
        &format!("{}x{}{:+}{:+}", nw, nh, -ox, -oy),
    );
    let mut out = match crate::distort::distort(&src, crate::distort::SRT, &[deg], false) {
        Ok(o) => o,
        Err(_) => return img.clone(),
    };
    out.virtual_pixel = img.virtual_pixel;
    out.background_color = img.background_color.clone();
    out.artifacts = img.artifacts.clone();
    if !img.alpha && !(background.alpha_trait && background.alpha < 65535.0) {
        out.alpha = false;
        for p in out.pixels.chunks_exact_mut(4) {
            p[3] = 65535;
        }
    }
    out.page = Rect::new(nw as usize, nh as usize, img.page.x - ox, img.page.y - oy);
    out.colors = None;
    out
}
