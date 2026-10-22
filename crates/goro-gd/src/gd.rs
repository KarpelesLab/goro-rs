//! gd.c: image creation, colors, pixels and the drawing primitives.

use crate::tables::{COS_T, SIN_T};
use crate::*;

#[inline]
fn floor_cast(x: f64) -> i64 {
    x as i64
}

impl Image {
    fn blank(sx: i32, sy: i32) -> Image {
        Image {
            pixels: Vec::new(),
            sx,
            sy,
            colors_total: 0,
            red: [0; MAX_COLORS],
            green: [0; MAX_COLORS],
            blue: [0; MAX_COLORS],
            open: [0; MAX_COLORS],
            transparent: -1,
            poly_ints: Vec::new(),
            brush: None,
            tile: None,
            brush_color_map: [0; MAX_COLORS],
            tile_color_map: [0; MAX_COLORS],
            style: None,
            style_pos: 0,
            interlace: 0,
            thick: 1,
            alpha: [0; MAX_COLORS],
            true_color: false,
            tpixels: Vec::new(),
            alpha_blending_flag: 0,
            save_alpha_flag: 0,
            aa: 0,
            aa_color: 0,
            aa_dont_blend: 0,
            cx1: 0,
            cy1: 0,
            cx2: sx - 1,
            cy2: sy - 1,
            res_x: RESOLUTION,
            res_y: RESOLUTION,
            interpolation_id: interp::BILINEAR_FIXED,
            interpolation: None,
            special_depth: 0,
        }
    }

    /// gdImageCreate()
    pub fn create(sx: i32, sy: i32) -> Option<Image> {
        if overflow2(sx, sy) {
            return None;
        }
        if overflow2(8, sy) {
            return None;
        }
        if overflow2(1, sx) {
            return None;
        }
        let mut im = Image::blank(sx, sy);
        im.pixels = vec![vec![0u8; sx as usize]; sy as usize];
        im.open = [1; MAX_COLORS];
        Some(im)
    }

    /// gdImageCreateTrueColor()
    pub fn create_true_color(sx: i32, sy: i32) -> Option<Image> {
        if overflow2(sx, sy) {
            return None;
        }
        if overflow2(8, sy) {
            return None;
        }
        if overflow2(4, sx) {
            return None;
        }
        let mut im = Image::blank(sx, sy);
        im.tpixels = vec![vec![0i32; sx as usize]; sy as usize];
        im.true_color = true;
        im.save_alpha_flag = 0;
        im.alpha_blending_flag = 1;
        Some(im)
    }

    /// gdImageBoundsSafe()
    #[inline]
    pub fn bounds_safe(&self, x: i32, y: i32) -> bool {
        !((y < self.cy1 || y > self.cy2) || (x < self.cx1 || x > self.cx2))
    }

    /// gdImageRed() & co.
    #[inline]
    pub fn red_of(&self, c: i32) -> i32 {
        if self.true_color {
            true_color_get_red(c)
        } else {
            self.red[(c as usize) & 0xff]
        }
    }
    #[inline]
    pub fn green_of(&self, c: i32) -> i32 {
        if self.true_color {
            true_color_get_green(c)
        } else {
            self.green[(c as usize) & 0xff]
        }
    }
    #[inline]
    pub fn blue_of(&self, c: i32) -> i32 {
        if self.true_color {
            true_color_get_blue(c)
        } else {
            self.blue[(c as usize) & 0xff]
        }
    }
    #[inline]
    pub fn alpha_of(&self, c: i32) -> i32 {
        if self.true_color {
            true_color_get_alpha(c)
        } else {
            self.alpha[(c as usize) & 0xff]
        }
    }

    /// gdImageColorClosest()
    pub fn color_closest(&self, r: i32, g: i32, b: i32) -> i32 {
        self.color_closest_alpha(r, g, b, ALPHA_OPAQUE)
    }

    /// gdImageColorClosestAlpha()
    pub fn color_closest_alpha(&self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.true_color {
            return true_color_alpha(r, g, b, a);
        }
        let mut ct = -1;
        let mut first = true;
        let mut mindist: i64 = 0;
        for i in 0..self.colors_total as usize {
            if self.open[i] != 0 {
                continue;
            }
            let rd = (self.red[i] - r) as i64;
            let gd = (self.green[i] - g) as i64;
            let bd = (self.blue[i] - b) as i64;
            let ad = (self.alpha[i] - a) as i64;
            let dist = rd * rd + gd * gd + bd * bd + ad * ad;
            if first || dist < mindist {
                mindist = dist;
                ct = i as i32;
                first = false;
            }
        }
        ct
    }

    /// gdImageColorClosestHWB()
    pub fn color_closest_hwb(&self, r: i32, g: i32, b: i32) -> i32 {
        if self.true_color {
            return true_color(r, g, b);
        }
        let mut ct = -1;
        let mut first = true;
        let mut mindist = 0f32;
        for i in 0..self.colors_total as usize {
            if self.open[i] != 0 {
                continue;
            }
            let dist = hwb_diff(self.red[i], self.green[i], self.blue[i], r, g, b);
            if first || dist < mindist {
                mindist = dist;
                ct = i as i32;
                first = false;
            }
        }
        ct
    }

    /// gdImageColorExact()
    pub fn color_exact(&self, r: i32, g: i32, b: i32) -> i32 {
        self.color_exact_alpha(r, g, b, ALPHA_OPAQUE)
    }

    /// gdImageColorExactAlpha()
    pub fn color_exact_alpha(&self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.true_color {
            return true_color_alpha(r, g, b, a);
        }
        for i in 0..self.colors_total as usize {
            if self.open[i] != 0 {
                continue;
            }
            if self.red[i] == r && self.green[i] == g && self.blue[i] == b && self.alpha[i] == a {
                return i as i32;
            }
        }
        -1
    }

    /// gdImageColorAllocate()
    pub fn color_allocate(&mut self, r: i32, g: i32, b: i32) -> i32 {
        self.color_allocate_alpha(r, g, b, ALPHA_OPAQUE)
    }

    /// gdImageColorAllocateAlpha()
    pub fn color_allocate_alpha(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.true_color {
            return true_color_alpha(r, g, b, a);
        }
        let mut ct = -1;
        for i in 0..self.colors_total {
            if self.open[i as usize] != 0 {
                ct = i;
                break;
            }
        }
        if ct == -1 {
            ct = self.colors_total;
            if ct == MAX_COLORS as i32 {
                return -1;
            }
            self.colors_total += 1;
        }
        let c = ct as usize;
        self.red[c] = r;
        self.green[c] = g;
        self.blue[c] = b;
        self.alpha[c] = a;
        self.open[c] = 0;
        ct
    }

    /// gdImageColorResolve()
    pub fn color_resolve(&mut self, r: i32, g: i32, b: i32) -> i32 {
        self.color_resolve_alpha(r, g, b, ALPHA_OPAQUE)
    }

    /// gdImageColorResolveAlpha()
    pub fn color_resolve_alpha(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        if self.true_color {
            return true_color_alpha(r, g, b, a);
        }
        let mut ct = -1;
        let mut op = -1;
        let mut mindist: i64 = 4 * 255 * 255;
        for c in 0..self.colors_total {
            let ci = c as usize;
            if self.open[ci] != 0 {
                op = c;
                continue;
            }
            if c == self.transparent {
                continue;
            }
            let rd = (self.red[ci] - r) as i64;
            let gd = (self.green[ci] - g) as i64;
            let bd = (self.blue[ci] - b) as i64;
            let ad = (self.alpha[ci] - a) as i64;
            let dist = rd * rd + gd * gd + bd * bd + ad * ad;
            if dist < mindist {
                if dist == 0 {
                    return c;
                }
                mindist = dist;
                ct = c;
            }
        }
        if op == -1 {
            op = self.colors_total;
            if op == MAX_COLORS as i32 {
                return ct;
            }
            self.colors_total += 1;
        }
        let o = op as usize;
        self.red[o] = r;
        self.green[o] = g;
        self.blue[o] = b;
        self.alpha[o] = a;
        self.open[o] = 0;
        op
    }

    /// gdImageColorDeallocate()
    pub fn color_deallocate(&mut self, color: i32) {
        if self.true_color {
            return;
        }
        self.open[(color as usize) & 0xff] = 1;
    }

    /// gdImageColorTransparent()
    pub fn color_transparent(&mut self, color: i32) {
        if color < 0 {
            return;
        }
        if !self.true_color {
            if color >= self.colors_total {
                return;
            }
            if self.transparent != -1 {
                self.alpha[self.transparent as usize] = ALPHA_OPAQUE;
            }
            self.alpha[color as usize] = ALPHA_TRANSPARENT;
        }
        self.transparent = color;
    }

    /// gdImagePaletteCopy()
    pub fn palette_copy(to: &mut Image, from: &Image) {
        if to.true_color || from.true_color {
            return;
        }
        let mut xlate = [-1i32; 256];
        for y in 0..to.sy {
            for x in 0..to.sx {
                let p = to.get_pixel(x, y) as usize;
                if xlate[p] == -1 {
                    xlate[p] =
                        from.color_closest_alpha(to.red[p], to.green[p], to.blue[p], to.alpha[p]);
                }
                to.set_pixel(x, y, xlate[p]);
            }
        }
        for i in 0..from.colors_total as usize {
            to.red[i] = from.red[i];
            to.blue[i] = from.blue[i];
            to.green[i] = from.green[i];
            to.alpha[i] = from.alpha[i];
            to.open[i] = 0;
        }
        for i in from.colors_total..to.colors_total {
            to.open[i as usize] = 1;
        }
        to.colors_total = from.colors_total;
    }

    /// gdImageSetPixel()
    pub fn set_pixel(&mut self, x: i32, y: i32, color: i32) {
        match color {
            STYLED | STYLED_BRUSHED | ANTI_ALIASED => {
                if self.special_depth > 64 {
                    return;
                }
                self.special_depth += 1;
                self.set_pixel_special(x, y, color);
                self.special_depth -= 1;
            }
            BRUSHED => self.brush_apply(x, y),
            TILED => self.tile_apply(x, y),
            _ => {
                if self.bounds_safe(x, y) {
                    let (xu, yu) = (x as usize, y as usize);
                    if self.true_color {
                        let d = self.tpixels[yu][xu];
                        self.tpixels[yu][xu] = match self.alpha_blending_flag {
                            EFFECT_ALPHA_BLEND | EFFECT_NORMAL => alpha_blend(d, color),
                            EFFECT_OVERLAY => layer_overlay(d, color),
                            EFFECT_MULTIPLY => layer_multiply(d, color),
                            _ => color,
                        };
                    } else {
                        self.pixels[yu][xu] = color as u8;
                    }
                }
            }
        }
    }

    /// The gdStyled / gdStyledBrushed / gdAntiAliased cases of
    /// gdImageSetPixel()
    fn set_pixel_special(&mut self, x: i32, y: i32, color: i32) {
        if color == ANTI_ALIASED {
            let c = self.aa_color;
            self.set_pixel(x, y, c);
            return;
        }
        let Some(style) = &self.style else {
            return;
        };
        if style.is_empty() {
            return;
        }
        let len = style.len() as i32;
        // (a nested styled pixel can leave stylePos at the length: C then
        // reads past the style array)
        let p = style[self.style_pos.rem_euclid(len) as usize];
        self.style_pos += 1;
        if color == STYLED {
            if p != TRANSPARENT {
                self.set_pixel(x, y, p);
            }
        } else if p != TRANSPARENT && p != 0 {
            self.set_pixel(x, y, BRUSHED);
        }
        self.style_pos %= len;
    }

    /// gdImageGetPixel()
    #[inline]
    pub fn get_pixel(&self, x: i32, y: i32) -> i32 {
        if self.bounds_safe(x, y) {
            if self.true_color {
                self.tpixels[y as usize][x as usize]
            } else {
                self.pixels[y as usize][x as usize] as i32
            }
        } else {
            0
        }
    }

    /// gdImageGetTrueColorPixel()
    pub fn get_true_color_pixel(&self, x: i32, y: i32) -> i32 {
        let p = self.get_pixel(x, y);
        if !self.true_color {
            let pi = p as usize;
            true_color_alpha(
                self.red[pi],
                self.green[pi],
                self.blue[pi],
                if self.transparent == p {
                    ALPHA_TRANSPARENT
                } else {
                    self.alpha[pi]
                },
            )
        } else {
            p
        }
    }

    fn brush_apply(&mut self, x: i32, y: i32) {
        let Some(brush_rc) = self.brush.clone() else {
            return;
        };
        let Ok(brush) = brush_rc.try_borrow() else {
            return;
        };
        let hy = brush.sy / 2;
        let y1 = y - hy;
        let y2 = y1 + brush.sy;
        let hx = brush.sx / 2;
        let x1 = x - hx;
        let x2 = x1 + brush.sx;
        let mut srcy = 0;
        if self.true_color {
            if brush.true_color {
                for ly in y1..y2 {
                    let mut srcx = 0;
                    for lx in x1..x2 {
                        let p = brush.get_true_color_pixel(srcx, srcy);
                        if p != brush.transparent {
                            self.set_pixel(lx, ly, p);
                        }
                        srcx += 1;
                    }
                    srcy += 1;
                }
            } else {
                for ly in y1..y2 {
                    let mut srcx = 0;
                    for lx in x1..x2 {
                        let p = brush.get_pixel(srcx, srcy);
                        let tc = brush.get_true_color_pixel(srcx, srcy);
                        if p != brush.transparent {
                            self.set_pixel(lx, ly, tc);
                        }
                        srcx += 1;
                    }
                    srcy += 1;
                }
            }
        } else {
            for ly in y1..y2 {
                let mut srcx = 0;
                for lx in x1..x2 {
                    let p = brush.get_pixel(srcx, srcy);
                    if p != brush.transparent {
                        if brush.true_color {
                            let c = self.color_resolve_alpha(
                                true_color_get_red(p),
                                true_color_get_green(p),
                                true_color_get_blue(p),
                                true_color_get_alpha(p),
                            );
                            self.set_pixel(lx, ly, c);
                        } else {
                            let c = self.brush_color_map[(p as usize) & 0xff];
                            self.set_pixel(lx, ly, c);
                        }
                    }
                    srcx += 1;
                }
                srcy += 1;
            }
        }
    }

    fn tile_apply(&mut self, x: i32, y: i32) {
        let Some(tile_rc) = self.tile.clone() else {
            return;
        };
        let Ok(tile) = tile_rc.try_borrow() else {
            return;
        };
        if tile.sx == 0 || tile.sy == 0 {
            return;
        }
        let srcx = x % tile.sx;
        let srcy = y % tile.sy;
        let mut p = tile.get_pixel(srcx, srcy);
        if self.true_color {
            if p != tile.transparent {
                if !tile.true_color {
                    let pi = (p as usize) & 0xff;
                    p = true_color_alpha(
                        tile.red[pi],
                        tile.green[pi],
                        tile.blue[pi],
                        tile.alpha[pi],
                    );
                }
                self.set_pixel(x, y, p);
            }
        } else if p != tile.transparent {
            if tile.true_color {
                let c = self.color_resolve_alpha(
                    true_color_get_red(p),
                    true_color_get_green(p),
                    true_color_get_blue(p),
                    true_color_get_alpha(p),
                );
                self.set_pixel(x, y, c);
            } else {
                let c = self.tile_color_map[(p as usize) & 0xff];
                self.set_pixel(x, y, c);
            }
        }
    }

    /// gdImageTileGet()
    fn tile_get(&mut self, x: i32, y: i32) -> i32 {
        let Some(tile_rc) = self.tile.clone() else {
            return -1;
        };
        let Ok(tile) = tile_rc.try_borrow() else {
            return -1;
        };
        if tile.sx == 0 || tile.sy == 0 {
            return -1;
        }
        let srcx = x % tile.sx;
        let srcy = y % tile.sy;
        let p = tile.get_pixel(srcx, srcy);
        if p == tile.transparent {
            self.transparent
        } else if self.true_color {
            if tile.true_color {
                p
            } else {
                true_color_alpha(
                    tile.red_of(p),
                    tile.green_of(p),
                    tile.blue_of(p),
                    tile.alpha_of(p),
                )
            }
        } else if tile.true_color {
            self.color_resolve_alpha(
                true_color_get_red(p),
                true_color_get_green(p),
                true_color_get_blue(p),
                true_color_get_alpha(p),
            )
        } else {
            self.color_resolve_alpha(
                tile.red_of(p),
                tile.green_of(p),
                tile.blue_of(p),
                tile.alpha_of(p),
            )
        }
    }

    /// gdImageClone()
    pub fn clone_image(&self) -> Option<Image> {
        let mut dst = if self.true_color {
            Image::create_true_color(self.sx, self.sy)?
        } else {
            Image::create(self.sx, self.sy)?
        };
        if !self.true_color {
            dst.colors_total = self.colors_total;
            dst.red = self.red;
            dst.green = self.green;
            dst.blue = self.blue;
            dst.alpha = self.alpha;
            dst.open = self.open;
            dst.pixels = self.pixels.clone();
        } else {
            dst.tpixels = self.tpixels.clone();
        }
        dst.interlace = self.interlace;
        dst.alpha_blending_flag = self.alpha_blending_flag;
        dst.save_alpha_flag = self.save_alpha_flag;
        dst.aa = self.aa;
        dst.aa_color = self.aa_color;
        dst.aa_dont_blend = self.aa_dont_blend;
        dst.cx1 = self.cx1;
        dst.cy1 = self.cy1;
        dst.cx2 = self.cx2;
        dst.cy2 = self.cy2;
        dst.res_x = self.res_x;
        dst.res_y = self.res_y;
        dst.interpolation_id = self.interpolation_id;
        dst.interpolation = self.interpolation;
        if let Some(b) = &self.brush {
            if let Ok(b) = b.try_borrow() {
                dst.brush = b.clone_image().map(|i| Rc::new(RefCell::new(i)));
            }
        }
        if let Some(t) = &self.tile {
            if let Ok(t) = t.try_borrow() {
                dst.tile = t.clone_image().map(|i| Rc::new(RefCell::new(i)));
            }
        }
        if let Some(s) = &self.style {
            dst.style = Some(s.clone());
            dst.style_pos = self.style_pos;
        }
        dst.brush_color_map = self.brush_color_map;
        dst.tile_color_map = self.tile_color_map;
        dst.poly_ints = self.poly_ints.clone();
        Some(dst)
    }

    fn hline(&mut self, y: i32, mut x1: i32, mut x2: i32, col: i32) {
        if self.thick > 1 {
            let thickhalf = self.thick >> 1;
            self.filled_h_rectangle(x1, y - thickhalf, x2, y + self.thick - thickhalf - 1, col);
        } else {
            if x2 < x1 {
                std::mem::swap(&mut x1, &mut x2);
            }
            while x1 <= x2 {
                self.set_pixel(x1, y, col);
                x1 += 1;
            }
        }
    }

    fn vline(&mut self, x: i32, mut y1: i32, mut y2: i32, col: i32) {
        if self.thick > 1 {
            let thickhalf = self.thick >> 1;
            self.filled_rectangle(x - thickhalf, y1, x + self.thick - thickhalf - 1, y2, col);
        } else {
            if y2 < y1 {
                std::mem::swap(&mut y1, &mut y2);
            }
            while y1 <= y2 {
                self.set_pixel(x, y1, col);
                y1 += 1;
            }
        }
    }

    /// gdImageLine(): Bresenham as presented in Foley & Van Dam
    pub fn line(&mut self, mut x1: i32, mut y1: i32, mut x2: i32, mut y2: i32, color: i32) {
        let thick = self.thick;
        if color == ANTI_ALIASED {
            let c = self.aa_color;
            self.aa_line(x1, y1, x2, y2, c);
            return;
        }
        if !clip_1d(&mut x1, &mut y1, &mut x2, &mut y2, self.cx1, self.cx2) {
            return;
        }
        if !clip_1d(&mut y1, &mut x1, &mut y2, &mut x2, self.cy1, self.cy2) {
            return;
        }
        let dx = x2.wrapping_sub(x1).wrapping_abs();
        let dy = y2.wrapping_sub(y1).wrapping_abs();
        if dx == 0 {
            self.vline(x1, y1, y2, color);
            return;
        } else if dy == 0 {
            self.hline(y1, x1, x2, color);
            return;
        }
        let wid: i32;
        if dy <= dx {
            let ac = (dy as f64).atan2(dx as f64).cos();
            let mut w = if ac != 0.0 {
                (thick as f64 / ac) as i32
            } else {
                1
            };
            if w == 0 {
                w = 1;
            }
            wid = w;
            let mut d = 2 * dy - dx;
            let incr1 = 2 * dy;
            let incr2 = 2 * (dy - dx);
            let (mut x, mut y, ydirflag, xend);
            if x1 > x2 {
                x = x2;
                y = y2;
                ydirflag = -1;
                xend = x1;
            } else {
                x = x1;
                y = y1;
                ydirflag = 1;
                xend = x2;
            }
            let mut wstart = y - wid / 2;
            for w in wstart..wstart + wid {
                self.set_pixel(x, w, color);
            }
            let step = if (y2 - y1) * ydirflag > 0 { 1 } else { -1 };
            while x < xend {
                x += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    y += step;
                    d += incr2;
                }
                wstart = y - wid / 2;
                for w in wstart..wstart + wid {
                    self.set_pixel(x, w, color);
                }
            }
        } else {
            let as_ = (dy as f64).atan2(dx as f64).sin();
            let mut w = if as_ != 0.0 {
                (thick as f64 / as_) as i32
            } else {
                1
            };
            if w == 0 {
                w = 1;
            }
            wid = w;
            let mut d = 2 * dx - dy;
            let incr1 = 2 * dx;
            let incr2 = 2 * (dx - dy);
            let (mut x, mut y, yend, xdirflag);
            if y1 > y2 {
                y = y2;
                x = x2;
                yend = y1;
                xdirflag = -1;
            } else {
                y = y1;
                x = x1;
                yend = y2;
                xdirflag = 1;
            }
            let mut wstart = x - wid / 2;
            for w in wstart..wstart + wid {
                self.set_pixel(w, y, color);
            }
            let step = if (x2 - x1) * xdirflag > 0 { 1 } else { -1 };
            while y < yend {
                y += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    x += step;
                    d += incr2;
                }
                wstart = x - wid / 2;
                for w in wstart..wstart + wid {
                    self.set_pixel(w, y, color);
                }
            }
        }
    }

    fn set_aa_pixel_color(&mut self, x: i32, y: i32, color: i32, t: i32) {
        if x < 0 || y < 0 || x >= self.sx || y >= self.sy {
            return;
        }
        let mut dr = true_color_get_red(color);
        let mut dg = true_color_get_green(color);
        let mut db = true_color_get_blue(color);
        let p = self.get_pixel(x, y);
        let r = true_color_get_red(p);
        let g = true_color_get_green(p);
        let b = true_color_get_blue(p);
        let blend = |c: i32, cc: i32| -> i32 {
            cc + ((((c - cc) * t) + (((c - cc) * t) >> 8) + 0x80) >> 8)
        };
        dr = blend(r, dr);
        dg = blend(g, dg);
        db = blend(b, db);
        self.tpixels[y as usize][x as usize] = true_color_alpha(dr, dg, db, ALPHA_OPAQUE);
    }

    /// gdImageAALine()
    pub fn aa_line(&mut self, mut x1: i32, mut y1: i32, mut x2: i32, mut y2: i32, col: i32) {
        if !self.true_color {
            // (with gdAntiAliased as the AA color C recurses until the
            // stack overflows)
            if col != ANTI_ALIASED {
                self.line(x1, y1, x2, y2, col);
            }
            return;
        }
        if !clip_1d(&mut x1, &mut y1, &mut x2, &mut y2, self.cx1, self.cx2) {
            return;
        }
        if !clip_1d(&mut y1, &mut x1, &mut y2, &mut x2, self.cy1, self.cy2) {
            return;
        }
        let (mut x1, mut y1, mut x2, mut y2) = (x1 as i64, y1 as i64, x2 as i64, y2 as i64);
        let mut dx = x2 - x1;
        let mut dy = y2 - y1;
        if dx == 0 && dy == 0 {
            return;
        }
        if (dx as i32).wrapping_abs() > (dy as i32).wrapping_abs() {
            if dx < 0 {
                std::mem::swap(&mut x1, &mut x2);
                std::mem::swap(&mut y1, &mut y2);
                dx = x2 - x1;
                dy = y2 - y1;
            }
            let mut y = y1;
            let inc = (dy * 65536) / dx;
            let mut frac: i64 = 0;
            let mut x = x1;
            while x <= x2 {
                self.set_aa_pixel_color(x as i32, y as i32, col, ((frac >> 8) & 0xFF) as i32);
                if y + 1 < self.sy as i64 {
                    self.set_aa_pixel_color(
                        x as i32,
                        (y + 1) as i32,
                        col,
                        ((!frac >> 8) & 0xFF) as i32,
                    );
                }
                frac += inc;
                if frac >= 65536 {
                    frac -= 65536;
                    y += 1;
                } else if frac < 0 {
                    frac += 65536;
                    y -= 1;
                }
                x += 1;
            }
        } else {
            if dy < 0 {
                std::mem::swap(&mut x1, &mut x2);
                std::mem::swap(&mut y1, &mut y2);
                dx = x2 - x1;
                dy = y2 - y1;
            }
            let mut x = x1;
            let inc = (dx * 65536) / dy;
            let mut frac: i64 = 0;
            let mut y = y1;
            while y <= y2 {
                self.set_aa_pixel_color(x as i32, y as i32, col, ((frac >> 8) & 0xFF) as i32);
                if x + 1 < self.sx as i64 {
                    self.set_aa_pixel_color(
                        (x + 1) as i32,
                        y as i32,
                        col,
                        ((!frac >> 8) & 0xFF) as i32,
                    );
                }
                frac += inc;
                if frac >= 65536 {
                    frac -= 65536;
                    x += 1;
                } else if frac < 0 {
                    frac += 65536;
                    x -= 1;
                }
                y += 1;
            }
        }
    }

    /// gdImageDashedLine()
    pub fn dashed_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: i32) {
        let mut dash_step = 0;
        let mut on = true;
        let thick = self.thick;
        let dx = (x2 - x1).wrapping_abs();
        let dy = (y2 - y1).wrapping_abs();
        let as_ = (dy as f64).atan2(dx as f64).sin();
        let wid = if as_ != 0.0 {
            (thick as f64 / as_) as i32
        } else {
            1
        };
        if dy <= dx {
            let vert = true;
            let mut d = 2 * dy - dx;
            let incr1 = 2 * dy;
            let incr2 = 2 * (dy - dx);
            let (mut x, mut y, ydirflag, xend);
            if x1 > x2 {
                x = x2;
                y = y2;
                ydirflag = -1;
                xend = x1;
            } else {
                x = x1;
                y = y1;
                ydirflag = 1;
                xend = x2;
            }
            self.dashed_set(x, y, color, &mut on, &mut dash_step, wid, vert);
            let step = if (y2 - y1) * ydirflag > 0 { 1 } else { -1 };
            while x < xend {
                x += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    y += step;
                    d += incr2;
                }
                self.dashed_set(x, y, color, &mut on, &mut dash_step, wid, vert);
            }
        } else {
            let vert = false;
            let mut d = 2 * dx - dy;
            let incr1 = 2 * dx;
            let incr2 = 2 * (dx - dy);
            let (mut x, mut y, yend, xdirflag);
            if y1 > y2 {
                y = y2;
                x = x2;
                yend = y1;
                xdirflag = -1;
            } else {
                y = y1;
                x = x1;
                yend = y2;
                xdirflag = 1;
            }
            self.dashed_set(x, y, color, &mut on, &mut dash_step, wid, vert);
            let step = if (x2 - x1) * xdirflag > 0 { 1 } else { -1 };
            while y < yend {
                y += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    x += step;
                    d += incr2;
                }
                self.dashed_set(x, y, color, &mut on, &mut dash_step, wid, vert);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn dashed_set(
        &mut self,
        x: i32,
        y: i32,
        color: i32,
        on: &mut bool,
        dash_step: &mut i32,
        wid: i32,
        vert: bool,
    ) {
        *dash_step += 1;
        if *dash_step == DASH_SIZE {
            *dash_step = 0;
            *on = !*on;
        }
        if *on {
            if vert {
                let wstart = y - wid / 2;
                for w in wstart..wstart + wid {
                    self.set_pixel(x, w, color);
                }
            } else {
                let wstart = x - wid / 2;
                for w in wstart..wstart + wid {
                    self.set_pixel(w, y, color);
                }
            }
        }
    }

    /// gdImageChar()
    pub fn char(&mut self, f: &Font, x: i32, y: i32, c: i32, color: i32) {
        let xupper = if x > i32::MAX - f.w {
            i32::MAX
        } else {
            x + f.w
        };
        let yupper = if y > i32::MAX - f.h {
            i32::MAX
        } else {
            y + f.h
        };
        if c < f.offset || c >= f.offset + f.nchars {
            return;
        }
        let fline = ((c - f.offset) * f.h * f.w) as usize;
        let mut cy = 0usize;
        let mut py = y;
        while py < yupper {
            let mut cx = 0usize;
            let mut px = x;
            while px < xupper {
                if f.data
                    .get(fline + cy * f.w as usize + cx)
                    .copied()
                    .unwrap_or(0)
                    != 0
                {
                    self.set_pixel(px, py, color);
                }
                cx += 1;
                px += 1;
            }
            cy += 1;
            py += 1;
        }
    }

    /// gdImageCharUp()
    pub fn char_up(&mut self, f: &Font, x: i32, y: i32, c: i32, color: i32) {
        let xupper = if x > i32::MAX - f.h {
            i32::MAX
        } else {
            x + f.h
        };
        let ylower = if y < i32::MIN + f.w {
            i32::MIN
        } else {
            y - f.w
        };
        if c < f.offset || c >= f.offset + f.nchars {
            return;
        }
        let fline = ((c - f.offset) * f.h * f.w) as usize;
        let mut cx = 0usize;
        let mut py = y;
        while py > ylower {
            let mut cy = 0usize;
            let mut px = x;
            while px < xupper {
                if f.data
                    .get(fline + cy * f.w as usize + cx)
                    .copied()
                    .unwrap_or(0)
                    != 0
                {
                    self.set_pixel(px, py, color);
                }
                cy += 1;
                px += 1;
            }
            cx += 1;
            py -= 1;
        }
    }

    /// gdImageString(): stops at a NUL byte like strlen()
    pub fn string(&mut self, f: &Font, mut x: i32, y: i32, s: &[u8], color: i32) {
        for &c in s.iter().take_while(|&&c| c != 0) {
            self.char(f, x, y, c as i32, color);
            x = x.wrapping_add(f.w);
        }
    }

    /// gdImageStringUp()
    pub fn string_up(&mut self, f: &Font, x: i32, mut y: i32, s: &[u8], color: i32) {
        for &c in s.iter().take_while(|&&c| c != 0) {
            self.char_up(f, x, y, c as i32, color);
            y = y.wrapping_sub(f.w);
        }
    }

    /// gdImageArc()
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cx: i32, cy: i32, w: i32, h: i32, s: i32, e: i32, color: i32) {
        self.filled_arc(cx, cy, w, h, s, e, color, NO_FILL);
    }

    /// gdImageFilledArc()
    #[allow(clippy::too_many_arguments)]
    pub fn filled_arc(
        &mut self,
        cx: i32,
        cy: i32,
        w: i32,
        h: i32,
        mut s: i32,
        mut e: i32,
        color: i32,
        style: i32,
    ) {
        // gdPoint pts[363] (with room for the two insertions C may write past it)
        let mut pts = [Point::default(); 366];
        let (mut lx, mut ly) = (0, 0);
        let (mut fx, mut fy) = (0, 0);
        let (mut startx, mut starty, mut endx, mut endy) = (-1, -1, -1, -1);
        if s % 360 == e % 360 {
            s = 0;
            e = 360;
        } else {
            if s > 360 {
                s %= 360;
            }
            if e > 360 {
                e %= 360;
            }
            while s < 0 {
                s += 360;
            }
            while e < s {
                e += 360;
            }
            if s == e {
                s = 0;
                e = 360;
            }
        }
        let mut i = s;
        let mut pti: usize = 1;
        while i <= e {
            let x = ((COS_T[(i % 360) as usize] as i64 * w as i64 / (2 * 1024)) + cx as i64) as i32;
            let y = ((SIN_T[(i % 360) as usize] as i64 * h as i64 / (2 * 1024)) + cy as i64) as i32;
            endx = x;
            endy = y;
            if i != s {
                if style & CHORD == 0 {
                    if style & NO_FILL != 0 {
                        self.line(lx, ly, x, y, color);
                    } else if y == ly {
                        pti -= 1;
                        if ((i > 270 || i < 90) && x > lx) || ((i > 90 && i < 270) && x < lx) {
                            pts[pti].x = x;
                        }
                    } else {
                        pts[pti].x = x;
                        pts[pti].y = y;
                    }
                }
            } else {
                fx = x;
                fy = y;
                if style & (CHORD | NO_FILL) == 0 {
                    pts[0].x = cx;
                    pts[0].y = cy;
                    startx = x;
                    starty = y;
                    pts[pti].x = x;
                    pts[pti].y = y;
                }
            }
            lx = x;
            ly = y;
            i += 1;
            pti += 1;
        }
        if style & CHORD != 0 {
            if style & NO_FILL != 0 {
                if style & EDGED != 0 {
                    self.line(cx, cy, lx, ly, color);
                    self.line(cx, cy, fx, fy, color);
                }
                self.line(fx, fy, lx, ly, color);
            } else {
                pts[0] = Point { x: fx, y: fy };
                pts[1] = Point { x: lx, y: ly };
                pts[2] = Point { x: cx, y: cy };
                self.filled_polygon(&pts[..3], color);
            }
        } else if style & NO_FILL != 0 {
            if style & EDGED != 0 {
                self.line(cx, cy, lx, ly, color);
                self.line(cx, cy, fx, fy, color);
            }
        } else {
            if e - s < 360 {
                if pts[1].x != startx && pts[1].y == starty {
                    let mut i = pti;
                    while i > 1 {
                        pts[i] = pts[i - 1];
                        i -= 1;
                    }
                    pts[1] = Point {
                        x: startx,
                        y: starty,
                    };
                    pti += 1;
                }
                if pts[pti - 1].x != endx && pts[pti - 1].y == endy {
                    pts[pti] = Point { x: endx, y: endy };
                    pti += 1;
                }
            }
            pts[pti] = Point { x: cx, y: cy };
            self.filled_polygon(&pts[..pti + 1], color);
        }
    }

    /// gdImageEllipse()
    pub fn ellipse(&mut self, mx: i32, my: i32, w: i32, h: i32, c: i32) {
        let a = (w >> 1) as i64;
        let b = (h >> 1) as i64;
        self.set_pixel(mx.wrapping_add(a as i32), my, c);
        self.set_pixel(mx.wrapping_sub(a as i32), my, c);
        let mut mx1 = mx.wrapping_sub(a as i32);
        let mut my1 = my;
        let mut mx2 = mx.wrapping_add(a as i32);
        let mut my2 = my;
        let aq = a * a;
        let bq = b * b;
        let dx = aq << 1;
        let dy = bq << 1;
        let mut r = a * bq;
        let mut rx = r << 1;
        let mut ry: i64 = 0;
        let mut x = a as i32;
        while x > 0 {
            if r > 0 {
                my1 += 1;
                my2 -= 1;
                ry += dx;
                r -= ry;
            }
            if r <= 0 {
                x -= 1;
                mx1 += 1;
                mx2 -= 1;
                rx -= dy;
                r += rx;
            }
            self.set_pixel(mx1, my1, c);
            self.set_pixel(mx1, my2, c);
            self.set_pixel(mx2, my1, c);
            self.set_pixel(mx2, my2, c);
        }
    }

    /// gdImageFilledEllipse()
    pub fn filled_ellipse(&mut self, mx: i32, my: i32, w: i32, h: i32, c: i32) {
        let a = (w >> 1) as i64;
        let b = (h >> 1) as i64;
        let mut x = mx.wrapping_sub(a as i32);
        while x <= mx.wrapping_add(a as i32) {
            self.set_pixel(x, my, c);
            if x == i32::MAX {
                break;
            }
            x += 1;
        }
        let mut mx1 = mx.wrapping_sub(a as i32);
        let mut my1 = my;
        let mut mx2 = mx.wrapping_add(a as i32);
        let mut my2 = my;
        let aq = a * a;
        let bq = b * b;
        let dx = aq << 1;
        let dy = bq << 1;
        let mut r = a * bq;
        let mut rx = r << 1;
        let mut ry: i64 = 0;
        let mut x = a as i32;
        let mut old_y2 = -2;
        while x > 0 {
            if r > 0 {
                my1 += 1;
                my2 -= 1;
                ry += dx;
                r -= ry;
            }
            if r <= 0 {
                x -= 1;
                mx1 += 1;
                mx2 -= 1;
                rx -= dy;
                r += rx;
            }
            if old_y2 != my2 {
                for i in mx1..=mx2 {
                    self.set_pixel(i, my1, c);
                    self.set_pixel(i, my2, c);
                }
            }
            old_y2 = my2;
        }
    }

    /// gdImageFillToBorder()
    pub fn fill_to_border(&mut self, mut x: i32, mut y: i32, border: i32, color: i32) {
        if border < 0 || color < 0 {
            return;
        }
        if !self.true_color && (color > self.colors_total - 1 || border > self.colors_total - 1) {
            return;
        }
        let restore = self.alpha_blending_flag;
        self.alpha_blending_flag = 0;
        if x >= self.sx {
            x = self.sx - 1;
        } else if x < 0 {
            x = 0;
        }
        if y >= self.sy {
            y = self.sy - 1;
        } else if y < 0 {
            y = 0;
        }
        self.fill_to_border_rec(x, y, border, color);
        self.alpha_blending_flag = restore;
    }

    fn fill_to_border_rec(&mut self, x: i32, y: i32, border: i32, color: i32) {
        // An explicit stack instead of the C recursion; the visiting order
        // (and so the result) is the same: each call processes its span,
        // then its "above" children (in order, each fully), then "below".
        enum Task {
            Span(i32, i32),
            Scan {
                left: i32,
                right: i32,
                y: i32,
                row: i32,
                i: i32,
                last_border: bool,
                below: bool,
            },
        }
        let mut stack = vec![Task::Span(x, y)];
        // Each span of a terminating fill paints at least one pixel that is
        // never a seed again; more spans than pixels means the C recursion
        // would never end (e.g. seeds outside the clipping rectangle, where
        // nothing can be painted) and overflow its stack.
        let mut budget = (self.sx as i64) * (self.sy as i64) + 1;
        while let Some(t) = stack.pop() {
            match t {
                Task::Span(x, y) => {
                    budget -= 1;
                    if budget < 0 {
                        return;
                    }
                    let mut left_limit = -1;
                    let mut i = x;
                    while i >= 0 {
                        if self.get_pixel(i, y) == border {
                            break;
                        }
                        self.set_pixel(i, y, color);
                        left_limit = i;
                        i -= 1;
                    }
                    if left_limit == -1 {
                        continue;
                    }
                    let mut right_limit = x;
                    let mut i = x + 1;
                    while i < self.sx {
                        if self.get_pixel(i, y) == border {
                            break;
                        }
                        self.set_pixel(i, y, color);
                        right_limit = i;
                        i += 1;
                    }
                    // Below is scanned after above completes
                    if y < self.sy - 1 {
                        stack.push(Task::Scan {
                            left: left_limit,
                            right: right_limit,
                            y,
                            row: y + 1,
                            i: left_limit,
                            last_border: true,
                            below: true,
                        });
                    }
                    if y > 0 {
                        stack.push(Task::Scan {
                            left: left_limit,
                            right: right_limit,
                            y,
                            row: y - 1,
                            i: left_limit,
                            last_border: true,
                            below: false,
                        });
                    }
                }
                Task::Scan {
                    left,
                    right,
                    y,
                    row,
                    mut i,
                    mut last_border,
                    below,
                } => {
                    while i <= right {
                        let c = self.get_pixel(i, row);
                        if last_border {
                            if c != border && c != color {
                                // recurse, then continue this scan
                                stack.push(Task::Scan {
                                    left,
                                    right,
                                    y,
                                    row,
                                    i: i + 1,
                                    last_border: false,
                                    below,
                                });
                                stack.push(Task::Span(i, row));
                                break;
                            }
                        } else if c == border || c == color {
                            last_border = true;
                        }
                        i += 1;
                    }
                }
            }
        }
    }

    /// gdImageFill()
    pub fn fill(&mut self, mut x: i32, mut y: i32, nc: i32) {
        if !self.true_color && nc > self.colors_total - 1 {
            return;
        }
        let bak = self.alpha_blending_flag;
        self.alpha_blending_flag = 0;
        if nc == TILED {
            self.fill_tiled(x, y, nc);
            self.alpha_blending_flag = bak;
            return;
        }
        let wx2 = self.sx;
        let wy2 = self.sy;
        let oc = self.get_pixel(x, y);
        if oc == nc || x < 0 || x > wx2 || y < 0 || y > wy2 {
            self.alpha_blending_flag = bak;
            return;
        }
        if self.sx < 4 {
            let mut ix = x;
            let mut iy = y;
            'done: loop {
                loop {
                    let c = self.get_pixel(ix, iy);
                    if c != oc {
                        break 'done;
                    }
                    self.set_pixel(ix, iy, nc);
                    let cont = ix < self.sx - 1;
                    ix += 1;
                    if !cont {
                        break;
                    }
                }
                ix = x;
                let cont = iy < self.sy - 1;
                iy += 1;
                if !cont {
                    break;
                }
            }
            self.alpha_blending_flag = bak;
            return;
        }
        if overflow2(self.sy, self.sx) {
            return;
        }
        let fill_max = (self.sy * self.sx) / 4;
        let mut stack: Vec<(i32, i32, i32, i32)> = Vec::new();
        let push = |stack: &mut Vec<(i32, i32, i32, i32)>, yy: i32, xl: i32, xr: i32, dy: i32| {
            if (stack.len() as i32) < fill_max && yy + dy >= 0 && yy + dy < wy2 {
                stack.push((yy, xl, xr, dy));
            }
        };
        push(&mut stack, y, x, x, 1);
        push(&mut stack, y + 1, x, x, -1);
        // (special colors that do not change the pixel make the C loop spin
        // forever; bound the work to what any real fill can need)
        let mut budget = 64 * (self.sx as i64) * (self.sy as i64) + 1024;
        while let Some((sy_, xl, xr, sdy)) = stack.pop() {
            budget -= 1;
            if budget < 0 {
                break;
            }
            let dy = sdy;
            y = sy_ + dy;
            let x1 = xl;
            let x2 = xr;
            x = x1;
            while x >= 0 && self.get_pixel(x, y) == oc {
                self.set_pixel(x, y, nc);
                x -= 1;
            }
            let mut l;
            let mut skip = x >= x1;
            if !skip {
                l = x + 1;
                if l < x1 {
                    push(&mut stack, y, l, x1 - 1, -dy);
                }
                x = x1 + 1;
            } else {
                l = 0;
            }
            loop {
                if !skip {
                    while x <= wx2 && self.get_pixel(x, y) == oc {
                        self.set_pixel(x, y, nc);
                        x += 1;
                    }
                    push(&mut stack, y, l, x - 1, dy);
                    if x > x2 + 1 {
                        push(&mut stack, y, x2 + 1, x - 1, -dy);
                    }
                }
                skip = false;
                x += 1;
                while x <= x2 && self.get_pixel(x, y) != oc {
                    x += 1;
                }
                l = x;
                if x > x2 {
                    break;
                }
            }
        }
        self.alpha_blending_flag = bak;
    }

    fn fill_tiled(&mut self, x: i32, y: i32, _nc: i32) {
        if self.tile.is_none() {
            return;
        }
        let wx2 = self.sx;
        let wy2 = self.sy;
        let _ = self.tile_get(x, y);
        if overflow2(self.sy, self.sx) {
            return;
        }
        let fill_max = (self.sy * self.sx) / 4;
        let mut pts = vec![false; (self.sy as usize) * (self.sx as usize)];
        let mut stack: Vec<(i32, i32, i32, i32)> = Vec::new();
        let push = |stack: &mut Vec<(i32, i32, i32, i32)>, yy: i32, xl: i32, xr: i32, dy: i32| {
            if (stack.len() as i32) < fill_max && yy + dy >= 0 && yy + dy < wy2 {
                stack.push((yy, xl, xr, dy));
            }
        };
        let oc = self.get_pixel(x, y);
        let idx = |x: i32, y: i32| -> usize { (y + x * wy2) as usize };
        push(&mut stack, y, x, x, 1);
        push(&mut stack, y + 1, x, x, -1);
        let mut budget = 64 * (self.sx as i64) * (self.sy as i64) + 1024;
        while let Some((sy_, xl, xr, dy)) = stack.pop() {
            budget -= 1;
            if budget < 0 {
                break;
            }
            let y = sy_ + dy;
            let x1 = xl;
            let x2 = xr;
            let mut x = x1;
            while x >= 0
                && (!pts.get(idx(x, y)).copied().unwrap_or(true) && self.get_pixel(x, y) == oc)
            {
                let nc = self.tile_get(x, y);
                pts[idx(x, y)] = true;
                self.set_pixel(x, y, nc);
                x -= 1;
            }
            let mut l;
            let mut skip = x >= x1;
            if !skip {
                l = x + 1;
                if l < x1 {
                    push(&mut stack, y, l, x1 - 1, -dy);
                }
                x = x1 + 1;
            } else {
                l = 0;
            }
            loop {
                if !skip {
                    while x < wx2
                        && (!pts.get(idx(x, y)).copied().unwrap_or(true)
                            && self.get_pixel(x, y) == oc)
                    {
                        let nc = self.tile_get(x, y);
                        pts[idx(x, y)] = true;
                        self.set_pixel(x, y, nc);
                        x += 1;
                    }
                    push(&mut stack, y, l, x - 1, dy);
                    if x > x2 + 1 {
                        push(&mut stack, y, x2 + 1, x - 1, -dy);
                    }
                }
                skip = false;
                x += 1;
                while x <= x2
                    && (pts.get(idx(x, y)).copied().unwrap_or(false) || self.get_pixel(x, y) != oc)
                {
                    x += 1;
                }
                l = x;
                if x > x2 {
                    break;
                }
            }
        }
    }

    /// gdImageRectangle()
    pub fn rectangle(&mut self, mut x1: i32, mut y1: i32, mut x2: i32, mut y2: i32, color: i32) {
        let thick = self.thick;
        if x1 == x2 && y1 == y2 && thick == 1 {
            self.set_pixel(x1, y1, color);
            return;
        }
        if y2 < y1 {
            std::mem::swap(&mut y1, &mut y2);
        }
        if x2 < x1 {
            std::mem::swap(&mut x1, &mut x2);
        }
        if thick > 1 {
            let half = thick >> 1;
            let x1ul = x1 - half;
            let y1ul = y1 - half;
            let x2lr = x2 + half;
            let y2lr = y2 + half;

            let mut cy = y1ul + thick;
            while {
                let c = cy > y1ul;
                cy -= 1;
                c
            } {
                let mut cx = x1ul - 1;
                while {
                    let c = cx < x2lr;
                    cx += 1;
                    c
                } {
                    self.set_pixel(cx, cy, color);
                }
            }

            let mut cy = y2lr - thick;
            while {
                let c = cy < y2lr;
                cy += 1;
                c
            } {
                let mut cx = x1ul - 1;
                while {
                    let c = cx < x2lr;
                    cx += 1;
                    c
                } {
                    self.set_pixel(cx, cy, color);
                }
            }

            let mut cy = y1ul + thick - 1;
            while {
                let c = cy < y2lr - thick;
                cy += 1;
                c
            } {
                let mut cx = x1ul - 1;
                while {
                    let c = cx < x1ul + thick;
                    cx += 1;
                    c
                } {
                    self.set_pixel(cx, cy, color);
                }
            }

            let mut cy = y1ul + thick - 1;
            while {
                let c = cy < y2lr - thick;
                cy += 1;
                c
            } {
                let mut cx = x2lr - thick - 1;
                while {
                    let c = cx < x2lr;
                    cx += 1;
                    c
                } {
                    self.set_pixel(cx, cy, color);
                }
            }
        } else if x1 == x2 || y1 == y2 {
            self.line(x1, y1, x2, y2, color);
        } else {
            self.line(x1, y1, x2, y1, color);
            self.line(x1, y2, x2, y2, color);
            self.line(x1, y1 + 1, x1, y2 - 1, color);
            self.line(x2, y1 + 1, x2, y2 - 1, color);
        }
    }

    fn clamp_rect(&self, x1: &mut i32, y1: &mut i32, x2: &mut i32, y2: &mut i32) {
        if *x1 > *x2 {
            std::mem::swap(x1, x2);
        }
        if *y1 > *y2 {
            std::mem::swap(y1, y2);
        }
        if *x1 < 0 {
            *x1 = 0;
        }
        if *x2 >= self.sx {
            *x2 = self.sx - 1;
        }
        if *y1 < 0 {
            *y1 = 0;
        }
        if *y2 >= self.sy {
            *y2 = self.sy - 1;
        }
    }

    fn filled_h_rectangle(
        &mut self,
        mut x1: i32,
        mut y1: i32,
        mut x2: i32,
        mut y2: i32,
        color: i32,
    ) {
        if x1 == x2 && y1 == y2 {
            self.set_pixel(x1, y1, color);
            return;
        }
        self.clamp_rect(&mut x1, &mut y1, &mut x2, &mut y2);
        for x in x1..=x2 {
            for y in y1..=y2 {
                self.set_pixel(x, y, color);
            }
        }
    }

    fn filled_v_rectangle(
        &mut self,
        mut x1: i32,
        mut y1: i32,
        mut x2: i32,
        mut y2: i32,
        color: i32,
    ) {
        if x1 == x2 && y1 == y2 {
            self.set_pixel(x1, y1, color);
            return;
        }
        self.clamp_rect(&mut x1, &mut y1, &mut x2, &mut y2);
        for y in y1..=y2 {
            for x in x1..=x2 {
                self.set_pixel(x, y, color);
            }
        }
    }

    /// gdImageFilledRectangle()
    pub fn filled_rectangle(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: i32) {
        self.filled_v_rectangle(x1, y1, x2, y2, color);
    }

    /// gdImagePolygon()
    pub fn polygon(&mut self, p: &[Point], c: i32) {
        if p.is_empty() {
            return;
        }
        let n = p.len();
        self.line(p[0].x, p[0].y, p[n - 1].x, p[n - 1].y, c);
        self.open_polygon(p, c);
    }

    /// gdImageOpenPolygon()
    pub fn open_polygon(&mut self, p: &[Point], c: i32) {
        if p.is_empty() {
            return;
        }
        let mut lx = p[0].x;
        let mut ly = p[0].y;
        for q in &p[1..] {
            self.line(lx, ly, q.x, q.y, c);
            lx = q.x;
            ly = q.y;
        }
    }

    /// gdImageFilledPolygon()
    pub fn filled_polygon(&mut self, p: &[Point], c: i32) {
        let n = p.len();
        if n == 0 {
            return;
        }
        if overflow2(4, n as i32) {
            return;
        }
        let fill_color = if c == ANTI_ALIASED { self.aa_color } else { c };
        let mut ints_buf = std::mem::take(&mut self.poly_ints);
        if ints_buf.len() < n {
            ints_buf.resize(n, 0);
        }
        let mut miny = p[0].y;
        let mut maxy = p[0].y;
        for q in &p[1..] {
            if q.y < miny {
                miny = q.y;
            }
            if q.y > maxy {
                maxy = q.y;
            }
        }
        if n > 1 && miny == maxy {
            let mut x1 = p[0].x;
            let mut x2 = p[0].x;
            for q in &p[1..] {
                if q.x < x1 {
                    x1 = q.x;
                } else if q.x > x2 {
                    x2 = q.x;
                }
            }
            self.poly_ints = ints_buf;
            self.line(x1, miny, x2, miny, c);
            return;
        }
        let pmaxy = maxy;
        if miny < self.cy1 {
            miny = self.cy1;
        }
        if maxy > self.cy2 {
            maxy = self.cy2;
        }
        let mut y = miny;
        while y <= maxy {
            let mut ints = 0usize;
            for i in 0..n {
                let (ind1, ind2) = if i == 0 { (n - 1, 0) } else { (i - 1, i) };
                let mut y1 = p[ind1].y;
                let mut y2 = p[ind2].y;
                let (x1, x2);
                if y1 < y2 {
                    x1 = p[ind1].x;
                    x2 = p[ind2].x;
                } else if y1 > y2 {
                    y2 = p[ind1].y;
                    y1 = p[ind2].y;
                    x2 = p[ind1].x;
                    x1 = p[ind2].x;
                } else {
                    continue;
                }
                if y >= y1 && y < y2 {
                    let v = ((y - y1).wrapping_mul(x2 - x1)) as f32 / (y2 - y1) as f32;
                    ints_buf[ints] = (v as f64 + 0.5 + x1 as f64) as i32;
                    ints += 1;
                } else if y == pmaxy && y == y2 {
                    ints_buf[ints] = x2;
                    ints += 1;
                }
            }
            for i in 1..ints {
                let index = ints_buf[i];
                let mut j = i;
                while j > 0 && ints_buf[j - 1] > index {
                    ints_buf[j] = ints_buf[j - 1];
                    j -= 1;
                }
                ints_buf[j] = index;
            }
            let mut i = 0;
            while i + 1 < ints {
                self.line(ints_buf[i], y, ints_buf[i + 1], y, fill_color);
                i += 2;
            }
            if y == i32::MAX {
                break;
            }
            y += 1;
        }
        self.poly_ints = ints_buf;
        if c == ANTI_ALIASED {
            self.polygon(p, c);
        }
    }

    /// gdImageSetStyle()
    pub fn set_style(&mut self, style: &[i32]) {
        self.style = Some(style.to_vec());
        self.style_pos = 0;
    }

    /// gdImageSetThickness()
    pub fn set_thickness(&mut self, thickness: i32) {
        self.thick = thickness;
    }

    /// gdImageSetBrush()
    pub fn set_brush(&mut self, brush: Rc<RefCell<Image>>) {
        {
            if let Ok(b) = brush.try_borrow() {
                if !self.true_color && !b.true_color {
                    for i in 0..b.colors_total {
                        let idx = self.color_resolve_alpha(
                            b.red_of(i),
                            b.green_of(i),
                            b.blue_of(i),
                            b.alpha_of(i),
                        );
                        self.brush_color_map[i as usize] = idx;
                    }
                }
            }
        }
        self.brush = Some(brush);
    }

    /// gdImageSetTile()
    pub fn set_tile(&mut self, tile: Rc<RefCell<Image>>) {
        {
            if let Ok(t) = tile.try_borrow() {
                if !self.true_color && !t.true_color {
                    for i in 0..t.colors_total {
                        let idx = self.color_resolve_alpha(
                            t.red_of(i),
                            t.green_of(i),
                            t.blue_of(i),
                            t.alpha_of(i),
                        );
                        self.tile_color_map[i as usize] = idx;
                    }
                }
            }
        }
        self.tile = Some(tile);
    }

    /// gdImageSetAntiAliased()
    pub fn set_anti_aliased(&mut self, c: i32) {
        self.aa = 1;
        self.aa_color = c;
        self.aa_dont_blend = -1;
    }

    /// gdImageSetAntiAliasedDontBlend()
    pub fn set_anti_aliased_dont_blend(&mut self, c: i32, dont_blend: i32) {
        self.aa = 1;
        self.aa_color = c;
        self.aa_dont_blend = dont_blend;
    }

    /// gdImageInterlace()
    pub fn set_interlace(&mut self, i: i32) {
        self.interlace = i;
    }

    /// gdImageAlphaBlending()
    pub fn set_alpha_blending(&mut self, a: i32) {
        self.alpha_blending_flag = a;
    }

    /// gdImageSaveAlpha()
    pub fn set_save_alpha(&mut self, a: i32) {
        self.save_alpha_flag = a;
    }

    /// gdImageSetClip()
    pub fn set_clip(&mut self, mut x1: i32, mut y1: i32, mut x2: i32, mut y2: i32) {
        if x1 < 0 {
            x1 = 0;
        }
        if x1 >= self.sx {
            x1 = self.sx - 1;
        }
        if x2 < 0 {
            x2 = 0;
        }
        if x2 >= self.sx {
            x2 = self.sx - 1;
        }
        if y1 < 0 {
            y1 = 0;
        }
        if y1 >= self.sy {
            y1 = self.sy - 1;
        }
        if y2 < 0 {
            y2 = 0;
        }
        if y2 >= self.sy {
            y2 = self.sy - 1;
        }
        self.cx1 = x1;
        self.cy1 = y1;
        self.cx2 = x2;
        self.cy2 = y2;
    }

    /// gdImageGetClip()
    pub fn get_clip(&self) -> (i32, i32, i32, i32) {
        (self.cx1, self.cy1, self.cx2, self.cy2)
    }

    /// gdImageSetResolution()
    pub fn set_resolution(&mut self, res_x: u32, res_y: u32) {
        if res_x > 0 {
            self.res_x = res_x;
        }
        if res_y > 0 {
            self.res_y = res_y;
        }
    }

    /// gdImagePaletteToTrueColor()
    pub fn palette_to_true_color(&mut self) -> bool {
        if self.true_color {
            return true;
        }
        let sy = self.sy as usize;
        let sx = self.sx as usize;
        let mut tp = Vec::with_capacity(sy);
        for y in 0..sy {
            let src_row = &self.pixels[y];
            let mut row = Vec::with_capacity(sx);
            for &c in src_row.iter().take(sx) {
                let ci = c as usize;
                if c as i32 == self.transparent {
                    row.push(true_color_alpha(0, 0, 0, 127));
                } else {
                    row.push(true_color_alpha(
                        self.red[ci],
                        self.green[ci],
                        self.blue[ci],
                        self.alpha[ci],
                    ));
                }
            }
            tp.push(row);
        }
        self.tpixels = tp;
        self.pixels = Vec::new();
        self.true_color = true;
        self.alpha_blending_flag = 0;
        self.save_alpha_flag = 1;
        if self.transparent >= 0 {
            let c = (self.transparent as usize) & 0xff;
            self.transparent =
                true_color_alpha(self.red[c], self.green[c], self.blue[c], self.alpha[c]);
        }
        true
    }

    /// gdImageCompare()
    pub fn compare(im1: &Image, im2: &Image) -> i32 {
        let mut cmp = 0;
        if im1.interlace != im2.interlace {
            cmp |= GD_CMP_INTERLACE;
        }
        if im1.transparent != im2.transparent {
            cmp |= GD_CMP_TRANSPARENT;
        }
        if im1.true_color != im2.true_color {
            cmp |= GD_CMP_TRUECOLOR;
        }
        let mut sx = im1.sx;
        if im1.sx != im2.sx {
            cmp |= GD_CMP_SIZE_X + GD_CMP_IMAGE;
            if im2.sx < im1.sx {
                sx = im2.sx;
            }
        }
        let mut sy = im1.sy;
        if im1.sy != im2.sy {
            cmp |= GD_CMP_SIZE_Y + GD_CMP_IMAGE;
            if im2.sy < im1.sy {
                sy = im2.sy;
            }
        }
        if im1.colors_total != im2.colors_total {
            cmp |= GD_CMP_NUM_COLORS;
        }
        'outer: for y in 0..sy as usize {
            for x in 0..sx as usize {
                let p1 = if im1.true_color {
                    im1.tpixels[y][x]
                } else {
                    im1.pixels[y][x] as i32
                };
                let p2 = if im2.true_color {
                    im2.tpixels[y][x]
                } else {
                    im2.pixels[y][x] as i32
                };
                if im1.red_of(p1) != im2.red_of(p2)
                    || im1.green_of(p1) != im2.green_of(p2)
                    || im1.blue_of(p1) != im2.blue_of(p2)
                {
                    cmp |= GD_CMP_COLOR + GD_CMP_IMAGE;
                    break 'outer;
                }
            }
        }
        cmp
    }
}

/// gdImageCopy()
#[allow(clippy::too_many_arguments)]
pub fn copy(
    dst: &mut Image,
    src: &Image,
    same: bool,
    dst_x: i32,
    dst_y: i32,
    src_x: i32,
    src_y: i32,
    w: i32,
    h: i32,
) {
    if dst.true_color {
        if src.true_color {
            for y in 0..h {
                for x in 0..w {
                    let c = src.get_true_color_pixel(src_x + x, src_y + y);
                    if c != src.transparent {
                        dst.set_pixel(dst_x + x, dst_y + y, c);
                    }
                }
            }
        } else {
            for y in 0..h {
                for x in 0..w {
                    let c = src.get_pixel(src_x + x, src_y + y);
                    if c != src.transparent {
                        let ci = c as usize;
                        dst.set_pixel(
                            dst_x + x,
                            dst_y + y,
                            true_color_alpha(
                                src.red[ci],
                                src.green[ci],
                                src.blue[ci],
                                src.alpha[ci],
                            ),
                        );
                    }
                }
            }
        }
        return;
    }
    let mut color_map = [-1i32; MAX_COLORS];
    let mut toy = dst_y;
    for y in src_y..src_y.wrapping_add(h) {
        let mut tox = dst_x;
        for x in src_x..src_x.wrapping_add(w) {
            let c = src.get_pixel(x, y);
            if src.transparent == c {
                tox += 1;
                continue;
            }
            let map_to = if src.true_color {
                dst.color_resolve_alpha(
                    true_color_get_red(c),
                    true_color_get_green(c),
                    true_color_get_blue(c),
                    true_color_get_alpha(c),
                )
            } else if color_map[c as usize] == -1 {
                let ci = c as usize;
                let nc = if same {
                    c
                } else {
                    dst.color_resolve_alpha(src.red[ci], src.green[ci], src.blue[ci], src.alpha[ci])
                };
                color_map[ci] = nc;
                nc
            } else {
                color_map[c as usize]
            };
            dst.set_pixel(tox, toy, map_to);
            tox += 1;
        }
        toy += 1;
    }
}

/// gdImageCopyMerge()
#[allow(clippy::too_many_arguments)]
pub fn copy_merge(
    dst: &mut Image,
    src: &Image,
    same: bool,
    dst_x: i32,
    dst_y: i32,
    src_x: i32,
    src_y: i32,
    w: i32,
    h: i32,
    pct: i32,
) {
    let mut toy = dst_y;
    for y in src_y..src_y.wrapping_add(h) {
        let mut tox = dst_x;
        for x in src_x..src_x.wrapping_add(w) {
            let c = src.get_pixel(x, y);
            if src.transparent == c {
                tox += 1;
                continue;
            }
            let nc = if same {
                c
            } else {
                let dc = dst.get_pixel(tox, toy);
                let p = pct as f64 / 100.0;
                let q = (100 - pct) as f64 / 100.0;
                let ncr = (src.red_of(c) as f64 * p + dst.red_of(dc) as f64 * q) as i32;
                let ncg = (src.green_of(c) as f64 * p + dst.green_of(dc) as f64 * q) as i32;
                let ncb = (src.blue_of(c) as f64 * p + dst.blue_of(dc) as f64 * q) as i32;
                dst.color_resolve(ncr, ncg, ncb)
            };
            dst.set_pixel(tox, toy, nc);
            tox += 1;
        }
        toy += 1;
    }
}

/// gdImageCopyMergeGray()
#[allow(clippy::too_many_arguments)]
pub fn copy_merge_gray(
    dst: &mut Image,
    src: &Image,
    same: bool,
    dst_x: i32,
    dst_y: i32,
    src_x: i32,
    src_y: i32,
    w: i32,
    h: i32,
    pct: i32,
) {
    let mut toy = dst_y;
    for y in src_y..src_y.wrapping_add(h) {
        let mut tox = dst_x;
        for x in src_x..src_x.wrapping_add(w) {
            let c = src.get_pixel(x, y);
            if src.transparent == c {
                tox += 1;
                continue;
            }
            let nc = if same && pct == 100 {
                c
            } else {
                let dc = dst.get_pixel(tox, toy);
                let g: f32 = (0.29900f32 * dst.red_of(dc) as f32)
                    + (0.58700f32 * dst.green_of(dc) as f32)
                    + (0.11400f32 * dst.blue_of(dc) as f32);
                let p = pct as f32 / 100.0f32;
                let q = (100 - pct) as f64 / 100.0;
                let ncr = ((src.red_of(c) as f32 * p) as f64 + g as f64 * q) as i32;
                let ncg = ((src.green_of(c) as f32 * p) as f64 + g as f64 * q) as i32;
                let ncb = ((src.blue_of(c) as f32 * p) as f64 + g as f64 * q) as i32;
                let mut nc = dst.color_exact(ncr, ncg, ncb);
                if nc == -1 {
                    nc = dst.color_allocate(ncr, ncg, ncb);
                    if nc == -1 {
                        nc = dst.color_closest(ncr, ncg, ncb);
                    }
                }
                nc
            };
            dst.set_pixel(tox, toy, nc);
            tox += 1;
        }
        toy += 1;
    }
}

/// gdImageCopyResized()
#[allow(clippy::too_many_arguments)]
pub fn copy_resized(
    dst: &mut Image,
    src: &Image,
    same: bool,
    dst_x: i32,
    dst_y: i32,
    src_x: i32,
    src_y: i32,
    dst_w: i32,
    dst_h: i32,
    src_w: i32,
    src_h: i32,
) {
    if overflow2(4, src_w) {
        return;
    }
    if overflow2(4, src_h) {
        return;
    }
    let stx: Vec<i32> = (0..src_w)
        .map(|i| {
            (dst_w as i64 * (i as i64 + 1) / src_w as i64 - dst_w as i64 * i as i64 / src_w as i64)
                as i32
        })
        .collect();
    let sty: Vec<i32> = (0..src_h)
        .map(|i| {
            (dst_h as i64 * (i as i64 + 1) / src_h as i64 - dst_h as i64 * i as i64 / src_h as i64)
                as i32
        })
        .collect();
    let mut color_map = [-1i32; MAX_COLORS];
    let mut toy = dst_y;
    for y in src_y..src_y + src_h {
        for _ydest in 0..sty[(y - src_y) as usize] {
            let mut tox = dst_x;
            for x in src_x..src_x + src_w {
                let st = stx[(x - src_x) as usize];
                if st == 0 {
                    continue;
                }
                let map_to;
                if dst.true_color {
                    if !src.true_color {
                        let tmp = src.get_pixel(x, y);
                        map_to = src.get_true_color_pixel(x, y);
                        if src.transparent == tmp {
                            tox += st;
                            continue;
                        }
                    } else {
                        map_to = src.get_true_color_pixel(x, y);
                        if src.transparent == map_to {
                            tox += st;
                            continue;
                        }
                    }
                } else {
                    let c = src.get_pixel(x, y);
                    if src.transparent == c {
                        tox += st;
                        continue;
                    }
                    if src.true_color {
                        map_to = dst.color_resolve_alpha(
                            true_color_get_red(c),
                            true_color_get_green(c),
                            true_color_get_blue(c),
                            true_color_get_alpha(c),
                        );
                    } else {
                        let ci = c as usize;
                        if color_map[ci] == -1 {
                            let nc = if same {
                                c
                            } else {
                                dst.color_resolve_alpha(
                                    src.red_of(c),
                                    src.green_of(c),
                                    src.blue_of(c),
                                    src.alpha_of(c),
                                )
                            };
                            color_map[ci] = nc;
                        }
                        map_to = color_map[ci];
                    }
                }
                for _ in 0..st {
                    dst.set_pixel(tox, toy, map_to);
                    tox += 1;
                }
            }
            toy += 1;
        }
    }
}

/// gdImageCopyResampled()
#[allow(clippy::too_many_arguments)]
pub fn copy_resampled(
    dst: &mut Image,
    src: &Image,
    same: bool,
    dst_x: i32,
    dst_y: i32,
    src_x: i32,
    src_y: i32,
    dst_w: i32,
    dst_h: i32,
    src_w: i32,
    src_h: i32,
) {
    if !dst.true_color {
        copy_resized(
            dst, src, same, dst_x, dst_y, src_x, src_y, dst_w, dst_h, src_w, src_h,
        );
        return;
    }
    for y in dst_y..dst_y + dst_h {
        let sy1 = (y as f64 - dst_y as f64) * src_h as f64 / dst_h as f64;
        let sy2 = ((y + 1) as f64 - dst_y as f64) * src_h as f64 / dst_h as f64;
        for x in dst_x..dst_x + dst_w {
            let mut spixels = 0f64;
            let (mut red, mut green, mut blue, mut alpha) = (0f64, 0f64, 0f64, 0f64);
            let (mut alpha_sum, mut contrib_sum) = (0f64, 0f64);
            let sx1 = (x as f64 - dst_x as f64) * src_w as f64 / dst_w as f64;
            let sx2 = ((x + 1) as f64 - dst_x as f64) * src_w as f64 / dst_w as f64;
            let mut sy = sy1;
            loop {
                let yportion;
                if floor_cast(sy) == floor_cast(sy1) {
                    let mut yp = 1.0f32 as f64 - (sy - floor_cast(sy) as f64);
                    if yp > sy2 - sy1 {
                        yp = sy2 - sy1;
                    }
                    yportion = yp;
                    sy = floor_cast(sy) as f64;
                } else if sy == (sy2 as f32).floor() as f64 {
                    yportion = sy2 - floor_cast(sy2) as f64;
                } else {
                    yportion = 1.0;
                }
                let mut sx = sx1;
                loop {
                    let xportion;
                    if (sx as f32).floor() as f64 == floor_cast(sx1) as f64 {
                        let mut xp = 1.0 - (sx - floor_cast(sx) as f64);
                        if xp > sx2 - sx1 {
                            xp = sx2 - sx1;
                        }
                        xportion = xp;
                        sx = floor_cast(sx) as f64;
                    } else if sx == (sx2 as f32).floor() as f64 {
                        xportion = sx2 - floor_cast(sx2) as f64;
                    } else {
                        xportion = 1.0;
                    }
                    let pcontribution = xportion * yportion;
                    let p = src.get_true_color_pixel(sx as i32 + src_x, sy as i32 + src_y);
                    let alpha_factor = (ALPHA_MAX - true_color_get_alpha(p)) as f64 * pcontribution;
                    red += true_color_get_red(p) as f64 * alpha_factor;
                    green += true_color_get_green(p) as f64 * alpha_factor;
                    blue += true_color_get_blue(p) as f64 * alpha_factor;
                    alpha += true_color_get_alpha(p) as f64 * pcontribution;
                    alpha_sum += alpha_factor;
                    contrib_sum += pcontribution;
                    spixels += xportion * yportion;
                    sx += 1.0;
                    if sx >= sx2 {
                        break;
                    }
                }
                sy += 1.0;
                if sy >= sy2 {
                    break;
                }
            }
            if spixels != 0.0 {
                red /= spixels;
                green /= spixels;
                blue /= spixels;
                alpha /= spixels;
            }
            if alpha_sum != 0.0 {
                if contrib_sum != 0.0 {
                    alpha_sum /= contrib_sum;
                }
                red /= alpha_sum;
                green /= alpha_sum;
                blue /= alpha_sum;
            }
            red = if red >= 255.5 { 255.0 } else { red + 0.5 };
            blue = if blue >= 255.5 { 255.0 } else { blue + 0.5 };
            green = if green >= 255.5 { 255.0 } else { green + 0.5 };
            alpha = if alpha >= ALPHA_MAX as f64 + 0.5 {
                ALPHA_MAX as f64
            } else {
                alpha + 0.5
            };
            dst.set_pixel(
                x,
                y,
                true_color_alpha(red as i32, green as i32, blue as i32, alpha as i32),
            );
        }
    }
}

/// clip_1d(): Sutherland Hodgman clipping of one coordinate
fn clip_1d(
    x0: &mut i32,
    y0: &mut i32,
    x1: &mut i32,
    y1: &mut i32,
    mindim: i32,
    maxdim: i32,
) -> bool {
    let slope = |x0: i32, y0: i32, x1: i32, y1: i32| -> f64 {
        (y1 as i64 - y0 as i64) as i32 as f64 / (x1 as i64 - x0 as i64) as i32 as f64
    };
    if *x0 < mindim {
        if *x1 < mindim {
            return false;
        }
        let m = slope(*x0, *y0, *x1, *y1);
        *y0 = y0.wrapping_sub((m * (x0.wrapping_sub(mindim)) as f64) as i32);
        *x0 = mindim;
        if *x1 > maxdim {
            *y1 = y1.wrapping_add((m * (maxdim.wrapping_sub(*x1)) as f64) as i32);
            *x1 = maxdim;
        }
        return true;
    }
    if *x0 > maxdim {
        if *x1 > maxdim {
            return false;
        }
        let m = slope(*x0, *y0, *x1, *y1);
        *y0 = y0.wrapping_add((m * (maxdim.wrapping_sub(*x0)) as f64) as i32);
        *x0 = maxdim;
        if *x1 < mindim {
            *y1 = y1.wrapping_sub((m * (x1.wrapping_sub(mindim)) as f64) as i32);
            *x1 = mindim;
        }
        return true;
    }
    if *x1 > maxdim {
        let m = slope(*x0, *y0, *x1, *y1);
        *y1 = y1.wrapping_add((m * (maxdim.wrapping_sub(*x1)) as f64) as i32);
        *x1 = maxdim;
        return true;
    }
    if *x1 < mindim {
        let m = slope(*x0, *y0, *x1, *y1);
        *y1 = y1.wrapping_sub((m * (x1.wrapping_sub(mindim)) as f64) as i32);
        *x1 = mindim;
        return true;
    }
    true
}

struct Hwb {
    h: f32,
    w: f32,
    b: f32,
}

const HWB_UNDEFINED: f32 = -1.0;

fn rgb_to_hwb(r: f32, g: f32, b: f32) -> Hwb {
    let min3 = |a: f32, b: f32, c: f32| {
        if a < b {
            if a < c { a } else { c }
        } else if b < c {
            b
        } else {
            c
        }
    };
    let max3 = |a: f32, b: f32, c: f32| {
        if a < b {
            if b < c { c } else { b }
        } else if a < c {
            c
        } else {
            a
        }
    };
    let w = min3(r, g, b);
    let v = max3(r, g, b);
    let bb = 1.0 - v;
    if v == w {
        return Hwb {
            h: HWB_UNDEFINED,
            w,
            b: bb,
        };
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
    Hwb {
        h: i - f / (v - w),
        w,
        b: bb,
    }
}

fn hwb_diff(r1: i32, g1: i32, b1: i32, r2: i32, g2: i32, b2: i32) -> f32 {
    let h1 = rgb_to_hwb(r1 as f32 / 255.0, g1 as f32 / 255.0, b1 as f32 / 255.0);
    let h2 = rgb_to_hwb(r2 as f32 / 255.0, g2 as f32 / 255.0, b2 as f32 / 255.0);
    let mut diff = if h1.h == HWB_UNDEFINED || h2.h == HWB_UNDEFINED {
        0.0f32
    } else {
        let mut d = (h1.h - h2.h).abs();
        if d > 3.0 {
            d = 6.0 - d;
        }
        d
    };
    diff = diff * diff + (h1.w - h2.w) * (h1.w - h2.w) + (h1.b - h2.b) * (h1.b - h2.b);
    diff
}

/// gdAlphaBlend()
pub fn alpha_blend(dst: i32, src: i32) -> i32 {
    let src_alpha = true_color_get_alpha(src);
    if src_alpha == ALPHA_OPAQUE {
        return src;
    }
    let dst_alpha = true_color_get_alpha(dst);
    if src_alpha == ALPHA_TRANSPARENT {
        return dst;
    }
    if dst_alpha == ALPHA_TRANSPARENT {
        return src;
    }
    let src_weight = ALPHA_TRANSPARENT - src_alpha;
    let dst_weight = (ALPHA_TRANSPARENT - dst_alpha) * src_alpha / ALPHA_MAX;
    let tot_weight = src_weight + dst_weight;
    let alpha = src_alpha * dst_alpha / ALPHA_MAX;
    let red =
        (true_color_get_red(src) * src_weight + true_color_get_red(dst) * dst_weight) / tot_weight;
    let green = (true_color_get_green(src) * src_weight + true_color_get_green(dst) * dst_weight)
        / tot_weight;
    let blue = (true_color_get_blue(src) * src_weight + true_color_get_blue(dst) * dst_weight)
        / tot_weight;
    (alpha << 24) + (red << 16) + (green << 8) + blue
}

/// gdLayerOverlay()
pub fn layer_overlay(dst: i32, src: i32) -> i32 {
    let a1 = ALPHA_MAX - true_color_get_alpha(dst);
    let a2 = ALPHA_MAX - true_color_get_alpha(src);
    ((ALPHA_MAX - a1 * a2 / ALPHA_MAX) << 24)
        + (alpha_overlay_color(true_color_get_red(src), true_color_get_red(dst), RED_MAX) << 16)
        + (alpha_overlay_color(
            true_color_get_green(src),
            true_color_get_green(dst),
            GREEN_MAX,
        ) << 8)
        + alpha_overlay_color(true_color_get_blue(src), true_color_get_blue(dst), BLUE_MAX)
}

fn alpha_overlay_color(src: i32, dst: i32, max: i32) -> i32 {
    let dst = dst << 1;
    if dst > max {
        dst + (src << 1) - (dst * src / max) - max
    } else {
        dst * src / max
    }
}

/// gdLayerMultiply()
pub fn layer_multiply(dst: i32, src: i32) -> i32 {
    let mut a1 = ALPHA_MAX - true_color_get_alpha(src);
    let mut a2 = ALPHA_MAX - true_color_get_alpha(dst);
    let r1 = RED_MAX - (a1 * (RED_MAX - true_color_get_red(src))) / ALPHA_MAX;
    let r2 = RED_MAX - (a2 * (RED_MAX - true_color_get_red(dst))) / ALPHA_MAX;
    let g1 = GREEN_MAX - (a1 * (GREEN_MAX - true_color_get_green(src))) / ALPHA_MAX;
    let g2 = GREEN_MAX - (a2 * (GREEN_MAX - true_color_get_green(dst))) / ALPHA_MAX;
    let b1 = BLUE_MAX - (a1 * (BLUE_MAX - true_color_get_blue(src))) / ALPHA_MAX;
    let b2 = BLUE_MAX - (a2 * (BLUE_MAX - true_color_get_blue(dst))) / ALPHA_MAX;
    a1 = ALPHA_MAX - a1;
    a2 = ALPHA_MAX - a2;
    ((a1 * a2 / ALPHA_MAX) << 24)
        + ((r1 * r2 / RED_MAX) << 16)
        + ((g1 * g2 / GREEN_MAX) << 8)
        + (b1 * b2 / BLUE_MAX)
}
