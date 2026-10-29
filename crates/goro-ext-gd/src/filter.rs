//! imagefilter, imageconvolution and imagegammacorrect.
//!
//! Behaviour sources: the PHP manual pages of imagefilter (the filter
//! list and their arguments), imageconvolution and imagegammacorrect;
//! the ext/gd tests' reference images (imageconvolution_basic.png,
//! imagegammacorrect_*.png).

use crate::image::*;

#[inline]
fn clamp255(v: f64) -> i32 {
    v.clamp(0.0, 255.0) as i32
}

impl Image {
    /// Applies `f` to every pixel color (palette entries for palette
    /// images).
    fn map_colors(&mut self, f: impl Fn(i32, i32, i32, i32) -> (i32, i32, i32, i32)) {
        if self.truecolor {
            for p in self.tpixels.iter_mut() {
                let c = *p;
                let (r, g, b, a) = f(tc_r(c), tc_g(c), tc_b(c), tc_a(c));
                *p = tc(r, g, b, a);
            }
        } else {
            for i in 0..self.colors_total as usize {
                let (r, g, b, a) = f(self.red[i], self.green[i], self.blue[i], self.alpha[i]);
                self.red[i] = r;
                self.green[i] = g;
                self.blue[i] = b;
                self.alpha[i] = a;
            }
        }
    }

    /// IMG_FILTER_NEGATE.
    pub fn filter_negate(&mut self) {
        self.map_colors(|r, g, b, a| (255 - r, 255 - g, 255 - b, a));
    }

    /// IMG_FILTER_GRAYSCALE.
    pub fn filter_grayscale(&mut self) {
        self.map_colors(|r, g, b, a| {
            let v = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) as i32;
            (v, v, v, a)
        });
    }

    /// IMG_FILTER_BRIGHTNESS.
    pub fn filter_brightness(&mut self, level: i32) {
        self.map_colors(|r, g, b, a| {
            (
                (r + level).clamp(0, 255),
                (g + level).clamp(0, 255),
                (b + level).clamp(0, 255),
                a,
            )
        });
    }

    /// IMG_FILTER_CONTRAST.
    pub fn filter_contrast(&mut self, contrast: f64) {
        let c = (100.0 - contrast) / 100.0;
        let c = c * c;
        self.map_colors(|r, g, b, a| {
            let f = |v: i32| {
                let x = ((v as f64 / 255.0 - 0.5) * c + 0.5) * 255.0;
                clamp255(x)
            };
            (f(r), f(g), f(b), a)
        });
    }

    /// IMG_FILTER_COLORIZE.
    pub fn filter_colorize(&mut self, dr: i32, dg: i32, db: i32, da: i32) {
        self.map_colors(|r, g, b, a| {
            (
                (r + dr).clamp(0, 255),
                (g + dg).clamp(0, 255),
                (b + db).clamp(0, 255),
                (a + da).clamp(0, 127),
            )
        });
    }

    /// imageconvolution: 3x3 matrix, divisor and offset; edge pixels
    /// repeat the border.
    pub fn convolution(&mut self, m: &[[f64; 3]; 3], div: f64, offset: f64) -> bool {
        if !self.truecolor {
            return false;
        }
        let src = self.tpixels.clone();
        let (w, h) = (self.sx, self.sy);
        let at = |x: i32, y: i32| -> i32 {
            let x = x.clamp(0, w - 1);
            let y = y.clamp(0, h - 1);
            src[(y * w + x) as usize]
        };
        let saved = self.alpha_blending;
        self.alpha_blending = EFFECT_REPLACE;
        for y in 0..h {
            for x in 0..w {
                let (mut nr, mut ng, mut nb) = (0.0f64, 0.0f64, 0.0f64);
                for (j, row) in m.iter().enumerate() {
                    let yv = (y - 1 + j as i32).clamp(0, h - 1);
                    for (i, &k) in row.iter().enumerate() {
                        let xv = (x - 1 + i as i32).clamp(0, w - 1);
                        let p = at(xv, yv);
                        // Transparent pixels add nothing (bug47946:
                        // transparent white around a shape blurs to
                        // the offset only).
                        let op = (ALPHA_MAX - tc_a(p)) as f64 / ALPHA_MAX as f64;
                        let k = k * op;
                        nr += tc_r(p) as f64 * k;
                        ng += tc_g(p) as f64 * k;
                        nb += tc_b(p) as f64 * k;
                    }
                }
                let f = |v: f64| clamp255(v / div + offset);
                let a = tc_a(src[(y * w + x) as usize]);
                let c = tc(f(nr), f(ng), f(nb), a);
                self.set_pixel_color(x, y, c);
            }
        }
        self.alpha_blending = saved;
        true
    }

    /// IMG_FILTER_EDGEDETECT.
    pub fn filter_edge_detect(&mut self) -> bool {
        self.convolution(
            &[[-1.0, 0.0, -1.0], [0.0, 4.0, 0.0], [-1.0, 0.0, -1.0]],
            1.0,
            127.0,
        )
    }

    /// IMG_FILTER_EMBOSS.
    pub fn filter_emboss(&mut self) -> bool {
        self.convolution(
            &[[1.5, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, -1.5]],
            1.0,
            127.0,
        )
    }

    /// IMG_FILTER_GAUSSIAN_BLUR.
    pub fn filter_gaussian_blur(&mut self) -> bool {
        self.convolution(
            &[[1.0, 2.0, 1.0], [2.0, 4.0, 2.0], [1.0, 2.0, 1.0]],
            16.0,
            0.0,
        )
    }

    /// IMG_FILTER_MEAN_REMOVAL.
    pub fn filter_mean_removal(&mut self) -> bool {
        self.convolution(
            &[[-1.0, -1.0, -1.0], [-1.0, 9.0, -1.0], [-1.0, -1.0, -1.0]],
            1.0,
            0.0,
        )
    }

    /// IMG_FILTER_SMOOTH.
    pub fn filter_smooth(&mut self, weight: f64) -> bool {
        self.convolution(
            &[[1.0, 1.0, 1.0], [1.0, weight, 1.0], [1.0, 1.0, 1.0]],
            weight + 8.0,
            0.0,
        )
    }

    /// IMG_FILTER_SELECTIVE_BLUR: averages each pixel with its
    /// neighbours, weighting each by how similar it is.
    pub fn filter_selective_blur(&mut self) -> bool {
        if !self.truecolor {
            return false;
        }
        let src = self.tpixels.clone();
        let (w, h) = (self.sx, self.sy);
        let at = |x: i32, y: i32| src[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
        for y in 0..h {
            for x in 0..w {
                let cpx = at(x, y);
                let mut flt_r = [[0.0f64; 3]; 3];
                let mut flt_g = [[0.0f64; 3]; 3];
                let mut flt_b = [[0.0f64; 3]; 3];
                let (mut sr, mut sg, mut sb) = (0.0, 0.0, 0.0);
                for j in 0..3 {
                    for i in 0..3 {
                        if i == 1 && j == 1 {
                            continue;
                        }
                        let p = at(x - 1 + i as i32, y - 1 + j as i32);
                        let diff = |a: i32, b: i32| {
                            let d = (a - b).abs() as f64;
                            if d == 0.0 { 1.0 } else { 1.0 / d }
                        };
                        flt_r[j][i] = diff(tc_r(cpx), tc_r(p));
                        flt_g[j][i] = diff(tc_g(cpx), tc_g(p));
                        flt_b[j][i] = diff(tc_b(cpx), tc_b(p));
                        sr += flt_r[j][i];
                        sg += flt_g[j][i];
                        sb += flt_b[j][i];
                    }
                }
                flt_r[1][1] = 0.5;
                flt_g[1][1] = 0.5;
                flt_b[1][1] = 0.5;
                sr += 0.5;
                sg += 0.5;
                sb += 0.5;
                let (mut nr, mut ng, mut nb) = (0.0, 0.0, 0.0);
                for j in 0..3 {
                    for i in 0..3 {
                        let p = at(x - 1 + i as i32, y - 1 + j as i32);
                        nr += tc_r(p) as f64 * flt_r[j][i] / sr;
                        ng += tc_g(p) as f64 * flt_g[j][i] / sg;
                        nb += tc_b(p) as f64 * flt_b[j][i] / sb;
                    }
                }
                let c = tc(clamp255(nr), clamp255(ng), clamp255(nb), tc_a(cpx));
                self.tpixels[(y * w + x) as usize] = c;
            }
        }
        true
    }

    /// IMG_FILTER_PIXELATE: blocks of `size` pixels take the block's
    /// top-left color, or (advanced) its average.
    pub fn filter_pixelate(&mut self, size: i32, advanced: bool) -> bool {
        if size <= 0 {
            return false;
        }
        if size == 1 {
            return true;
        }
        let (w, h) = (self.sx, self.sy);
        let mut y = 0;
        while y < h {
            let mut x = 0;
            while x < w {
                let c = if advanced {
                    let (mut r, mut g, mut b, mut a, mut n) = (0i64, 0i64, 0i64, 0i64, 0i64);
                    for yy in y..(y + size).min(h) {
                        for xx in x..(x + size).min(w) {
                            let p = self.get_truecolor_pixel(xx, yy);
                            r += tc_r(p) as i64;
                            g += tc_g(p) as i64;
                            b += tc_b(p) as i64;
                            a += tc_a(p) as i64;
                            n += 1;
                        }
                    }
                    let n = n.max(1);
                    let (r, g, b, a) = (
                        (r / n) as i32,
                        (g / n) as i32,
                        (b / n) as i32,
                        (a / n) as i32,
                    );
                    if self.truecolor {
                        tc(r, g, b, a)
                    } else {
                        self.color_resolve(r, g, b, a)
                    }
                } else {
                    self.get_pixel(x, y)
                };
                for yy in y..(y + size).min(h) {
                    for xx in x..(x + size).min(w) {
                        self.put_raw(xx, yy, c);
                    }
                }
                x += size;
            }
            y += size;
        }
        true
    }

    /// IMG_FILTER_SCATTER: moves pixels by a random offset in
    /// [-sub, plus) (only to/from pixels of `colors` when given).
    pub fn filter_scatter(
        &mut self,
        sub: i32,
        plus: i32,
        colors: &[i32],
        rng: &mut dyn FnMut() -> u32,
    ) -> bool {
        if sub == 0 && plus == 0 {
            return true;
        }
        let (w, h) = (self.sx, self.sy);
        let range = (plus + sub) as u32;
        for y in 0..h {
            for x in 0..w {
                let off = |r: u32| -> i32 {
                    if range == 0 {
                        0
                    } else {
                        (r % range) as i32 - sub
                    }
                };
                let dx = x + off(rng());
                let dy = y + off(rng());
                if !self.in_bounds(dx, dy) {
                    continue;
                }
                let a = self.get_pixel(x, y);
                let b = self.get_pixel(dx, dy);
                if !colors.is_empty() && !colors.contains(&a) {
                    continue;
                }
                self.put_raw(dx, dy, a);
                self.put_raw(x, y, b);
            }
        }
        true
    }

    /// imagegammacorrect.
    pub fn gamma_correct(&mut self, input: f64, output: f64) {
        let e = input / output;
        let f = |v: i32| -> i32 { ((v as f64 / 255.0).powf(e) * 255.0 + 0.5) as i32 };
        self.map_colors(|r, g, b, a| (f(r), f(g), f(b), a));
    }
}
