//! The GD image: a palette image (up to 256 colors, one index per pixel)
//! or a truecolor image (one packed 0xAARRGGBB value per pixel with GD's
//! 7-bit alpha, 0 opaque to 127 transparent), with the drawing state PHP's
//! `image*()` functions keep on it.
//!
//! Behaviour sources: the PHP manual (GD and Image functions), libgd's
//! published API documentation (semantics of gdImageCreate,
//! gdImageColorAllocate, gdImageSetPixel, ...) and the expected output of
//! php-src's ext/gd tests.

use std::cell::RefCell;
use std::rc::Rc;

/// Maximum palette size.
pub const MAX_COLORS: usize = 256;
/// Fully opaque / fully transparent alpha values.
pub const ALPHA_OPAQUE: i32 = 0;
pub const ALPHA_TRANSPARENT: i32 = 127;
pub const ALPHA_MAX: i32 = 127;

/// Special colors (PHP's IMG_COLOR_* constants).
pub const STYLED: i32 = -2;
pub const BRUSHED: i32 = -3;
pub const STYLED_BRUSHED: i32 = -4;
pub const TILED: i32 = -5;
pub const TRANSPARENT: i32 = -6;
pub const ANTIALIASED: i32 = -7;

/// Layer effects (IMG_EFFECT_*).
pub const EFFECT_REPLACE: i32 = 0;
pub const EFFECT_ALPHABLEND: i32 = 1;
pub const EFFECT_NORMAL: i32 = 2;
pub const EFFECT_OVERLAY: i32 = 3;
pub const EFFECT_MULTIPLY: i32 = 4;

#[inline]
pub fn tc(r: i32, g: i32, b: i32, a: i32) -> i32 {
    (a << 24) | (r << 16) | (g << 8) | b
}
#[inline]
pub fn tc_a(c: i32) -> i32 {
    (c >> 24) & 0x7f
}
#[inline]
pub fn tc_r(c: i32) -> i32 {
    (c >> 16) & 0xff
}
#[inline]
pub fn tc_g(c: i32) -> i32 {
    (c >> 8) & 0xff
}
#[inline]
pub fn tc_b(c: i32) -> i32 {
    c & 0xff
}

/// A shared image handle (brushes and tiles keep one).
pub type Shared = Rc<RefCell<Image>>;

#[derive(Clone)]
pub struct Image {
    pub sx: i32,
    pub sy: i32,
    pub truecolor: bool,
    /// Palette indices (palette images).
    pub pixels: Vec<u8>,
    /// Packed colors (truecolor images).
    pub tpixels: Vec<i32>,
    pub red: [i32; MAX_COLORS],
    pub green: [i32; MAX_COLORS],
    pub blue: [i32; MAX_COLORS],
    pub alpha: [i32; MAX_COLORS],
    /// Deallocated palette slots.
    pub open: [bool; MAX_COLORS],
    pub colors_total: i32,
    /// Transparent color index (palette) or value (truecolor); -1: none.
    pub transparent: i32,
    pub interlace: bool,
    pub thick: i32,
    /// Layer effect for truecolor drawing (alpha blending on: ALPHABLEND).
    pub alpha_blending: i32,
    pub save_alpha: bool,
    pub aa: bool,
    pub aa_color: i32,
    pub aa_dont_blend: i32,
    pub style: Vec<i32>,
    pub style_pos: usize,
    pub brush: Option<Shared>,
    pub tile: Option<Shared>,
    pub cx1: i32,
    pub cy1: i32,
    pub cx2: i32,
    pub cy2: i32,
    pub res_x: u32,
    pub res_y: u32,
    pub interpolation: i32,
    /// Palette images: the per-brush-color map built when brushing.
    pub paletted_from_quant: bool,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Image({}x{}, {})",
            self.sx,
            self.sy,
            if self.truecolor {
                "truecolor"
            } else {
                "palette"
            }
        )
    }
}

/// IMG_BILINEAR_FIXED, the default interpolation method.
pub const DEFAULT_INTERPOLATION: i32 = 3;

impl Image {
    fn blank(sx: i32, sy: i32, truecolor: bool) -> Image {
        let n = (sx as usize) * (sy as usize);
        Image {
            sx,
            sy,
            truecolor,
            pixels: if truecolor { Vec::new() } else { vec![0; n] },
            tpixels: if truecolor { vec![0; n] } else { Vec::new() },
            red: [0; MAX_COLORS],
            green: [0; MAX_COLORS],
            blue: [0; MAX_COLORS],
            alpha: [0; MAX_COLORS],
            open: [true; MAX_COLORS],
            colors_total: 0,
            transparent: -1,
            interlace: false,
            thick: 1,
            alpha_blending: if truecolor {
                EFFECT_ALPHABLEND
            } else {
                EFFECT_REPLACE
            },
            save_alpha: false,
            aa: false,
            aa_color: 0,
            aa_dont_blend: -1,
            style: Vec::new(),
            style_pos: 0,
            brush: None,
            tile: None,
            cx1: 0,
            cy1: 0,
            cx2: sx - 1,
            cy2: sy - 1,
            res_x: 96,
            res_y: 96,
            interpolation: DEFAULT_INTERPOLATION,
            paletted_from_quant: false,
        }
    }

    /// A palette image (gdImageCreate). `None` when the size overflows.
    pub fn new_palette(sx: i32, sy: i32) -> Option<Image> {
        if sx <= 0 || sy <= 0 || overflow(sx, sy, 1) {
            return None;
        }
        Some(Image::blank(sx, sy, false))
    }

    /// A truecolor image (gdImageCreateTrueColor), all black.
    pub fn new_truecolor(sx: i32, sy: i32) -> Option<Image> {
        if sx <= 0 || sy <= 0 || overflow(sx, sy, 4) {
            return None;
        }
        Some(Image::blank(sx, sy, true))
    }

    pub fn new_like(&self, sx: i32, sy: i32) -> Option<Image> {
        if self.truecolor {
            Image::new_truecolor(sx, sy)
        } else {
            Image::new_palette(sx, sy)
        }
    }

    #[inline]
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.sx && y < self.sy
    }

    #[inline]
    pub fn in_clip(&self, x: i32, y: i32) -> bool {
        x >= self.cx1 && y >= self.cy1 && x <= self.cx2 && y <= self.cy2
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> usize {
        y as usize * self.sx as usize + x as usize
    }

    /// The raw pixel (index or packed color); 0 outside the image.
    #[inline]
    pub fn get_pixel(&self, x: i32, y: i32) -> i32 {
        if !self.in_bounds(x, y) {
            return 0;
        }
        let i = self.idx(x, y);
        if self.truecolor {
            self.tpixels[i]
        } else {
            self.pixels[i] as i32
        }
    }

    /// The pixel as a packed truecolor value (palette entries converted).
    #[inline]
    pub fn get_truecolor_pixel(&self, x: i32, y: i32) -> i32 {
        let p = self.get_pixel(x, y);
        if self.truecolor {
            p
        } else {
            self.palette_to_tc(p)
        }
    }

    /// A palette entry as a packed value (transparent index: alpha 127).
    #[inline]
    pub fn palette_to_tc(&self, p: i32) -> i32 {
        let p = (p & 0xff) as usize;
        let a = if self.transparent == p as i32 {
            ALPHA_TRANSPARENT
        } else {
            self.alpha[p]
        };
        tc(self.red[p], self.green[p], self.blue[p], a)
    }

    /// Writes the raw pixel value without effects or clipping.
    #[inline]
    pub fn put_raw(&mut self, x: i32, y: i32, c: i32) {
        if self.in_bounds(x, y) {
            let i = self.idx(x, y);
            if self.truecolor {
                self.tpixels[i] = c;
            } else {
                self.pixels[i] = c as u8;
            }
        }
    }

    // ------------------------------------------------------------ colors

    pub fn red_of(&self, c: i32) -> i32 {
        if self.truecolor {
            tc_r(c)
        } else {
            self.red[(c & 0xff) as usize]
        }
    }
    pub fn green_of(&self, c: i32) -> i32 {
        if self.truecolor {
            tc_g(c)
        } else {
            self.green[(c & 0xff) as usize]
        }
    }
    pub fn blue_of(&self, c: i32) -> i32 {
        if self.truecolor {
            tc_b(c)
        } else {
            self.blue[(c & 0xff) as usize]
        }
    }
    pub fn alpha_of(&self, c: i32) -> i32 {
        if self.truecolor {
            tc_a(c)
        } else {
            self.alpha[(c & 0xff) as usize]
        }
    }

    /// gdImageColorAllocateAlpha: the first free palette slot, or -1.
    pub fn color_allocate(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.truecolor {
            return tc(r, g, b, a);
        }
        let mut ct = -1;
        for i in 0..self.colors_total {
            if self.open[i as usize] {
                ct = i;
                break;
            }
        }
        if ct == -1 {
            ct = self.colors_total;
            if ct as usize == MAX_COLORS {
                return -1;
            }
            self.colors_total += 1;
        }
        let i = ct as usize;
        self.red[i] = r;
        self.green[i] = g;
        self.blue[i] = b;
        self.alpha[i] = a;
        self.open[i] = false;
        ct
    }

    /// gdImageColorDeallocate.
    pub fn color_deallocate(&mut self, c: i32) {
        if self.truecolor {
            return;
        }
        if c >= 0 && (c as usize) < MAX_COLORS {
            self.open[c as usize] = true;
        }
    }

    /// gdImageColorClosestAlpha: the nearest allocated color (squared
    /// RGBA distance), -1 for an empty palette.
    pub fn color_closest(&self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.truecolor {
            return tc(r, g, b, a);
        }
        let mut ct = -1;
        let mut best = i64::MAX;
        for i in 0..self.colors_total as usize {
            if self.open[i] {
                continue;
            }
            let dr = (self.red[i] - r) as i64;
            let dg = (self.green[i] - g) as i64;
            let db = (self.blue[i] - b) as i64;
            let da = (self.alpha[i] - a) as i64;
            let d = dr * dr + dg * dg + db * db + da * da;
            if d < best {
                best = d;
                ct = i as i32;
            }
        }
        ct
    }

    /// gdImageColorClosestHWB: nearest color by hue/whiteness/blackness.
    pub fn color_closest_hwb(&self, r: i32, g: i32, b: i32) -> i32 {
        if self.truecolor {
            return tc(r, g, b, 0);
        }
        let mut ct = -1;
        let mut best = f64::MAX;
        for i in 0..self.colors_total as usize {
            if self.open[i] {
                continue;
            }
            let d = hwb_diff(r, g, b, self.red[i], self.green[i], self.blue[i]);
            if d < best {
                best = d;
                ct = i as i32;
            }
        }
        ct
    }

    /// gdImageColorExactAlpha: the allocated entry with exactly these
    /// components, -1 if none.
    pub fn color_exact(&self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.truecolor {
            return tc(r, g, b, a);
        }
        for i in 0..self.colors_total as usize {
            if self.open[i] {
                continue;
            }
            if self.red[i] == r && self.green[i] == g && self.blue[i] == b && self.alpha[i] == a {
                return i as i32;
            }
        }
        -1
    }

    /// gdImageColorResolveAlpha: exact match, else a new entry, else the
    /// closest one.
    pub fn color_resolve(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.truecolor {
            return tc(r, g, b, a);
        }
        let mut op = -1;
        let mut ct = -1;
        let mut best = i64::MAX;
        for i in 0..self.colors_total as usize {
            if self.open[i] {
                if op == -1 {
                    op = i as i32;
                }
                continue;
            }
            let dr = (self.red[i] - r) as i64;
            let dg = (self.green[i] - g) as i64;
            let db = (self.blue[i] - b) as i64;
            let da = (self.alpha[i] - a) as i64;
            let d = dr * dr + dg * dg + db * db + da * da;
            if d == 0 {
                return i as i32;
            }
            if d < best {
                best = d;
                ct = i as i32;
            }
        }
        if op == -1 {
            op = self.colors_total;
            if op as usize == MAX_COLORS {
                return ct;
            }
            self.colors_total += 1;
        }
        let i = op as usize;
        self.red[i] = r;
        self.green[i] = g;
        self.blue[i] = b;
        self.alpha[i] = a;
        self.open[i] = false;
        op
    }

    /// gdImageColorTransparent.
    pub fn set_transparent(&mut self, c: i32) {
        if c < 0 {
            self.transparent = -1;
            return;
        }
        if !self.truecolor {
            if c >= MAX_COLORS as i32 {
                return;
            }
            if self.transparent != -1 {
                let t = self.transparent as usize;
                self.alpha[t] = ALPHA_OPAQUE;
            }
            self.alpha[c as usize] = ALPHA_TRANSPARENT;
        }
        self.transparent = c;
    }

    /// Sets the clipping rectangle (clamped to the image).
    pub fn set_clip(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let cl = |v: i32, max: i32| v.clamp(0, max);
        self.cx1 = cl(x1, self.sx - 1);
        self.cy1 = cl(y1, self.sy - 1);
        self.cx2 = cl(x2, self.sx - 1);
        self.cy2 = cl(y2, self.sy - 1);
    }

    pub fn reset_clip(&mut self) {
        self.cx1 = 0;
        self.cy1 = 0;
        self.cx2 = self.sx - 1;
        self.cy2 = self.sy - 1;
    }

    // ------------------------------------------------------------ pixels

    /// gdImageSetPixel: draws one pixel with color `c`, which may be one
    /// of the special colors.
    pub fn set_pixel(&mut self, x: i32, y: i32, c: i32) {
        match c {
            STYLED => {
                if self.style.is_empty() {
                    return;
                }
                let p = self.style[self.style_pos % self.style.len()];
                self.style_pos = (self.style_pos + 1) % self.style.len();
                if p != TRANSPARENT {
                    self.set_pixel(x, y, p);
                }
            }
            STYLED_BRUSHED => {
                if self.style.is_empty() {
                    return;
                }
                let p = self.style[self.style_pos % self.style.len()];
                self.style_pos = (self.style_pos + 1) % self.style.len();
                if p != TRANSPARENT && p != 0 {
                    self.brush_at(x, y);
                }
            }
            BRUSHED => self.brush_at(x, y),
            TILED => self.tile_at(x, y),
            ANTIALIASED => {
                let c = self.aa_color;
                self.set_pixel(x, y, c);
            }
            TRANSPARENT => {}
            _ => self.set_pixel_color(x, y, c),
        }
    }

    /// Sets a pixel to a real color, with the layer effect and clipping.
    #[inline]
    pub fn set_pixel_color(&mut self, x: i32, y: i32, c: i32) {
        if !self.in_bounds(x, y) || !self.in_clip(x, y) {
            return;
        }
        let i = self.idx(x, y);
        if self.truecolor {
            let dst = self.tpixels[i];
            self.tpixels[i] = match self.alpha_blending {
                EFFECT_REPLACE => c,
                EFFECT_ALPHABLEND | EFFECT_NORMAL => alpha_blend(dst, c),
                EFFECT_OVERLAY => layer_overlay(dst, c),
                EFFECT_MULTIPLY => layer_multiply(dst, c),
                _ => alpha_blend(dst, c),
            };
        } else {
            self.pixels[i] = c as u8;
        }
    }

    fn brush_at(&mut self, x: i32, y: i32) {
        let Some(brush) = self.brush.clone() else {
            return;
        };
        let Ok(b) = brush.try_borrow() else {
            return;
        };
        let (bw, bh) = (b.sx, b.sy);
        let sx0 = x - bw / 2;
        let sy0 = y - bh / 2;
        if self.truecolor {
            for by in 0..bh {
                for bx in 0..bw {
                    let p = b.get_pixel(bx, by);
                    if p == b.transparent {
                        continue;
                    }
                    let c = if b.truecolor { p } else { b.palette_to_tc(p) };
                    self.set_pixel_color(sx0 + bx, sy0 + by, c);
                }
            }
        } else {
            for by in 0..bh {
                for bx in 0..bw {
                    let p = b.get_pixel(bx, by);
                    if p == b.transparent {
                        continue;
                    }
                    let c = if b.truecolor {
                        self.color_resolve(tc_r(p), tc_g(p), tc_b(p), tc_a(p))
                    } else {
                        let pi = p as usize;
                        self.color_resolve(b.red[pi], b.green[pi], b.blue[pi], b.alpha[pi])
                    };
                    self.set_pixel_color(sx0 + bx, sy0 + by, c);
                }
            }
        }
    }

    fn tile_at(&mut self, x: i32, y: i32) {
        let Some(tile) = self.tile.clone() else {
            return;
        };
        let Ok(t) = tile.try_borrow() else {
            return;
        };
        let tx = x.rem_euclid(t.sx);
        let ty = y.rem_euclid(t.sy);
        let p = t.get_pixel(tx, ty);
        if p == t.transparent {
            return;
        }
        let c = if self.truecolor {
            if t.truecolor { p } else { t.palette_to_tc(p) }
        } else if t.truecolor {
            self.color_resolve(tc_r(p), tc_g(p), tc_b(p), tc_a(p))
        } else {
            let pi = p as usize;
            self.color_resolve(t.red[pi], t.green[pi], t.blue[pi], t.alpha[pi])
        };
        self.set_pixel_color(x, y, c);
    }

    /// The color a tile gives at (x, y), for fills (None: transparent).
    pub fn tile_color(&mut self, x: i32, y: i32) -> Option<i32> {
        let tile = self.tile.clone()?;
        let t = tile.try_borrow().ok()?;
        let p = t.get_pixel(x.rem_euclid(t.sx), y.rem_euclid(t.sy));
        if p == t.transparent {
            return None;
        }
        Some(if self.truecolor {
            if t.truecolor { p } else { t.palette_to_tc(p) }
        } else if t.truecolor {
            self.color_resolve(tc_r(p), tc_g(p), tc_b(p), tc_a(p))
        } else {
            let pi = p as usize;
            self.color_resolve(t.red[pi], t.green[pi], t.blue[pi], t.alpha[pi])
        })
    }
}

/// Whether sx*sy*bpp overflows an int (gd's overflow2 checks).
pub fn overflow(sx: i32, sy: i32, bpp: i64) -> bool {
    let n = sx as i64 * sy as i64;
    n > i32::MAX as i64 || n * bpp > i32::MAX as i64
}

/// Composites `src` over `dst` (both packed, 7-bit alpha).
#[inline]
pub fn alpha_blend(dst: i32, src: i32) -> i32 {
    let src_alpha = tc_a(src);
    if src_alpha == ALPHA_OPAQUE {
        return src;
    }
    let dst_alpha = tc_a(dst);
    if src_alpha == ALPHA_TRANSPARENT {
        return dst;
    }
    if dst_alpha == ALPHA_TRANSPARENT {
        return src;
    }
    // Weights: the source's opacity, and the destination's opacity scaled
    // by how much of it shows through the source.
    let src_weight = ALPHA_TRANSPARENT - src_alpha;
    let dst_weight = (ALPHA_TRANSPARENT - dst_alpha) * src_alpha / ALPHA_MAX;
    let tot_weight = src_weight + dst_weight;
    let alpha = src_alpha * dst_alpha / ALPHA_MAX;
    let r = (tc_r(src) * src_weight + tc_r(dst) * dst_weight) / tot_weight;
    let g = (tc_g(src) * src_weight + tc_g(dst) * dst_weight) / tot_weight;
    let b = (tc_b(src) * src_weight + tc_b(dst) * dst_weight) / tot_weight;
    tc(r, g, b, alpha)
}

/// IMG_EFFECT_OVERLAY.
pub fn layer_overlay(dst: i32, src: i32) -> i32 {
    let src_alpha = tc_a(src);
    let dst_alpha = tc_a(dst);
    let ov = |d: i32, s: i32| -> i32 {
        // Overlay: multiply in the shadows, screen in the highlights.
        let (d, s) = (d as f64 / 255.0, s as f64 / 255.0);
        let v = if d < 0.5 {
            2.0 * d * s
        } else {
            1.0 - 2.0 * (1.0 - d) * (1.0 - s)
        };
        (v * 255.0) as i32
    };
    let a = src_alpha * dst_alpha / ALPHA_MAX;
    tc(
        ov(tc_r(dst), tc_r(src)),
        ov(tc_g(dst), tc_g(src)),
        ov(tc_b(dst), tc_b(src)),
        a,
    )
}

/// IMG_EFFECT_MULTIPLY.
pub fn layer_multiply(dst: i32, src: i32) -> i32 {
    let src_alpha = tc_a(src);
    let dst_alpha = tc_a(dst);
    let a1 = ALPHA_MAX - src_alpha;
    let a2 = ALPHA_MAX - dst_alpha;
    let alpha = ALPHA_MAX - a1 * a2 / ALPHA_MAX;
    let mul = |s: i32, d: i32| -> i32 {
        let sv = (s * a1 + 255 * (ALPHA_MAX - a1)) / ALPHA_MAX;
        let dv = (d * a2 + 255 * (ALPHA_MAX - a2)) / ALPHA_MAX;
        sv * dv / 255
    };
    tc(
        mul(tc_r(src), tc_r(dst)),
        mul(tc_g(src), tc_g(dst)),
        mul(tc_b(src), tc_b(dst)),
        alpha,
    )
}

/// Distance in the HWB color space (Smith & Lyons, "HWB — A More
/// Intuitive Hue-Based Color Model", 1996).
fn hwb_diff(r1: i32, g1: i32, b1: i32, r2: i32, g2: i32, b2: i32) -> f64 {
    let (h1, w1, k1) = rgb_to_hwb(r1, g1, b1);
    let (h2, w2, k2) = rgb_to_hwb(r2, g2, b2);
    let mut diff = 0.0;
    if h1 >= 0.0 && h2 >= 0.0 {
        let mut d = (h1 - h2).abs();
        if d > 3.0 {
            d = 6.0 - d;
        }
        diff = d * d;
    }
    diff + (w1 - w2) * (w1 - w2) + (k1 - k2) * (k1 - k2)
}

fn rgb_to_hwb(r: i32, g: i32, b: i32) -> (f64, f64, f64) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let w = r.min(g).min(b);
    let v = r.max(g).max(b);
    let k = 1.0 - v;
    if v == w {
        return (-1.0, w, k);
    }
    let f = if r == w {
        g - b
    } else if g == w {
        b - r
    } else {
        r - g
    };
    let i = if r == w {
        3.0
    } else if g == w {
        5.0
    } else {
        1.0
    };
    (i - f / (v - w), w, k)
}
