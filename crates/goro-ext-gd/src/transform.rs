//! Geometric transformations: imageflip, imagecrop, imagecropauto,
//! imagerotate, imagescale, imageaffine and the affine matrix helpers.
//!
//! Behaviour sources: the PHP manual pages of these functions (rotation
//! is counter-clockwise; imagescale keeps the aspect ratio for a negative
//! size; the IMG_CROP_* modes), libgd's API documentation, and the ext/gd
//! tests: rotated image sizes (bug65148: 40x20 by -5° gives 40x23;
//! gd223: 64x64 by 45° gives 89x90; bug73272: 100x100 by 60° gives
//! 135x136), the threshold of IMG_CROP_THRESHOLD as a percentage of the
//! largest squared RGB distance (bug73291).

use crate::image::*;

/// Interpolation methods (IMG_* constants of imagesetinterpolation).
pub const IMG_BELL: i32 = 1;
pub const IMG_BESSEL: i32 = 2;
pub const IMG_BILINEAR_FIXED: i32 = 3;
pub const IMG_BICUBIC: i32 = 4;
pub const IMG_BICUBIC_FIXED: i32 = 5;
pub const IMG_BLACKMAN: i32 = 6;
pub const IMG_BOX: i32 = 7;
pub const IMG_BSPLINE: i32 = 8;
pub const IMG_CATMULLROM: i32 = 9;
pub const IMG_GAUSSIAN: i32 = 10;
pub const IMG_GENERALIZED_CUBIC: i32 = 11;
pub const IMG_HERMITE: i32 = 12;
pub const IMG_HAMMING: i32 = 13;
pub const IMG_HANNING: i32 = 14;
pub const IMG_MITCHELL: i32 = 15;
pub const IMG_NEAREST_NEIGHBOUR: i32 = 16;
pub const IMG_POWER: i32 = 17;
pub const IMG_QUADRATIC: i32 = 18;
pub const IMG_SINC: i32 = 19;
pub const IMG_TRIANGLE: i32 = 20;
pub const IMG_WEIGHTED4: i32 = 21;

pub fn valid_interpolation(m: i64) -> bool {
    (1..=21).contains(&m)
}

/// Flip modes.
pub const FLIP_HORIZONTAL: i32 = 1;
pub const FLIP_VERTICAL: i32 = 2;
pub const FLIP_BOTH: i32 = 3;

/// Crop modes.
pub const CROP_DEFAULT: i32 = 0;
pub const CROP_TRANSPARENT: i32 = 1;
pub const CROP_BLACK: i32 = 2;
pub const CROP_WHITE: i32 = 3;
pub const CROP_SIDES: i32 = 4;
pub const CROP_THRESHOLD: i32 = 5;

/// Size errors (the warnings PHP prints).
#[derive(Debug, PartialEq)]
pub enum SizeError {
    NegativeOrZero,
    Overflow,
}

pub fn check_size(w: i64, h: i64) -> Result<(), SizeError> {
    if w <= 0 || h <= 0 {
        return Err(SizeError::NegativeOrZero);
    }
    if w > i32::MAX as i64 || h > i32::MAX as i64 || overflow(w as i32, h as i32, 4) {
        return Err(SizeError::Overflow);
    }
    Ok(())
}

impl Image {
    /// imageflip.
    pub fn flip(&mut self, mode: i32) {
        let (w, h) = (self.sx as usize, self.sy as usize);
        let horiz = mode == FLIP_HORIZONTAL || mode == FLIP_BOTH;
        let vert = mode == FLIP_VERTICAL || mode == FLIP_BOTH;
        macro_rules! go {
            ($v:expr) => {{
                let v = $v;
                if horiz {
                    for row in v.chunks_mut(w) {
                        row.reverse();
                    }
                }
                if vert {
                    for y in 0..h / 2 {
                        let (a, b) = v.split_at_mut((h - 1 - y) * w);
                        a[y * w..y * w + w].swap_with_slice(&mut b[..w]);
                    }
                }
            }};
        }
        if self.truecolor {
            go!(&mut self.tpixels)
        } else {
            go!(&mut self.pixels)
        }
    }

    /// An empty image of the same kind and palette.
    pub fn blank_like(&self, w: i32, h: i32) -> Option<Image> {
        let mut d = self.new_like(w, h)?;
        if !self.truecolor {
            d.red = self.red;
            d.green = self.green;
            d.blue = self.blue;
            d.alpha = self.alpha;
            d.open = self.open;
            d.colors_total = self.colors_total;
        }
        d.transparent = self.transparent;
        d.save_alpha = self.save_alpha;
        d.interpolation = self.interpolation;
        Some(d)
    }

    /// imagecrop: the part of the image in the rectangle (size errors
    /// before anything).
    pub fn crop(&self, x: i32, y: i32, w: i32, h: i32) -> Result<Image, SizeError> {
        check_size(w as i64, h as i64)?;
        let mut d = self.blank_like(w, h).ok_or(SizeError::Overflow)?;
        d.alpha_blending = self.alpha_blending;
        // The part inside the source.
        let x0 = x.max(0) as i64;
        let y0 = y.max(0) as i64;
        let x1 = (x as i64 + w as i64).min(self.sx as i64);
        let y1 = (y as i64 + h as i64).min(self.sy as i64);
        for sy in y0..y1 {
            for sx in x0..x1 {
                let p = self.get_pixel(sx as i32, sy as i32);
                d.put_raw((sx - x as i64) as i32, (sy - y as i64) as i32, p);
            }
        }
        Ok(d)
    }

    /// imagecropauto. `None` when nothing would remain.
    pub fn crop_auto(&self, mode: i32, threshold: f64, color: i32) -> Option<Image> {
        let (w, h) = (self.sx, self.sy);
        let is_bg: Box<dyn Fn(i32) -> bool + '_> = match mode {
            CROP_TRANSPARENT | CROP_DEFAULT => {
                if self.truecolor {
                    Box::new(|p| tc_a(p) == ALPHA_TRANSPARENT)
                } else {
                    let t = self.transparent;
                    Box::new(move |p| t >= 0 && p == t)
                }
            }
            // Pixels of the image's color closest to black / white
            // (imagecrop_auto: an unallocated palette index is not black).
            CROP_BLACK => {
                let c = self.color_closest(0, 0, 0, 0);
                Box::new(move |p| p == c)
            }
            CROP_WHITE => {
                let c = self.color_closest(255, 255, 255, 0);
                Box::new(move |p| p == c)
            }
            CROP_SIDES => {
                let bg = self.guess_background()?;
                Box::new(move |p| p == bg)
            }
            CROP_THRESHOLD => {
                let want = if self.truecolor {
                    color
                } else {
                    self.palette_to_tc(color)
                };
                Box::new(move |p| {
                    let c = self.get_truecolor_pixel_of(p);
                    let dr = (tc_r(c) - tc_r(want)) as f64;
                    let dg = (tc_g(c) - tc_g(want)) as f64;
                    let db = (tc_b(c) - tc_b(want)) as f64;
                    let da = (tc_a(c) - tc_a(want)) as f64;
                    let dist = dr * dr + dg * dg + db * db + da * da;
                    dist / (3.0 * 255.0 * 255.0) * 100.0 <= threshold
                })
            }
            _ => return None,
        };
        let row_bg = |y: i32| (0..w).all(|x| is_bg(self.get_pixel(x, y)));
        let col_bg = |x: i32, y0: i32, y1: i32| (y0..=y1).all(|y| is_bg(self.get_pixel(x, y)));
        let mut top = 0;
        while top < h && row_bg(top) {
            top += 1;
        }
        if top == h {
            return None;
        }
        let mut bottom = h - 1;
        while bottom > top && row_bg(bottom) {
            bottom -= 1;
        }
        let mut left = 0;
        while left < w && col_bg(left, top, bottom) {
            left += 1;
        }
        let mut right = w - 1;
        while right > left && col_bg(right, top, bottom) {
            right -= 1;
        }
        self.crop(left, top, right - left + 1, bottom - top + 1).ok()
    }

    fn get_truecolor_pixel_of(&self, p: i32) -> i32 {
        if self.truecolor {
            p
        } else {
            self.palette_to_tc(p)
        }
    }

    /// The background color guessed from the corners: one shared by at
    /// least two corners (bug77200), else their average.
    fn guess_background(&self) -> Option<i32> {
        let (w, h) = (self.sx, self.sy);
        let c = [
            self.get_pixel(0, 0),
            self.get_pixel(w - 1, 0),
            self.get_pixel(0, h - 1),
            self.get_pixel(w - 1, h - 1),
        ];
        let count = |v: i32| c.iter().filter(|&&x| x == v).count();
        let mut best = None;
        let mut best_n = 1;
        for &v in &c {
            let n = count(v);
            if n > best_n {
                best_n = n;
                best = Some(v);
            }
        }
        if best_n == 2 {
            // Two pairs: ambiguous.
            let distinct: Vec<i32> = {
                let mut d = c.to_vec();
                d.sort_unstable();
                d.dedup();
                d
            };
            if distinct.len() == 2 {
                return best;
            }
        }
        if let Some(b) = best {
            return Some(b);
        }
        // Average of the four corners.
        let (mut r, mut g, mut b, mut a) = (0, 0, 0, 0);
        for &p in &c {
            let t = self.get_truecolor_pixel_of(p);
            r += tc_r(t);
            g += tc_g(t);
            b += tc_b(t);
            a += tc_a(t);
        }
        let avg = tc(r / 4, g / 4, b / 4, a / 4);
        if self.truecolor {
            Some(avg)
        } else {
            Some(self.color_closest(tc_r(avg), tc_g(avg), tc_b(avg), tc_a(avg)))
        }
    }

    // ------------------------------------------------------------ rotation

    /// A copy rotated by a multiple of 90° counter-clockwise.
    pub fn rotate90(&self, quarter: i32) -> Option<Image> {
        let (w, h) = (self.sx, self.sy);
        let q = quarter.rem_euclid(4);
        let (nw, nh) = if q % 2 == 1 { (h, w) } else { (w, h) };
        let mut d = self.blank_like(nw, nh)?;
        d.alpha_blending = self.alpha_blending;
        d.save_alpha = self.save_alpha;
        for y in 0..h {
            for x in 0..w {
                let p = self.get_pixel(x, y);
                let (nx, ny) = match q {
                    0 => (x, y),
                    1 => (y, w - 1 - x),
                    2 => (w - 1 - x, h - 1 - y),
                    _ => (h - 1 - y, x),
                };
                d.put_raw(nx, ny, p);
            }
        }
        Some(d)
    }

    /// imagerotate for any angle: a truecolor image of the rotated
    /// bounding box, uncovered pixels in `bg`.
    pub fn rotate(&self, angle: f64, bg: i32) -> Option<Image> {
        let a = angle.rem_euclid(360.0);
        if a == 0.0 || a == 90.0 || a == 180.0 || a == 270.0 {
            let q = (a / 90.0) as i32;
            let mut r = self.rotate90(q)?;
            if !self.truecolor && q == 0 {
                r.transparent = self.transparent;
            }
            return Some(r);
        }
        let src = if self.truecolor {
            std::borrow::Cow::Borrowed(self)
        } else {
            let mut s = self.clone();
            crate::copy::palette_to_truecolor(&mut s);
            std::borrow::Cow::Owned(s)
        };
        let bg = if self.truecolor {
            bg
        } else if (0..MAX_COLORS as i32).contains(&bg) && bg < self.colors_total {
            self.palette_to_tc(bg)
        } else {
            bg
        };
        let rad = a.to_radians();
        let (s, c) = rad.sin_cos();
        let (w, h) = (src.sx as f64, src.sy as f64);
        let xr = (c * (w - 1.0)).abs() + (s * (h - 1.0)).abs();
        let yr = (s * (w - 1.0)).abs() + (c * (h - 1.0)).abs();
        let nw = (xr + 1e-9).floor().max(1.0) as i32;
        let nh = (yr - 1e-9).ceil().max(1.0) as i32;
        let mut d = Image::new_truecolor(nw, nh)?;
        d.save_alpha = true;
        d.alpha_blending = EFFECT_REPLACE;
        let (scx, scy) = ((w - 1.0) / 2.0, (h - 1.0) / 2.0);
        let (dcx, dcy) = ((nw - 1) as f64 / 2.0, (nh - 1) as f64 / 2.0);
        let method = src.interpolation;
        for y in 0..nh {
            for x in 0..nw {
                let dx = x as f64 - dcx;
                let dy = y as f64 - dcy;
                let sxf = c * dx - s * dy + scx;
                let syf = s * dx + c * dy + scy;
                let p = sample(&src, sxf, syf, method, bg);
                d.tpixels[(y * nw + x) as usize] = p;
            }
        }
        d.alpha_blending = EFFECT_ALPHABLEND;
        Some(d)
    }

    // ------------------------------------------------------------ scaling

    /// imagescale: a truecolor image of the new size.
    pub fn scale(&self, nw: i32, nh: i32, method: i32) -> Result<Image, SizeError> {
        check_size(nw as i64, nh as i64)?;
        if overflow(self.sx, self.sy, 4) {
            return Err(SizeError::Overflow);
        }
        if !matches!(
            method,
            IMG_NEAREST_NEIGHBOUR | IMG_BILINEAR_FIXED | IMG_BICUBIC_FIXED | IMG_BICUBIC
        ) {
            // The filter weights of each output pixel span the source
            // pixels under the kernel (bug77272: 2^28 pixels down to 1).
            let (support, _) = kernel(method);
            for (n_in, n_out) in [(self.sx, nw), (self.sy, nh)] {
                let scale = n_out as f64 / n_in as f64;
                let width = if scale < 1.0 { support / scale } else { support };
                let window = 2.0 * width.ceil() + 1.0;
                if window * n_out as f64 * 8.0 > i32::MAX as f64 {
                    return Err(SizeError::Overflow);
                }
            }
        }
        let src = if self.truecolor {
            std::borrow::Cow::Borrowed(self)
        } else {
            let mut s = self.clone();
            crate::copy::palette_to_truecolor(&mut s);
            std::borrow::Cow::Owned(s)
        };
        let mut d = Image::new_truecolor(nw, nh).ok_or(SizeError::Overflow)?;
        d.save_alpha = self.save_alpha || !self.truecolor;
        d.interpolation = self.interpolation;
        let (sw, sh) = (src.sx, src.sy);
        match method {
            IMG_NEAREST_NEIGHBOUR => {
                for y in 0..nh {
                    let sy = ((y as i64 * sh as i64) / nh as i64) as i32;
                    for x in 0..nw {
                        let sx = ((x as i64 * sw as i64) / nw as i64) as i32;
                        d.tpixels[(y * nw + x) as usize] = src.get_pixel(sx, sy);
                    }
                }
            }
            IMG_BILINEAR_FIXED | IMG_BICUBIC_FIXED | IMG_BICUBIC => {
                let fx = sw as f64 / nw as f64;
                let fy = sh as f64 / nh as f64;
                for y in 0..nh {
                    let syf = ((y as f64 + 0.5) * fy - 0.5).max(0.0);
                    for x in 0..nw {
                        let sxf = ((x as f64 + 0.5) * fx - 0.5).max(0.0);
                        let p = if method == IMG_BILINEAR_FIXED {
                            bilinear(&src, sxf, syf, None)
                        } else {
                            bicubic(&src, sxf, syf, None)
                        };
                        d.tpixels[(y * nw + x) as usize] = p;
                    }
                }
            }
            _ => {
                resample_generic(&src, &mut d, method);
            }
        }
        Ok(d)
    }
}

/// Clamped source pixel.
#[inline]
fn px(src: &Image, x: i32, y: i32) -> i32 {
    src.tpixels[(y.clamp(0, src.sy - 1) * src.sx + x.clamp(0, src.sx - 1)) as usize]
}

/// Samples at (x, y); outside the image: `bg`.
fn sample(src: &Image, x: f64, y: f64, method: i32, bg: i32) -> i32 {
    let (w, h) = (src.sx as f64, src.sy as f64);
    if x < -0.5 || y < -0.5 || x > w - 0.5 || y > h - 0.5 {
        return bg;
    }
    match method {
        IMG_NEAREST_NEIGHBOUR => {
            let xi = (x + 0.5).floor() as i32;
            let yi = (y + 0.5).floor() as i32;
            if xi < 0 || yi < 0 || xi >= src.sx || yi >= src.sy {
                bg
            } else {
                px(src, xi, yi)
            }
        }
        IMG_BICUBIC | IMG_BICUBIC_FIXED => bicubic(src, x, y, Some(bg)),
        _ => bilinear(src, x, y, Some(bg)),
    }
}

fn pix_or(src: &Image, x: i32, y: i32, bg: Option<i32>) -> i32 {
    if x < 0 || y < 0 || x >= src.sx || y >= src.sy {
        match bg {
            Some(b) => b,
            None => px(src, x, y),
        }
    } else {
        src.tpixels[(y * src.sx + x) as usize]
    }
}

fn bilinear(src: &Image, x: f64, y: f64, bg: Option<i32>) -> i32 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    let p = [
        (pix_or(src, x0, y0, bg), (1.0 - fx) * (1.0 - fy)),
        (pix_or(src, x0 + 1, y0, bg), fx * (1.0 - fy)),
        (pix_or(src, x0, y0 + 1, bg), (1.0 - fx) * fy),
        (pix_or(src, x0 + 1, y0 + 1, bg), fx * fy),
    ];
    blend_weighted(&p)
}

fn cubic(t: f64) -> f64 {
    // Keys cubic convolution, a = -0.5.
    let a = -0.5;
    let t = t.abs();
    if t <= 1.0 {
        (a + 2.0) * t * t * t - (a + 3.0) * t * t + 1.0
    } else if t < 2.0 {
        a * t * t * t - 5.0 * a * t * t + 8.0 * a * t - 4.0 * a
    } else {
        0.0
    }
}

fn bicubic(src: &Image, x: f64, y: f64, bg: Option<i32>) -> i32 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    let mut v = Vec::with_capacity(16);
    for j in -1..=2 {
        let wy = cubic(j as f64 - fy);
        for i in -1..=2 {
            let wx = cubic(i as f64 - fx);
            v.push((pix_or(src, x0 + i, y0 + j, bg), wx * wy));
        }
    }
    blend_weighted(&v)
}

fn blend_weighted(p: &[(i32, f64)]) -> i32 {
    let (mut r, mut g, mut b, mut a, mut ws) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(c, w) in p {
        r += tc_r(c) as f64 * w;
        g += tc_g(c) as f64 * w;
        b += tc_b(c) as f64 * w;
        a += tc_a(c) as f64 * w;
        ws += w;
    }
    if ws != 0.0 && (ws - 1.0).abs() > 1e-9 {
        r /= ws;
        g /= ws;
        b /= ws;
        a /= ws;
    }
    let cl = |v: f64, m: f64| (v + 0.5).clamp(0.0, m) as i32;
    tc(cl(r, 255.0), cl(g, 255.0), cl(b, 255.0), cl(a, 127.0))
}

/// Filter kernels of the generic two-pass resampler: (support, kernel).
fn kernel(method: i32) -> (f64, fn(f64) -> f64) {
    fn bell(t: f64) -> f64 {
        let t = t.abs();
        if t < 0.5 {
            0.75 - t * t
        } else if t < 1.5 {
            0.5 * (t - 1.5) * (t - 1.5)
        } else {
            0.0
        }
    }
    fn box_(t: f64) -> f64 {
        if t > -0.5 && t <= 0.5 { 1.0 } else { 0.0 }
    }
    fn triangle(t: f64) -> f64 {
        let t = t.abs();
        if t < 1.0 { 1.0 - t } else { 0.0 }
    }
    fn hermite(t: f64) -> f64 {
        let t = t.abs();
        if t < 1.0 {
            (2.0 * t - 3.0) * t * t + 1.0
        } else {
            0.0
        }
    }
    fn bspline(t: f64) -> f64 {
        let t = t.abs();
        if t < 1.0 {
            0.5 * t * t * t - t * t + 2.0 / 3.0
        } else if t < 2.0 {
            let u = 2.0 - t;
            u * u * u / 6.0
        } else {
            0.0
        }
    }
    fn sinc(t: f64) -> f64 {
        if t == 0.0 {
            1.0
        } else {
            let x = std::f64::consts::PI * t;
            x.sin() / x
        }
    }
    fn mitchell(t: f64) -> f64 {
        let (b, c) = (1.0 / 3.0, 1.0 / 3.0);
        let t = t.abs();
        if t < 1.0 {
            ((12.0 - 9.0 * b - 6.0 * c) * t * t * t
                + (-18.0 + 12.0 * b + 6.0 * c) * t * t
                + (6.0 - 2.0 * b))
                / 6.0
        } else if t < 2.0 {
            ((-b - 6.0 * c) * t * t * t
                + (6.0 * b + 30.0 * c) * t * t
                + (-12.0 * b - 48.0 * c) * t
                + (8.0 * b + 24.0 * c))
                / 6.0
        } else {
            0.0
        }
    }
    fn gaussian(t: f64) -> f64 {
        (-2.0 * t * t).exp() * (2.0 / std::f64::consts::PI).sqrt()
    }
    fn hanning(t: f64) -> f64 {
        0.5 + 0.5 * (std::f64::consts::PI * t).cos()
    }
    fn hamming(t: f64) -> f64 {
        0.54 + 0.46 * (std::f64::consts::PI * t).cos()
    }
    fn blackman(t: f64) -> f64 {
        0.42 + 0.5 * (std::f64::consts::PI * t).cos() + 0.08 * (2.0 * std::f64::consts::PI * t).cos()
    }
    fn quadratic(t: f64) -> f64 {
        let t = t.abs();
        if t < 0.5 {
            -2.0 * t * t + 1.0
        } else if t < 1.5 {
            t * t - 2.5 * t + 1.5
        } else {
            0.0
        }
    }
    fn catmull(t: f64) -> f64 {
        cubic(t)
    }
    match method {
        IMG_BELL => (1.5, bell),
        IMG_BOX => (0.5, box_),
        IMG_HERMITE => (1.0, hermite),
        IMG_BSPLINE => (2.0, bspline),
        IMG_SINC | IMG_BESSEL => (1.0, sinc),
        IMG_MITCHELL => (2.0, mitchell),
        IMG_GAUSSIAN => (1.25, gaussian),
        IMG_HANNING => (1.0, hanning),
        IMG_HAMMING => (1.0, hamming),
        IMG_BLACKMAN => (1.0, blackman),
        IMG_QUADRATIC => (1.5, quadratic),
        IMG_CATMULLROM | IMG_GENERALIZED_CUBIC | IMG_POWER | IMG_WEIGHTED4 => (2.0, catmull),
        _ => (1.0, triangle),
    }
}

fn resample_generic(src: &Image, dst: &mut Image, method: i32) {
    let (support, k) = kernel(method);
    let (sw, sh) = (src.sx, src.sy);
    let (nw, nh) = (dst.sx, dst.sy);
    // Horizontal pass into a temporary of nw x sh (f64 RGBA).
    let contribs = |n_in: i32, n_out: i32| -> Vec<Vec<(i32, f64)>> {
        let scale = n_out as f64 / n_in as f64;
        let (fscale, width) = if scale < 1.0 {
            (scale, support / scale)
        } else {
            (1.0, support)
        };
        (0..n_out)
            .map(|i| {
                let center = (i as f64 + 0.5) / scale - 0.5;
                let left = (center - width).floor() as i32;
                let right = (center + width).ceil() as i32;
                let mut v = Vec::new();
                let mut total = 0.0;
                for j in left..=right {
                    let w = k((center - j as f64) * fscale);
                    if w != 0.0 {
                        v.push((j.clamp(0, n_in - 1), w));
                        total += w;
                    }
                }
                if total != 0.0 {
                    for e in v.iter_mut() {
                        e.1 /= total;
                    }
                }
                if v.is_empty() {
                    v.push(((center.round() as i32).clamp(0, n_in - 1), 1.0));
                }
                v
            })
            .collect()
    };
    let cx = contribs(sw, nw);
    let cy = contribs(sh, nh);
    let mut tmp = vec![[0.0f64; 4]; (nw as usize) * (sh as usize)];
    for y in 0..sh {
        for (x, c) in cx.iter().enumerate() {
            let mut acc = [0.0; 4];
            for &(sx, w) in c {
                let p = src.tpixels[(y * sw + sx) as usize];
                acc[0] += tc_r(p) as f64 * w;
                acc[1] += tc_g(p) as f64 * w;
                acc[2] += tc_b(p) as f64 * w;
                acc[3] += tc_a(p) as f64 * w;
            }
            tmp[y as usize * nw as usize + x] = acc;
        }
    }
    for (y, c) in cy.iter().enumerate() {
        for x in 0..nw as usize {
            let mut acc = [0.0; 4];
            for &(sy, w) in c {
                let p = tmp[sy as usize * nw as usize + x];
                for i in 0..4 {
                    acc[i] += p[i] * w;
                }
            }
            let cl = |v: f64, m: f64| (v + 0.5).clamp(0.0, m) as i32;
            dst.tpixels[y * nw as usize + x] = tc(
                cl(acc[0], 255.0),
                cl(acc[1], 255.0),
                cl(acc[2], 255.0),
                cl(acc[3], 127.0),
            );
        }
    }
}

// ------------------------------------------------------------ affine

/// An affine matrix [a, b, c, d, tx, ty]: x' = a x + c y + tx,
/// y' = b x + d y + ty (PHP manual: imageaffine).
pub type Affine = [f64; 6];

pub fn affine_concat(m1: &Affine, m2: &Affine) -> Affine {
    [
        m1[0] * m2[0] + m1[1] * m2[2],
        m1[0] * m2[1] + m1[1] * m2[3],
        m1[2] * m2[0] + m1[3] * m2[2],
        m1[2] * m2[1] + m1[3] * m2[3],
        m1[4] * m2[0] + m1[5] * m2[2] + m2[4],
        m1[4] * m2[1] + m1[5] * m2[3] + m2[5],
    ]
}

fn affine_invert(m: &Affine) -> Option<Affine> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < f64::EPSILON {
        return None;
    }
    let r = 1.0 / det;
    Some([
        m[3] * r,
        -m[1] * r,
        -m[2] * r,
        m[0] * r,
        -m[4] * m[3] * r + m[5] * m[2] * r,
        -m[5] * m[0] * r + m[4] * m[1] * r,
    ])
}

fn affine_apply(m: &Affine, x: f64, y: f64) -> (f64, f64) {
    (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
}

/// imageaffine: the transformed `clip` rectangle of the source (default:
/// the whole image) as a new truecolor image. `None` when the matrix
/// can't be inverted.
pub fn affine(src: &Image, m: &Affine, clip: Option<(i32, i32, i32, i32)>) -> Option<Image> {
    let inv = affine_invert(m)?;
    let (cx, cy, cw, ch) = clip.unwrap_or((0, 0, src.sx, src.sy));
    let corners = [
        affine_apply(m, cx as f64, cy as f64),
        affine_apply(m, (cx + cw) as f64, cy as f64),
        affine_apply(m, cx as f64, (cy + ch) as f64),
        affine_apply(m, (cx + cw) as f64, (cy + ch) as f64),
    ];
    let minx = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
    let maxx = corners.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
    let miny = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
    let maxy = corners.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
    let ox = minx.floor();
    let oy = miny.floor();
    let w = (maxx.ceil() - ox) as i64;
    let h = (maxy.ceil() - oy) as i64;
    if w <= 0 || h <= 0 || w > 1 << 16 || h > 1 << 16 {
        return None;
    }
    let tsrc = if src.truecolor {
        std::borrow::Cow::Borrowed(src)
    } else {
        let mut s = src.clone();
        crate::copy::palette_to_truecolor(&mut s);
        std::borrow::Cow::Owned(s)
    };
    let mut d = Image::new_truecolor(w as i32, h as i32)?;
    d.save_alpha = true;
    d.interpolation = src.interpolation;
    let transparent = tc(0, 0, 0, ALPHA_TRANSPARENT);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (sx, sy) = affine_apply(&inv, x as f64 + ox, y as f64 + oy);
            let inside = sx >= cx as f64
                && sy >= cy as f64
                && sx < (cx + cw) as f64
                && sy < (cy + ch) as f64;
            let p = if inside {
                sample(&tsrc, sx, sy, tsrc.interpolation, transparent)
            } else {
                transparent
            };
            d.tpixels[(y * w as i32 + x) as usize] = p;
        }
    }
    Some(d)
}
