//! TrueType / OpenType text: imagettftext, imagettfbbox, imagefttext,
//! imageftbbox (PHP manual). Outlines come from the oxideav-ttf font
//! parser (TrueType `glyf` and CFF); they are scaled at GD's 96 dpi,
//! laid out with the fonts' advances and kerning, and rasterized here
//! with an antialiasing coverage rasterizer.
//!
//! Behaviour sources: the PHP manual pages (the bounding box is the
//! eight corner coordinates lower-left, lower-right, upper-right,
//! upper-left; angles in degrees counter-clockwise; the `linespacing`
//! option), the TrueType/OpenType specification (hhea metrics), and the
//! ext/gd tests' expected boxes (bug53504, bug43073, bug48732: lines are
//! 1.05 times the font's height apart).

use crate::image::*;

/// A text request.
pub struct TextReq<'a> {
    pub font: &'a [u8],
    pub size: f64,
    pub angle: f64,
    pub x: i32,
    pub y: i32,
    pub color: i32,
    pub text: &'a [u8],
    pub linespacing: f64,
}

const DPI: f64 = 96.0;
const LINESPACE: f64 = 1.05;

/// Decodes UTF-8 text with HTML numeric entities (`&#65;`, `&#x41;`) as
/// GD does; invalid bytes are read as Latin-1.
fn decode_text(t: &[u8]) -> Vec<char> {
    let mut out = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if t[i] == b'&' && t.get(i + 1) == Some(&b'#') {
            let (hex, start) = if matches!(t.get(i + 2), Some(b'x') | Some(b'X')) {
                (true, i + 3)
            } else {
                (false, i + 2)
            };
            let mut j = start;
            while j < t.len() && j - start < 8 && (t[j] as char).is_digit(if hex { 16 } else { 10 }) {
                j += 1;
            }
            if j > start && t.get(j) == Some(&b';') {
                let s = std::str::from_utf8(&t[start..j]).unwrap_or("0");
                if let Ok(v) = u32::from_str_radix(s, if hex { 16 } else { 10 })
                    && let Some(c) = char::from_u32(v)
                {
                    out.push(c);
                    i = j + 1;
                    continue;
                }
            }
        }
        let b = t[i];
        let len = match b {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        };
        if len > 1
            && let Some(s) = t.get(i..i + len)
            && let Ok(s) = std::str::from_utf8(s)
            && let Some(c) = s.chars().next()
        {
            out.push(c);
            i += len;
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// The hhea table's ascender, descender and line gap.
fn hhea_metrics(d: &[u8]) -> Option<(f64, f64, f64)> {
    let n = u16::from_be_bytes([*d.get(4)?, *d.get(5)?]) as usize;
    for i in 0..n {
        let r = d.get(12 + 16 * i..28 + 16 * i)?;
        if &r[..4] == b"hhea" {
            let off = u32::from_be_bytes(r[8..12].try_into().ok()?) as usize;
            let t = d.get(off..off + 10)?;
            let g = |k: usize| i16::from_be_bytes([t[k], t[k + 1]]) as f64;
            return Some((g(4), g(6), g(8)));
        }
    }
    None
}

/// A segment of a flattened outline, in pixel coordinates (y down).
type Seg = ((f64, f64), (f64, f64));

/// Flattens a TrueType-style contour (on/off-curve points, quadratic)
/// through `tf` into segments.
fn flatten(points: &[(f64, f64, bool)], out: &mut Vec<Seg>) {
    let n = points.len();
    if n < 2 {
        return;
    }
    // Start at an on-curve point (or the midpoint of the first two
    // off-curve points).
    let start_idx = points.iter().position(|p| p.2);
    let (start, rot): ((f64, f64), usize) = match start_idx {
        Some(i) => ((points[i].0, points[i].1), i),
        None => (
            (
                (points[0].0 + points[1].0) / 2.0,
                (points[0].1 + points[1].1) / 2.0,
            ),
            0,
        ),
    };
    let mut cur = start;
    let mut ctrl: Option<(f64, f64)> = None;
    let mut quad = |p0: (f64, f64), c: (f64, f64), p1: (f64, f64), out: &mut Vec<Seg>| {
        let dd = ((p0.0 - 2.0 * c.0 + p1.0).abs() + (p0.1 - 2.0 * c.1 + p1.1).abs()).max(0.0);
        let steps = ((dd * 2.0).sqrt().ceil() as usize).clamp(1, 64);
        let mut prev = p0;
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            let mt = 1.0 - t;
            let q = (
                mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0,
                mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1,
            );
            out.push((prev, q));
            prev = q;
        }
    };
    for k in 1..=n {
        let p = points[(rot + k) % n];
        let pt = (p.0, p.1);
        if p.2 {
            match ctrl.take() {
                Some(c) => quad(cur, c, pt, out),
                None => out.push((cur, pt)),
            }
            cur = pt;
        } else {
            if let Some(c) = ctrl {
                let mid = ((c.0 + pt.0) / 2.0, (c.1 + pt.1) / 2.0);
                quad(cur, c, mid, out);
                cur = mid;
            }
            ctrl = Some(pt);
        }
    }
    if let Some(c) = ctrl {
        quad(cur, c, start, out);
    } else if cur != start {
        out.push((cur, start));
    }
}

/// Scanline rasterizer with non-zero winding: each pixel row is sampled
/// on `SUB` sub-scanlines; spans between crossings add their exact
/// horizontal overlap with each pixel.
struct Raster {
    w: usize,
    h: usize,
    segs: Vec<Seg>,
}

const SUB: usize = 5;

impl Raster {
    fn new(w: usize, h: usize) -> Raster {
        Raster {
            w,
            h,
            segs: Vec::new(),
        }
    }

    fn line(&mut self, p0: (f64, f64), p1: (f64, f64)) {
        if (p0.1 - p1.1).abs() > f64::EPSILON {
            self.segs.push((p0, p1));
        }
    }

    /// Coverage 0..=1 per pixel.
    fn coverage(&self) -> Vec<f32> {
        let mut out = vec![0.0f32; self.w * self.h];
        let mut xs: Vec<(f64, i32)> = Vec::new();
        let wf = self.w as f64;
        for row in 0..self.h {
            for sub in 0..SUB {
                let y = row as f64 + (sub as f64 + 0.5) / SUB as f64;
                xs.clear();
                for &((x0, y0), (x1, y1)) in &self.segs {
                    let (lo, hi, dir) = if y0 < y1 { (y0, y1, 1) } else { (y1, y0, -1) };
                    if y >= lo && y < hi {
                        let x = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                        xs.push((x, dir));
                    }
                }
                if xs.is_empty() {
                    continue;
                }
                xs.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut wind = 0;
                let base = row * self.w;
                for i in 0..xs.len() {
                    let before = wind;
                    wind += xs[i].1;
                    if before == 0 && wind != 0 {
                        // Span start; find its end.
                        let a = xs[i].0;
                        let mut w2 = wind;
                        let mut j = i + 1;
                        while j < xs.len() {
                            w2 += xs[j].1;
                            if w2 == 0 {
                                break;
                            }
                            j += 1;
                        }
                        let b = if j < xs.len() { xs[j].0 } else { wf };
                        let (a, b) = (a.max(0.0), b.min(wf));
                        if b <= a {
                            continue;
                        }
                        let (ai, bi) = (a.floor() as usize, (b.ceil() as usize).min(self.w));
                        for px in ai..bi {
                            let l = (px as f64).max(a);
                            let r = (px as f64 + 1.0).min(b);
                            if r > l {
                                out[base + px] += ((r - l) / SUB as f64) as f32;
                            }
                        }
                    }
                }
            }
        }
        for v in out.iter_mut() {
            *v = v.min(1.0);
        }
        out
    }
}

struct Glyph {
    /// Contours in pixel units relative to the pen (y up).
    contours: Vec<Vec<(f64, f64, bool)>>,
    /// Box (xmin, ymin, xmax, ymax), pixel units relative to the pen,
    /// rounded outward.
    bbox: Option<(f64, f64, f64, f64)>,
}

/// Lays out and optionally draws the text. Returns the bounding box.
pub fn text(im: Option<&mut Image>, req: &TextReq) -> Result<[i32; 8], String> {
    let font = oxideav_ttf::Font::from_bytes(req.font).map_err(|_| "Could not read font".to_string())?;
    let upem = font.units_per_em().max(1) as f64;
    let ppem = req.size * DPI / 72.0;
    let scale = ppem / upem;
    // Line height from the hhea metrics (ascender - descender + lineGap).
    let (asc, desc, gap) = hhea_metrics(req.font).unwrap_or((
        font.ascent() as f64,
        font.descent() as f64,
        font.line_gap() as f64,
    ));
    // Pixel metrics: ascender rounded up, descender down, gap rounded
    // (bug48732: 12pt Tuffy lines are 22 pixels apart).
    let height = (asc * scale).ceil() + (-desc * scale).ceil() + (gap * scale).round();
    // Pair kerning from the 'kern' table only.
    let has_kern = font.kern_header_variant().is_some();
    let line_h = height * LINESPACE * req.linespacing;
    let chars = decode_text(req.text);
    // Pen positions and glyphs.
    let mut placed: Vec<(f64, f64, Glyph)> = Vec::new();
    let mut pen_x = 0.0f64;
    let mut pen_y = 0.0f64;
    let mut prev: Option<u16> = None;
    let mut bbox: Option<(f64, f64, f64, f64)> = None;
    for &c in &chars {
        if c == '\r' {
            continue;
        }
        if c == '\n' {
            pen_x = 0.0;
            pen_y -= line_h;
            prev = None;
            continue;
        }
        let gid = font.glyph_index(c).unwrap_or(0);
        if let Some(p) = prev
            && has_kern
        {
            let k = font.lookup_kerning(p, gid) as f64 * scale;
            pen_x += k.round();
        }
        let outline = font.glyph_outline(gid).unwrap_or_default();
        let contours: Vec<Vec<(f64, f64, bool)>> = outline
            .contours
            .iter()
            .map(|ct| {
                ct.points
                    .iter()
                    .map(|p| (p.x as f64 * scale, p.y as f64 * scale, p.on_curve))
                    .collect()
            })
            .collect();
        let mut gb: Option<(f64, f64, f64, f64)> = None;
        for ct in &contours {
            for &(x, y, _) in ct {
                gb = Some(match gb {
                    None => (x, y, x, y),
                    Some((a, b, c2, d)) => (a.min(x), b.min(y), c2.max(x), d.max(y)),
                });
            }
        }
        let gb = gb.map(|(a, b, c2, d)| (a.floor(), b.floor(), c2.ceil(), d.ceil()));
        if let Some((a, b, c2, d)) = gb {
            let g = (a + pen_x, b + pen_y, c2 + pen_x, d + pen_y);
            bbox = Some(match bbox {
                None => g,
                Some((p, q, r, s)) => (p.min(g.0), q.min(g.1), r.max(g.2), s.max(g.3)),
            });
        }
        placed.push((pen_x, pen_y, Glyph { contours, bbox: gb }));
        pen_x += (font.glyph_advance(gid) as f64 * scale).round();
        prev = Some(gid);
    }
    let (xmin, ymin, xmax, ymax) = bbox.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let rad = req.angle.to_radians();
    let (s, c) = rad.sin_cos();
    let rot = |x: f64, y: f64| -> (f64, f64) { (x * c - y * s, x * s + y * c) };
    let corners = [(xmin, ymin), (xmax, ymin), (xmax, ymax), (xmin, ymax)];
    let mut out = [0i32; 8];
    for (i, &(x, y)) in corners.iter().enumerate() {
        let (rx, ry) = rot(x, y);
        out[2 * i] = req.x + rx.round() as i32;
        out[2 * i + 1] = req.y - ry.round() as i32;
    }
    if let Some(im) = im {
        draw(im, req, &placed, &rot);
    }
    Ok(out)
}

fn draw(im: &mut Image, req: &TextReq, placed: &[(f64, f64, Glyph)], rot: &dyn Fn(f64, f64) -> (f64, f64)) {
    // Segments in image coordinates.
    let mut segs: Vec<Seg> = Vec::new();
    for (px, py, g) in placed {
        if g.bbox.is_none() {
            continue;
        }
        for ct in &g.contours {
            let pts: Vec<(f64, f64, bool)> = ct
                .iter()
                .map(|&(x, y, on)| {
                    let (rx, ry) = rot(x + px, y + py);
                    (req.x as f64 + rx, req.y as f64 - ry, on)
                })
                .collect();
            flatten(&pts, &mut segs);
        }
    }
    if segs.is_empty() {
        return;
    }
    let mut minx = f64::MAX;
    let mut miny = f64::MAX;
    let mut maxx = f64::MIN;
    let mut maxy = f64::MIN;
    for ((x0, y0), (x1, y1)) in &segs {
        minx = minx.min(*x0).min(*x1);
        maxx = maxx.max(*x0).max(*x1);
        miny = miny.min(*y0).min(*y1);
        maxy = maxy.max(*y0).max(*y1);
    }
    let ox = minx.floor().max(im.cx1 as f64).max(-1.0);
    let oy = miny.floor().max(im.cy1 as f64).max(-1.0);
    let ex = (maxx.ceil() + 1.0).min(im.cx2 as f64 + 1.0);
    let ey = (maxy.ceil() + 1.0).min(im.cy2 as f64 + 1.0);
    if ex <= ox || ey <= oy {
        return;
    }
    let (w, h) = ((ex - ox) as usize + 2, (ey - oy) as usize);
    if w * h > 64 << 20 {
        return;
    }
    let mut r = Raster::new(w, h);
    for &((x0, y0), (x1, y1)) in &segs {
        r.line((x0 - ox, y0 - oy), (x1 - ox, y1 - oy));
    }
    let cov = r.coverage();
    let color = req.color;
    let (fr, fg, fb, fa) = if im.truecolor {
        (tc_r(color), tc_g(color), tc_b(color), tc_a(color))
    } else {
        let i = (color & 0xff) as usize;
        (im.red[i], im.green[i], im.blue[i], im.alpha[i])
    };
    for yy in 0..h {
        let y = oy as i32 + yy as i32;
        for xx in 0..w {
            let x = ox as i32 + xx as i32;
            let cv = cov[yy * w + xx];
            if cv <= 0.0 || !im.in_bounds(x, y) || !im.in_clip(x, y) {
                continue;
            }
            if im.truecolor {
                let opacity = ((127 - fa) as f32 * cv).round() as i32;
                let src = tc(fr, fg, fb, 127 - opacity);
                let dst = im.get_pixel(x, y);
                let v = if cv >= 1.0 && fa == 0 { src } else { alpha_blend(dst, src) };
                im.put_raw(x, y, v);
            } else {
                if cv < 0.5 {
                    continue;
                }
                let v = if cv >= 0.99 {
                    color
                } else {
                    let d = im.get_pixel(x, y) as usize;
                    let mix = |f: i32, b: i32| (f as f32 * cv + b as f32 * (1.0 - cv)).round() as i32;
                    let (r2, g2, b2) = (mix(fr, im.red[d]), mix(fg, im.green[d]), mix(fb, im.blue[d]));
                    im.color_resolve(r2, g2, b2, fa)
                };
                im.put_raw(x, y, v);
            }
        }
    }
}
