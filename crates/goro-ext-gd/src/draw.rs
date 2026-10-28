//! Drawing primitives: lines (thick, dashed, styled, brushed,
//! antialiased), rectangles, polygons, ellipses, arcs and fills.
//!
//! Behaviour sources: the PHP manual pages of imageline, imagerectangle,
//! imagepolygon, imagefilledpolygon, imagearc, imagefilledarc,
//! imageellipse, imagefill, imagefilltoborder, imagesetthickness,
//! imagesetstyle, imageantialias; libgd's API documentation; the pixel
//! output the ext/gd tests compare against.

use crate::image::*;

/// Arc styles (IMG_ARC_*).
pub const ARC_PIE: i32 = 0;
pub const ARC_CHORD: i32 = 1;
pub const ARC_NOFILL: i32 = 2;
pub const ARC_EDGED: i32 = 4;

impl Image {
    /// imageline.
    pub fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: i32) {
        if color == ANTIALIASED {
            self.aa_line(x1, y1, x2, y2, self.aa_color, true);
            return;
        }
        let thick = self.thick;
        if x1 == x2 && y1 == y2 && thick == 1 {
            self.set_pixel(x1, y1, color);
            return;
        }
        let (mut x1, mut y1, mut x2, mut y2) = (x1 as i64, y1 as i64, x2 as i64, y2 as i64);
        // Clip to the image plus the thickness margin.
        // Clipped to the image first, which moves the endpoints
        // (bug77943).
        let m = 0;
        if !clip_line(
            &mut x1,
            &mut y1,
            &mut x2,
            &mut y2,
            -m,
            -m,
            self.sx as i64 - 1 + m,
            self.sy as i64 - 1 + m,
        ) {
            return;
        }
        let (x1, y1, x2, y2) = (x1 as i32, y1 as i32, x2 as i32, y2 as i32);
        let dx = (x2 - x1).abs();
        let dy = (y2 - y1).abs();
        if dx == 0 {
            self.vline(x1, y1, y2, color);
            return;
        }
        if dy == 0 {
            self.hline(y1, x1, x2, color);
            return;
        }
        if dy <= dx {
            // More horizontal: vertical runs of `wid` pixels.
            let wid = if thick > 1 {
                let ac = (dx as f64) / ((dx as f64).hypot(dy as f64));
                if ac != 0.0 {
                    (thick as f64 / ac) as i32
                } else {
                    1
                }
            } else {
                1
            }
            .max(1);
            let mut d = 2 * dy - dx;
            let incr1 = 2 * dy;
            let incr2 = 2 * (dy - dx);
            let (mut x, mut y, xend, ydirflag) = if x1 > x2 {
                (x2, y2, x1, if y1 < y2 { -1 } else { 1 })
            } else {
                (x1, y1, x2, if y2 < y1 { -1 } else { 1 })
            };
            self.vrun(x, y, wid, color);
            while x < xend {
                x += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    y += ydirflag;
                    d += incr2;
                }
                self.vrun(x, y, wid, color);
            }
        } else {
            let wid = if thick > 1 {
                let as_ = (dy as f64) / ((dx as f64).hypot(dy as f64));
                if as_ != 0.0 {
                    (thick as f64 / as_) as i32
                } else {
                    1
                }
            } else {
                1
            }
            .max(1);
            let mut d = 2 * dx - dy;
            let incr1 = 2 * dx;
            let incr2 = 2 * (dx - dy);
            let (mut x, mut y, yend, xdirflag) = if y1 > y2 {
                (x2, y2, y1, if x1 < x2 { -1 } else { 1 })
            } else {
                (x1, y1, y2, if x2 < x1 { -1 } else { 1 })
            };
            self.hrun(x, y, wid, color);
            while y < yend {
                y += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    x += xdirflag;
                    d += incr2;
                }
                self.hrun(x, y, wid, color);
            }
        }
    }

    /// A vertical run of `wid` pixels centred on (x, y).
    fn vrun(&mut self, x: i32, y: i32, wid: i32, color: i32) {
        if wid == 1 {
            self.set_pixel(x, y, color);
            return;
        }
        // Styled thick lines take one style entry per pixel of each
        // run (bug43475).
        let w = y - wid / 2;
        for yy in w..w + wid {
            self.set_pixel(x, yy, color);
        }
    }

    fn hrun(&mut self, x: i32, y: i32, wid: i32, color: i32) {
        if wid == 1 {
            self.set_pixel(x, y, color);
            return;
        }
        let w = x - wid / 2;
        for xx in w..w + wid {
            self.set_pixel(xx, y, color);
        }
    }

    /// Horizontal line with thickness.
    fn hline(&mut self, y: i32, x1: i32, x2: i32, color: i32) {
        let (x1, x2) = if x1 > x2 { (x2, x1) } else { (x1, x2) };
        if self.thick > 1 && color == STYLED {
            for x in x1..=x2 {
                self.vrun(x, y, self.thick, color);
            }
        } else if self.thick > 1 {
            let t = self.thick;
            let y1 = y - t / 2;
            self.filled_rectangle(x1, y1, x2, y1 + t - 1, color);
        } else {
            for x in x1..=x2 {
                self.set_pixel(x, y, color);
            }
        }
    }

    fn vline(&mut self, x: i32, y1: i32, y2: i32, color: i32) {
        let (y1, y2) = if y1 > y2 { (y2, y1) } else { (y1, y2) };
        if self.thick > 1 && color == STYLED {
            for y in y1..=y2 {
                self.hrun(x, y, self.thick, color);
            }
        } else if self.thick > 1 {
            let t = self.thick;
            let x1 = x - t / 2;
            self.filled_rectangle(x1, y1, x1 + t - 1, y2, color);
        } else {
            for y in y1..=y2 {
                self.set_pixel(x, y, color);
            }
        }
    }

    /// imagedashedline: 4 pixels on, 4 off, along the major axis.
    pub fn dashed_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: i32) {
        const DASH: i32 = 4;
        let dx = (x2 - x1).abs();
        let dy = (y2 - y1).abs();
        let mut dash_step = 0;
        let mut on = true;
        let thick = self.thick;
        let mut dot = |im: &mut Image, x: i32, y: i32, vert: bool, wid: i32| {
            dash_step += 1;
            if dash_step == DASH {
                dash_step = 0;
                on = !on;
            }
            if on {
                if vert {
                    im.vrun(x, y, wid, color);
                } else {
                    im.hrun(x, y, wid, color);
                }
            }
        };
        if dy <= dx {
            let wid = if thick > 1 && dx != 0 {
                let ac = (dx as f64) / ((dx as f64).hypot(dy as f64));
                ((thick as f64) / ac) as i32
            } else {
                1
            }
            .max(1);
            let mut d = 2 * dy - dx;
            let incr1 = 2 * dy;
            let incr2 = 2 * (dy - dx);
            let (mut x, mut y, xend, ydirflag) = if x1 > x2 {
                (x2, y2, x1, if y1 < y2 { -1 } else { 1 })
            } else {
                (x1, y1, x2, if y2 < y1 { -1 } else { 1 })
            };
            dot(self, x, y, true, wid);
            while x < xend {
                x += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    y += ydirflag;
                    d += incr2;
                }
                dot(self, x, y, true, wid);
            }
        } else {
            let wid = if thick > 1 {
                let as_ = (dy as f64) / ((dx as f64).hypot(dy as f64));
                ((thick as f64) / as_) as i32
            } else {
                1
            }
            .max(1);
            let mut d = 2 * dx - dy;
            let incr1 = 2 * dx;
            let incr2 = 2 * (dx - dy);
            let (mut x, mut y, yend, xdirflag) = if y1 > y2 {
                (x2, y2, y1, if x1 < x2 { -1 } else { 1 })
            } else {
                (x1, y1, y2, if x2 < x1 { -1 } else { 1 })
            };
            dot(self, x, y, false, wid);
            while y < yend {
                y += 1;
                if d < 0 {
                    d += incr1;
                } else {
                    x += xdirflag;
                    d += incr2;
                }
                dot(self, x, y, false, wid);
            }
        }
    }

    /// imagerectangle.
    pub fn rectangle(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: i32) {
        let (x1, x2) = if x1 > x2 { (x2, x1) } else { (x1, x2) };
        let (y1, y2) = if y1 > y2 { (y2, y1) } else { (y1, y2) };
        let t = self.thick;
        if t > 1 {
            // Horizontal bars `t` rows high centred on the edges,
            // vertical bars one column wider (imagesetthickness_basic.png:
            // thickness 5 gives 5-row and 6-column bars).
            let half = t / 2;
            let (ox1, oy1) = (x1 - half, y1 - half);
            let (ox2, oy2) = (x2 + half, y2 + half);
            let top_end = oy1 + t - 1;
            let bot_start = oy2 - t + 1;
            if top_end + 1 >= bot_start || ox1 + t >= ox2 - t {
                self.filled_rectangle(ox1, oy1, ox2, oy2, color);
                return;
            }
            self.filled_rectangle(ox1, oy1, ox2, top_end, color);
            self.filled_rectangle(ox1, bot_start, ox2, oy2, color);
            self.filled_rectangle(ox1, top_end + 1, ox1 + t, bot_start - 1, color);
            self.filled_rectangle(ox2 - t, top_end + 1, ox2, bot_start - 1, color);
            return;
        }
        if x1 == x2 && y1 == y2 {
            self.set_pixel(x1, y1, color);
            return;
        }
        if y1 == y2 {
            for x in x1..=x2 {
                self.set_pixel(x, y1, color);
            }
            return;
        }
        if x1 == x2 {
            for y in y1..=y2 {
                self.set_pixel(x1, y, color);
            }
            return;
        }
        for x in x1..=x2 {
            self.set_pixel(x, y1, color);
            self.set_pixel(x, y2, color);
        }
        for y in y1 + 1..y2 {
            self.set_pixel(x1, y, color);
            self.set_pixel(x2, y, color);
        }
    }

    /// imagefilledrectangle (clipped to the image).
    pub fn filled_rectangle(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: i32) {
        let (x1, x2) = if x1 > x2 { (x2, x1) } else { (x1, x2) };
        let (y1, y2) = if y1 > y2 { (y2, y1) } else { (y1, y2) };
        let x1 = x1.max(0);
        let y1 = y1.max(0);
        let x2 = x2.min(self.sx - 1);
        let y2 = y2.min(self.sy - 1);
        if x1 > x2 || y1 > y2 {
            return;
        }
        if color >= 0 && self.truecolor && self.alpha_blending == EFFECT_REPLACE {
            let x1 = x1.max(self.cx1);
            let x2 = x2.min(self.cx2);
            let y1c = y1.max(self.cy1);
            let y2c = y2.min(self.cy2);
            for y in y1c..=y2c {
                let row = y as usize * self.sx as usize;
                for x in x1..=x2 {
                    self.tpixels[row + x as usize] = color;
                }
            }
            return;
        }
        for y in y1..=y2 {
            for x in x1..=x2 {
                self.set_pixel(x, y, color);
            }
        }
    }

    /// imagepolygon / imageopenpolygon.
    pub fn polygon(&mut self, pts: &[(i32, i32)], color: i32, closed: bool) {
        if pts.is_empty() {
            return;
        }
        if color == ANTIALIASED {
            let c = self.aa_color;
            for w in pts.windows(2) {
                self.aa_line(w[0].0, w[0].1, w[1].0, w[1].1, c, false);
            }
            if closed && pts.len() > 1 {
                let (a, b) = (pts[pts.len() - 1], pts[0]);
                self.aa_line(a.0, a.1, b.0, b.1, c, false);
            }
            return;
        }
        if closed {
            let (a, b) = (pts[0], pts[pts.len() - 1]);
            self.line(a.0, a.1, b.0, b.1, color);
        }
        for w in pts.windows(2) {
            self.line(w[0].0, w[0].1, w[1].0, w[1].1, color);
        }
    }

    /// imagefilledpolygon: even-odd scanline fill.
    pub fn filled_polygon(&mut self, pts: &[(i32, i32)], color: i32) {
        if pts.is_empty() {
            return;
        }
        let fill_color = if color == ANTIALIASED {
            self.aa_color
        } else {
            color
        };
        let mut spans = Vec::new();
        polygon_spans(pts, 0, self.sy - 1, &mut spans);
        for (y, x1, x2) in spans {
            self.hline_plain(y, x1, x2, fill_color);
        }
        if color == ANTIALIASED {
            self.polygon(pts, color, true);
        }
    }

    fn hline_plain(&mut self, y: i32, x1: i32, x2: i32, color: i32) {
        let (x1, x2) = if x1 > x2 { (x2, x1) } else { (x1, x2) };
        let x1 = x1.max(0);
        let x2 = x2.min(self.sx - 1);
        for x in x1..=x2 {
            self.set_pixel(x, y, color);
        }
    }

    /// imageellipse (midpoint algorithm).
    pub fn ellipse(&mut self, mx: i32, my: i32, w: i32, h: i32, c: i32) {
        let a = w >> 1;
        let b = h >> 1;
        self.set_pixel(mx + a, my, c);
        self.set_pixel(mx - a, my, c);
        let mut mx1 = mx - a;
        let mut my1 = my;
        let mut mx2 = mx + a;
        let mut my2 = my;
        let a2 = a as i64 * a as i64;
        let b2 = b as i64 * b as i64;
        let mut tmp = a2;
        let mut x = a as i64;
        let mut y: i64 = 0;
        let mut rx = x * b2 * 2;
        let mut ry: i64 = 0;
        let mut r = x * b2;
        while x > 0 {
            if r > 0 {
                my1 += 1;
                my2 -= 1;
                y += 1;
                ry += tmp * 2;
                r -= ry;
                let _ = y;
            }
            if r <= 0 {
                x -= 1;
                mx1 += 1;
                mx2 -= 1;
                rx -= b2 * 2;
                r += rx;
            }
            self.set_pixel(mx1, my1, c);
            self.set_pixel(mx1, my2, c);
            self.set_pixel(mx2, my1, c);
            self.set_pixel(mx2, my2, c);
            tmp = a2;
        }
    }

    /// imagefilledellipse.
    pub fn filled_ellipse(&mut self, mx: i32, my: i32, w: i32, h: i32, c: i32) {
        let a = w >> 1;
        let b = h >> 1;
        for x in mx - a..=mx + a {
            self.set_pixel(x, my, c);
        }
        let mut mx1 = mx - a;
        let mut my1 = my;
        let mut mx2 = mx + a;
        let mut my2 = my;
        let a2 = a as i64 * a as i64;
        let b2 = b as i64 * b as i64;
        let mut x = a as i64;
        let mut rx = x * b2 * 2;
        let mut ry: i64 = 0;
        let mut r = x * b2;
        let mut old_y2 = -2;
        while x > 0 {
            if r > 0 {
                my1 += 1;
                my2 -= 1;
                ry += a2 * 2;
                r -= ry;
            }
            if r <= 0 {
                x -= 1;
                mx1 += 1;
                mx2 -= 1;
                rx -= b2 * 2;
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

    /// imagearc: the outline of a partial ellipse.
    pub fn arc(&mut self, cx: i32, cy: i32, w: i32, h: i32, s: i32, e: i32, color: i32) {
        // Points every degree, truncated toward the centre
        // (imagearc_basic.png); full circles too, so that the thickness
        // applies (bug72604).
        self.filled_arc(cx, cy, w, h, s, e, color, ARC_NOFILL);
    }

    /// imagefilledarc.
    #[allow(clippy::too_many_arguments)]
    pub fn filled_arc(
        &mut self,
        cx: i32,
        cy: i32,
        w: i32,
        h: i32,
        s: i32,
        e: i32,
        color: i32,
        style: i32,
    ) {
        let (mut s, mut e) = (s, e);
        if (s % 360) == (e % 360) {
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
        let pt = |a: i32| arc_point(cx, cy, w, h, a);
        let mut pts: Vec<(i32, i32)> = Vec::new();
        let (mut lx, mut ly) = (0, 0);
        let (mut fx, mut fy) = (0, 0);
        for i in s..=e {
            let (x, y) = pt(i);
            if i != s {
                if style & ARC_CHORD == 0 {
                    if style & ARC_NOFILL != 0 {
                        self.line(lx, ly, x, y, color);
                    } else if y == ly {
                        pts.push((x, y));
                    } else {
                        pts.push((x, y));
                    }
                }
            } else {
                fx = x;
                fy = y;
                if style & (ARC_CHORD | ARC_NOFILL) == 0 {
                    pts.push((cx, cy));
                    pts.push((x, y));
                }
            }
            lx = x;
            ly = y;
        }
        if style & ARC_CHORD != 0 {
            if style & ARC_NOFILL != 0 {
                if style & ARC_EDGED != 0 {
                    self.line(cx, cy, lx, ly, color);
                    self.line(cx, cy, fx, fy, color);
                }
                self.line(fx, fy, lx, ly, color);
            } else {
                let tri = [(cx, cy), (fx, fy), (lx, ly)];
                self.filled_polygon(&tri, color);
            }
        } else if style & ARC_NOFILL != 0 {
            if style & ARC_EDGED != 0 {
                self.line(cx, cy, lx, ly, color);
                self.line(cx, cy, fx, fy, color);
            }
        } else {
            // A pie: a fan of triangles from the centre, one per degree,
            // whose union is painted once (imagefilledarc_basic.png,
            // imagegammacorrect_basic.png).
            let fill_color = if color == ANTIALIASED {
                self.aa_color
            } else {
                color
            };
            let mut spans = Vec::new();
            let mut prev = pts[1];
            for &p in &pts[2..] {
                polygon_spans(&[(cx, cy), prev, p], 0, self.sy - 1, &mut spans);
                prev = p;
            }
            if pts.len() == 2 {
                polygon_spans(&[(cx, cy), pts[1]], 0, self.sy - 1, &mut spans);
            }
            self.paint_spans(&mut spans, fill_color);
        }
    }

    /// Paints the union of spans, each pixel once.
    fn paint_spans(&mut self, spans: &mut [(i32, i32, i32)], color: i32) {
        for sp in spans.iter_mut() {
            if sp.1 > sp.2 {
                std::mem::swap(&mut sp.1, &mut sp.2);
            }
        }
        spans.sort_unstable();
        let mut cur: Option<(i32, i32, i32)> = None;
        for &(y, a, b) in spans.iter() {
            match cur {
                Some((cy, ca, cb)) if cy == y && a <= cb + 1 => cur = Some((cy, ca, cb.max(b))),
                Some((cy, ca, cb)) => {
                    self.hline_plain(cy, ca, cb, color);
                    cur = Some((y, a, b));
                }
                None => cur = Some((y, a, b)),
            }
        }
        if let Some((cy, ca, cb)) = cur {
            self.hline_plain(cy, ca, cb, color);
        }
    }

    // ------------------------------------------------------------ fills

    /// imagefill: flood fills the 4-connected region of the color at
    /// (x, y).
    pub fn fill(&mut self, x: i32, y: i32, nc: i32) {
        if !self.in_bounds(x, y) {
            return;
        }
        if nc == TILED {
            self.fill_tiled(x, y);
            return;
        }
        let oc = self.get_pixel(x, y);
        if nc < 0 || (!self.truecolor && nc >= self.colors_total) {
            // An unallocated palette index fills nothing (imagefill_1).
            return;
        }
        if oc == nc {
            return;
        }
        // Without alpha blending the fill color must differ from the old
        // one; with it, pixels are compared before blending.
        let w = self.sx;
        let h = self.sy;
        let mut seen = vec![false; (w as usize) * (h as usize)];
        let mut stack = vec![(x, y)];
        while let Some((sx, sy)) = stack.pop() {
            let row = sy as usize * w as usize;
            if seen[row + sx as usize] || self.get_pixel(sx, sy) != oc {
                continue;
            }
            let mut l = sx;
            while l > 0 && !seen[row + l as usize - 1] && self.get_pixel(l - 1, sy) == oc {
                l -= 1;
            }
            let mut r = sx;
            while r < w - 1 && !seen[row + r as usize + 1] && self.get_pixel(r + 1, sy) == oc {
                r += 1;
            }
            for xx in l..=r {
                seen[row + xx as usize] = true;
            }
            for xx in l..=r {
                if sy > 0 && !seen[row - w as usize + xx as usize] && self.get_pixel(xx, sy - 1) == oc
                {
                    stack.push((xx, sy - 1));
                }
                if sy < h - 1
                    && !seen[row + w as usize + xx as usize]
                    && self.get_pixel(xx, sy + 1) == oc
                {
                    stack.push((xx, sy + 1));
                }
            }
        }
        // The fill color replaces the region's pixels, without alpha
        // blending (bug61221: a transparent fill stays transparent).
        let saved = self.alpha_blending;
        self.alpha_blending = EFFECT_REPLACE;
        for yy in 0..h {
            for xx in 0..w {
                if seen[yy as usize * w as usize + xx as usize] {
                    self.set_pixel(xx, yy, nc);
                }
            }
        }
        self.alpha_blending = saved;
    }

    fn fill_tiled(&mut self, x: i32, y: i32) {
        if self.tile.is_none() {
            return;
        }
        let oc = self.get_pixel(x, y);
        let w = self.sx;
        let h = self.sy;
        let mut seen = vec![false; (w as usize) * (h as usize)];
        let mut stack = vec![(x, y)];
        while let Some((sx, sy)) = stack.pop() {
            let i = sy as usize * w as usize + sx as usize;
            if seen[i] || self.get_pixel(sx, sy) != oc {
                continue;
            }
            seen[i] = true;
            if sx > 0 {
                stack.push((sx - 1, sy));
            }
            if sx < w - 1 {
                stack.push((sx + 1, sy));
            }
            if sy > 0 {
                stack.push((sx, sy - 1));
            }
            if sy < h - 1 {
                stack.push((sx, sy + 1));
            }
        }
        for yy in 0..h {
            for xx in 0..w {
                if seen[yy as usize * w as usize + xx as usize]
                    && let Some(c) = self.tile_color(xx, yy)
                {
                    self.set_pixel_color(xx, yy, c);
                }
            }
        }
    }

    /// imagefilltoborder: fills up to pixels of the border color.
    pub fn fill_to_border(&mut self, x: i32, y: i32, border: i32, color: i32) {
        if border < 0 || !self.in_bounds(x, y) {
            return;
        }
        let w = self.sx;
        let h = self.sy;
        let mut seen = vec![false; (w as usize) * (h as usize)];
        let mut stack = vec![(x, y)];
        let restore = self.alpha_blending;
        self.alpha_blending = EFFECT_REPLACE;
        let mut pts = Vec::new();
        while let Some((sx, sy)) = stack.pop() {
            let i = sy as usize * w as usize + sx as usize;
            if seen[i] {
                continue;
            }
            let p = self.get_pixel(sx, sy);
            if p == border {
                continue;
            }
            seen[i] = true;
            pts.push((sx, sy));
            if sx > 0 {
                stack.push((sx - 1, sy));
            }
            if sx < w - 1 {
                stack.push((sx + 1, sy));
            }
            if sy > 0 {
                stack.push((sx, sy - 1));
            }
            if sy < h - 1 {
                stack.push((sx, sy + 1));
            }
        }
        // The fill color replaces the pixels, without alpha blending
        // (bug27582_1: alpha 1 over opaque black stays alpha 1).
        for (px, py) in pts {
            self.set_pixel(px, py, color);
        }
        self.alpha_blending = restore;
    }

    // ------------------------------------------------------------ antialiasing

    /// An antialiased line (Wu's algorithm), blending `color` with
    /// coverage-weighted alpha.
    pub fn aa_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: i32, _cap: bool) {
        let dx = x2 - x1;
        let dy = y2 - y1;
        if dx == 0 && dy == 0 {
            self.set_pixel_color(x1, y1, color);
            return;
        }
        if dx == 0 || dy == 0 || self.thick > 1 || !self.truecolor {
            let saved = self.aa;
            self.aa = false;
            self.line(x1, y1, x2, y2, color);
            self.aa = saved;
            return;
        }
        let base_alpha = tc_a(color);
        let plot = |im: &mut Image, x: i32, y: i32, cov: i32| {
            if !im.in_bounds(x, y) || !im.in_clip(x, y) {
                return;
            }
            if im.aa_dont_blend >= 0 && im.get_pixel(x, y) == im.aa_dont_blend {
                return;
            }
            // Coverage 0..255 lowers the opacity.
            let opacity = (127 - base_alpha) * cov / 255;
            let a = 127 - opacity;
            let c = (color & 0x00ff_ffff) | (a << 24);
            let i = y as usize * im.sx as usize + x as usize;
            im.tpixels[i] = alpha_blend(im.tpixels[i], c);
        };
        if dx.abs() > dy.abs() {
            let (x1, y1, x2, y2) = if dx < 0 {
                (x2, y2, x1, y1)
            } else {
                (x1, y1, x2, y2)
            };
            let grad = ((y2 - y1) as i64) * 65536 / ((x2 - x1) as i64);
            let mut yf: i64 = (y1 as i64) << 16;
            for x in x1..=x2 {
                let yi = (yf >> 16) as i32;
                let frac = ((yf & 0xffff) >> 8) as i32;
                plot(self, x, yi, 255 - frac);
                plot(self, x, yi + 1, frac);
                yf += grad;
            }
        } else {
            let (x1, y1, x2, y2) = if dy < 0 {
                (x2, y2, x1, y1)
            } else {
                (x1, y1, x2, y2)
            };
            let grad = ((x2 - x1) as i64) * 65536 / ((y2 - y1) as i64);
            let mut xf: i64 = (x1 as i64) << 16;
            for y in y1..=y2 {
                let xi = (xf >> 16) as i32;
                let frac = ((xf & 0xffff) >> 8) as i32;
                plot(self, xi, y, 255 - frac);
                plot(self, xi + 1, y, frac);
                xf += grad;
            }
        }
    }
}

/// Clips a segment to a rectangle (Liang–Barsky with integer endpoints
/// rounded to the nearest pixel). Returns false when nothing is left.
#[allow(clippy::too_many_arguments)]
pub fn clip_line(
    x1: &mut i64,
    y1: &mut i64,
    x2: &mut i64,
    y2: &mut i64,
    minx: i64,
    miny: i64,
    maxx: i64,
    maxy: i64,
) -> bool {
    // Inside already.
    let inside = |x: i64, y: i64| x >= minx && x <= maxx && y >= miny && y <= maxy;
    if inside(*x1, *y1) && inside(*x2, *y2) {
        return true;
    }
    if !clip_1d(x1, y1, x2, y2, minx, maxx) {
        return false;
    }
    if !clip_1d(y1, x1, y2, x2, miny, maxy) {
        return false;
    }
    true
}

/// Clips along one axis: moves endpoints outside [min, max] on the
/// first coordinate onto the boundary, adjusting the other coordinate
/// along the line.
fn clip_1d(x0: &mut i64, y0: &mut i64, x1: &mut i64, y1: &mut i64, min: i64, max: i64) -> bool {
    if *x0 < min {
        if *x1 < min {
            return false;
        }
        let m = (*y1 - *y0) as f64 / (*x1 - *x0) as f64;
        *y0 -= (m * (*x0 - min) as f64) as i64;
        *x0 = min;
        if *x1 > max {
            *y1 += (m * (max - *x1) as f64) as i64;
            *x1 = max;
        }
        return true;
    }
    if *x0 > max {
        if *x1 > max {
            return false;
        }
        let m = (*y1 - *y0) as f64 / (*x1 - *x0) as f64;
        *y0 += (m * (max - *x0) as f64) as i64;
        *x0 = max;
        if *x1 < min {
            *y1 -= (m * (*x1 - min) as f64) as i64;
            *x1 = min;
        }
        return true;
    }
    if *x1 > max {
        let m = (*y1 - *y0) as f64 / (*x1 - *x0) as f64;
        *y1 += (m * (max - *x1) as f64) as i64;
        *x1 = max;
        return true;
    }
    if *x1 < min {
        let m = (*y1 - *y0) as f64 / (*x1 - *x0) as f64;
        *y1 -= (m * (*x1 - min) as f64) as i64;
        *x1 = min;
        return true;
    }
    true
}

/// The horizontal spans `(y, x1, x2)` filling a polygon (even-odd rule,
/// rows `ymin..=ymax` only). Each edge covers the rows from its upper
/// end to just above its lower end, the lowest row of the polygon
/// included; crossings are rounded to the nearest pixel.
pub fn polygon_spans(pts: &[(i32, i32)], ymin: i32, ymax: i32, out: &mut Vec<(i32, i32, i32)>) {
    let n = pts.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        if (ymin..=ymax).contains(&pts[0].1) {
            out.push((pts[0].1, pts[0].0, pts[0].0));
        }
        return;
    }
    let miny = pts.iter().map(|p| p.1).min().unwrap();
    let maxy = pts.iter().map(|p| p.1).max().unwrap();
    if miny == maxy {
        if (ymin..=ymax).contains(&miny) {
            let minx = pts.iter().map(|p| p.0).min().unwrap();
            let maxx = pts.iter().map(|p| p.0).max().unwrap();
            out.push((miny, minx, maxx));
        }
        return;
    }
    let mut xs: Vec<i32> = Vec::with_capacity(n);
    for y in miny.max(ymin)..=maxy.min(ymax) {
        xs.clear();
        for i in 0..n {
            let (ind1, ind2) = if i == 0 { (n - 1, 0) } else { (i - 1, i) };
            let (mut x1, mut y1) = pts[ind1];
            let (mut x2, mut y2) = pts[ind2];
            if y1 < y2 {
            } else if y1 > y2 {
                std::mem::swap(&mut x1, &mut x2);
                std::mem::swap(&mut y1, &mut y2);
            } else {
                continue;
            }
            if (y >= y1 && y < y2) || (y == maxy && y > y1 && y <= y2) {
                let v = ((y - y1) as f64 * (x2 - x1) as f64) / ((y2 - y1) as f64) + 0.5;
                xs.push(v.floor() as i32 + x1);
            }
        }
        xs.sort_unstable();
        let mut i = 0;
        while i + 1 < xs.len() {
            out.push((y, xs[i], xs[i + 1]));
            i += 2;
        }
    }
}

/// The point of an ellipse at `deg` degrees, as GD computes it: sine
/// and cosine scaled by 1024 and truncated, the offsets truncated toward
/// the centre (imagearc_basic.png, imagegammacorrect_basic.png).
pub fn arc_point(cx: i32, cy: i32, w: i32, h: i32, deg: i32) -> (i32, i32) {
    let r = (deg as f64).to_radians();
    let ct = (r.cos() * 1024.0) as i64;
    let st = (r.sin() * 1024.0) as i64;
    (
        cx + ((ct * w as i64 / 2) / 1024) as i32,
        cy + ((st * h as i64 / 2) / 1024) as i32,
    )
}
