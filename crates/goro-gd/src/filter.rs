//! gd_filter.c: imagefilter() and imageconvolution() filters, pixelate and
//! scatter.

use crate::*;

/// gdPixelateMode
pub const PIXELATE_UPPERLEFT: u32 = 0;
pub const PIXELATE_AVERAGE: u32 = 1;

/// GET_PIXEL_FUNCTION(src)
#[inline]
fn get_px(src: &Image, im: &Image, x: i32, y: i32) -> i32 {
    if src.true_color {
        im.get_true_color_pixel(x, y)
    } else {
        im.get_pixel(x, y)
    }
}

/// glibc's srand()/rand() (random_r.c, TYPE_3), used by the scatter filter
struct GlibcRand {
    r: [i32; 34],
    f: usize,
    b: usize,
}

impl GlibcRand {
    fn new(seed: u32) -> GlibcRand {
        let mut r = [0i32; 34];
        let seed = if seed == 0 { 1 } else { seed };
        r[0] = seed as i32;
        let mut word = seed as i32 as i64;
        for slot in r.iter_mut().take(31).skip(1) {
            // word = 16807 * hi ... (Schrage's method, as glibc)
            let hi = word / 127773;
            let lo = word % 127773;
            word = 16807 * lo - 2836 * hi;
            if word < 0 {
                word += 2147483647;
            }
            *slot = word as i32;
        }
        let mut g = GlibcRand { r, f: 3, b: 0 };
        // state: fptr = &state[SEP_3] (3), rptr = &state[0]; discard 310
        for _ in 0..310 {
            g.next();
        }
        g
    }

    fn next(&mut self) -> i32 {
        let v = self.r[self.f].wrapping_add(self.r[self.b]);
        self.r[self.f] = v;
        let result = ((v as u32) >> 1) as i32;
        self.f += 1;
        if self.f >= 31 {
            self.f = 0;
            self.b += 1;
        } else {
            self.b += 1;
            if self.b >= 31 {
                self.b = 0;
            }
        }
        result
    }
}

/// gdScatter
#[derive(Clone, Debug, Default)]
pub struct Scatter {
    pub sub: i32,
    pub plus: i32,
    pub colors: Vec<i32>,
    pub seed: u32,
}

/// GD_SCATTER_SEED()
fn scatter_seed() -> u32 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (t.wrapping_mul(std::process::id() as i64)) as u32
}

/// gdImageScatter()
pub fn scatter(im: &mut Image, sub: i32, plus: i32) -> bool {
    let s = Scatter {
        sub,
        plus,
        colors: Vec::new(),
        seed: scatter_seed(),
    };
    scatter_ex(im, &s)
}

/// gdImageScatterColor()
pub fn scatter_color(im: &mut Image, sub: i32, plus: i32, colors: &[i32]) -> bool {
    let s = Scatter {
        sub,
        plus,
        colors: colors.to_vec(),
        seed: scatter_seed(),
    };
    scatter_ex(im, &s)
}

/// gdImageScatterEx()
pub fn scatter_ex(im: &mut Image, s: &Scatter) -> bool {
    let (sub, plus) = (s.sub, s.plus);
    if plus == 0 && sub == 0 {
        return true;
    } else if sub >= plus {
        return false;
    }
    let mut rng = GlibcRand::new(s.seed);
    let range = plus.wrapping_sub(sub);
    for y in 0..im.sy {
        for x in 0..im.sx {
            let dest_x = x.wrapping_add((rng.next() % range).wrapping_add(sub));
            let dest_y = y.wrapping_add((rng.next() % range).wrapping_add(sub));
            if !im.bounds_safe(dest_x, dest_y) {
                continue;
            }
            let pxl = im.get_pixel(x, y);
            let new_pxl = im.get_pixel(dest_x, dest_y);
            if !s.colors.is_empty() {
                for &c in &s.colors {
                    if pxl == c {
                        im.set_pixel(dest_x, dest_y, pxl);
                        im.set_pixel(x, y, new_pxl);
                    }
                }
            } else {
                im.set_pixel(dest_x, dest_y, pxl);
                im.set_pixel(x, y, new_pxl);
            }
        }
    }
    true
}

/// gdImagePixelate()
pub fn pixelate(im: &mut Image, block_size: i32, mode: u32) -> bool {
    if block_size <= 0 {
        return false;
    } else if block_size == 1 {
        return true;
    }
    match mode {
        PIXELATE_UPPERLEFT => {
            let mut y = 0;
            while y < im.sy {
                let mut x = 0;
                while x < im.sx {
                    if im.bounds_safe(x, y) {
                        let c = im.get_pixel(x, y);
                        im.filled_rectangle(x, y, x + block_size - 1, y + block_size - 1, c);
                    }
                    x += block_size;
                }
                y += block_size;
            }
        }
        PIXELATE_AVERAGE => {
            let mut y = 0;
            while y < im.sy {
                let mut x = 0;
                while x < im.sx {
                    let (mut a, mut r, mut g, mut b, mut total) = (0i32, 0i32, 0i32, 0i32, 0i32);
                    for cy in 0..block_size {
                        for cx in 0..block_size {
                            if !im.bounds_safe(x + cx, y + cy) {
                                continue;
                            }
                            let c = im.get_pixel(x + cx, y + cy);
                            a += im.alpha_of(c);
                            r += im.red_of(c);
                            g += im.green_of(c);
                            b += im.blue_of(c);
                            total += 1;
                        }
                    }
                    if total > 0 {
                        let c = im.color_resolve_alpha(r / total, g / total, b / total, a / total);
                        im.filled_rectangle(x, y, x + block_size - 1, y + block_size - 1, c);
                    }
                    x += block_size;
                }
                y += block_size;
            }
        }
        _ => return false,
    }
    true
}

fn alloc_or_closest(src: &mut Image, r: i32, g: i32, b: i32, a: i32) -> i32 {
    let p = src.color_allocate_alpha(r, g, b, a);
    if p == -1 {
        src.color_closest_alpha(r, g, b, a)
    } else {
        p
    }
}

fn read_px(src: &Image, x: i32, y: i32) -> (i32, i32, i32, i32) {
    let pxl = get_px(src, src, x, y);
    (
        src.red_of(pxl),
        src.green_of(pxl),
        src.blue_of(pxl),
        src.alpha_of(pxl),
    )
}

/// gdImageNegate()
pub fn negate(src: &mut Image) -> bool {
    for y in 0..src.sy {
        for x in 0..src.sx {
            let (r, g, b, a) = read_px(src, x, y);
            let p = alloc_or_closest(src, 255 - r, 255 - g, 255 - b, a);
            src.set_pixel(x, y, p);
        }
    }
    true
}

/// gdImageGrayScale()
pub fn grayscale(src: &mut Image) -> bool {
    let alpha_blending = src.alpha_blending_flag;
    src.alpha_blending_flag = EFFECT_REPLACE;
    for y in 0..src.sy {
        for x in 0..src.sx {
            let (r, g, b, a) = read_px(src, x, y);
            let v = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) as i32;
            let p = alloc_or_closest(src, v, v, v, a);
            src.set_pixel(x, y, p);
        }
    }
    src.alpha_blending_flag = alpha_blending;
    true
}

/// gdImageBrightness()
pub fn brightness(src: &mut Image, brightness: i32) -> bool {
    if !(-255..=255).contains(&brightness) {
        return false;
    }
    if brightness == 0 {
        return true;
    }
    for y in 0..src.sy {
        for x in 0..src.sx {
            let (r, g, b, a) = read_px(src, x, y);
            let r = (r + brightness).clamp(0, 255);
            let g = (g + brightness).clamp(0, 255);
            let b = (b + brightness).clamp(0, 255);
            let p = alloc_or_closest(src, r, g, b, a);
            src.set_pixel(x, y, p);
        }
    }
    true
}

/// gdImageContrast()
pub fn contrast(src: &mut Image, contrast: f64) -> bool {
    let mut contrast = (100.0 - contrast) / 100.0;
    contrast *= contrast;
    let f = |c: i32| -> f64 {
        let mut v = c as f64 / 255.0;
        v -= 0.5;
        v *= contrast;
        v += 0.5;
        v *= 255.0;
        v.clamp(0.0, 255.0)
    };
    for y in 0..src.sy {
        for x in 0..src.sx {
            let (r, g, b, a) = read_px(src, x, y);
            let (rf, gf, bf) = (f(r), f(g), f(b));
            let p = alloc_or_closest(src, rf as i32, gf as i32, bf as i32, a);
            src.set_pixel(x, y, p);
        }
    }
    true
}

/// gdImageColor()
pub fn color(src: &mut Image, red: i32, green: i32, blue: i32, alpha: i32) -> bool {
    for y in 0..src.sy {
        for x in 0..src.sx {
            let (r, g, b, a) = read_px(src, x, y);
            let r = r.wrapping_add(red).clamp(0, 255);
            let g = g.wrapping_add(green).clamp(0, 255);
            let b = b.wrapping_add(blue).clamp(0, 255);
            let a = a.wrapping_add(alpha).clamp(0, 127);
            let p = alloc_or_closest(src, r, g, b, a);
            src.set_pixel(x, y, p);
        }
    }
    true
}

#[inline]
fn clamp_f(v: f32) -> f32 {
    v.clamp(0.0, 255.0)
}

/// gdImageConvolution()
pub fn convolution(src: &mut Image, filter: &[[f32; 3]; 3], filter_div: f32, offset: f32) -> bool {
    let Some(mut srcback) = Image::create_true_color(src.sx, src.sy) else {
        return false;
    };
    srcback.save_alpha_flag = 1;
    let np = srcback.color_allocate_alpha(0, 0, 0, 127);
    srcback.fill(0, 0, np);
    crate::gd::copy(&mut srcback, src, false, 0, 0, 0, 0, src.sx, src.sy);
    for y in 0..src.sy {
        for x in 0..src.sx {
            let (mut new_r, mut new_g, mut new_b) = (0f32, 0f32, 0f32);
            let pxl = get_px(src, &srcback, x, y);
            let new_a = srcback.alpha_of(pxl);
            for (j, frow) in filter.iter().enumerate() {
                let yv = (y - 1 + j as i32).max(0).min(src.sy - 1);
                for (i, &fv) in frow.iter().enumerate() {
                    let pxl = get_px(src, &srcback, (x - 1 + i as i32).max(0).min(src.sx - 1), yv);
                    new_r += srcback.red_of(pxl) as f32 * fv;
                    new_g += srcback.green_of(pxl) as f32 * fv;
                    new_b += srcback.blue_of(pxl) as f32 * fv;
                }
            }
            new_r = clamp_f((new_r / filter_div) + offset);
            new_g = clamp_f((new_g / filter_div) + offset);
            new_b = clamp_f((new_b / filter_div) + offset);
            let p = alloc_or_closest(src, new_r as i32, new_g as i32, new_b as i32, new_a);
            src.set_pixel(x, y, p);
        }
    }
    true
}

/// gdImageSelectiveBlur()
pub fn selective_blur(src: &mut Image) -> bool {
    let Some(mut srcback) = Image::create_true_color(src.sx, src.sy) else {
        return false;
    };
    crate::gd::copy(&mut srcback, src, false, 0, 0, 0, 0, src.sx, src.sy);
    let mut new_a = 0;
    for y in 0..src.sy {
        for x in 0..src.sx {
            let mut flt_r = [[0f32; 3]; 3];
            let mut flt_g = [[0f32; 3]; 3];
            let mut flt_b = [[0f32; 3]; 3];
            let (mut flt_r_sum, mut flt_g_sum, mut flt_b_sum) = (0f32, 0f32, 0f32);
            let cpxl = get_px(src, src, x, y);
            for j in 0..3 {
                for i in 0..3 {
                    if j == 1 && i == 1 {
                        flt_r[1][1] = 0.5;
                        flt_g[1][1] = 0.5;
                        flt_b[1][1] = 0.5;
                    } else {
                        let pxl = get_px(src, src, x - 1 + i as i32, y - 1 + j as i32);
                        new_a = srcback.alpha_of(pxl);
                        let w = |a: i32, b: i32| -> f32 {
                            let mut d = a as f32 - b as f32;
                            if d < 0.0 {
                                d = -d;
                            }
                            if d != 0.0 { 1.0 / d } else { 1.0 }
                        };
                        flt_r[j][i] = w(srcback.red_of(cpxl), srcback.red_of(pxl));
                        flt_g[j][i] = w(srcback.green_of(cpxl), srcback.green_of(pxl));
                        flt_b[j][i] = w(srcback.blue_of(cpxl), srcback.blue_of(pxl));
                    }
                    flt_r_sum += flt_r[j][i];
                    flt_g_sum += flt_g[j][i];
                    flt_b_sum += flt_b[j][i];
                }
            }
            for j in 0..3 {
                for i in 0..3 {
                    if flt_r_sum != 0.0 {
                        flt_r[j][i] /= flt_r_sum;
                    }
                    if flt_g_sum != 0.0 {
                        flt_g[j][i] /= flt_g_sum;
                    }
                    if flt_b_sum != 0.0 {
                        flt_b[j][i] /= flt_b_sum;
                    }
                }
            }
            let (mut new_r, mut new_g, mut new_b) = (0f32, 0f32, 0f32);
            for j in 0..3 {
                for i in 0..3 {
                    let pxl = get_px(src, src, x - 1 + i as i32, y - 1 + j as i32);
                    new_r += srcback.red_of(pxl) as f32 * flt_r[j][i];
                    new_g += srcback.green_of(pxl) as f32 * flt_g[j][i];
                    new_b += srcback.blue_of(pxl) as f32 * flt_b[j][i];
                }
            }
            new_r = clamp_f(new_r);
            new_g = clamp_f(new_g);
            new_b = clamp_f(new_b);
            let p = alloc_or_closest(src, new_r as i32, new_g as i32, new_b as i32, new_a);
            src.set_pixel(x, y, p);
        }
    }
    true
}

/// gdImageEdgeDetectQuick()
pub fn edge_detect_quick(src: &mut Image) -> bool {
    let f = [[-1.0, 0.0, -1.0], [0.0, 4.0, 0.0], [-1.0, 0.0, -1.0]];
    convolution(src, &f, 1.0, 127.0)
}

/// gdImageGaussianBlur()
pub fn gaussian_blur(im: &mut Image) -> bool {
    let f = [[1.0, 2.0, 1.0], [2.0, 4.0, 2.0], [1.0, 2.0, 1.0]];
    convolution(im, &f, 16.0, 0.0)
}

/// gdImageEmboss()
pub fn emboss(im: &mut Image) -> bool {
    let f = [[1.5, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, -1.5]];
    convolution(im, &f, 1.0, 127.0)
}

/// gdImageMeanRemoval()
pub fn mean_removal(im: &mut Image) -> bool {
    let f = [[-1.0, -1.0, -1.0], [-1.0, 9.0, -1.0], [-1.0, -1.0, -1.0]];
    convolution(im, &f, 1.0, 0.0)
}

/// gdImageSmooth()
pub fn smooth(im: &mut Image, weight: f32) -> bool {
    let mut f = [[1.0, 1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0]];
    f[1][1] = weight;
    convolution(im, &f, weight + 8.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::GlibcRand;

    #[test]
    fn glibc_rand_sequence() {
        // srand(1); rand() x3 with glibc
        let mut r = GlibcRand::new(1);
        assert_eq!(r.next(), 1804289383);
        assert_eq!(r.next(), 846930886);
        assert_eq!(r.next(), 1681692777);
        let mut r = GlibcRand::new(42);
        assert_eq!(r.next(), 71876166);
        let mut r = GlibcRand::new(3000000000);
        assert_eq!(r.next(), 2058147116);
    }
}
