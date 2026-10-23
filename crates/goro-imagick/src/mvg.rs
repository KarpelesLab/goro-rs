//! ImageMagick's own vector rasterisation, reconstructed black-box from
//! `-debug draw` logs and per-pixel probes of ImageMagick 7.1.2 Q16:
//!
//! - Primitives become point lists: ellipses step their angle by
//!   1/(8·max(rx, ry)) radians and end exactly on the end angle; an n-point
//!   Bézier is sampled at n·max(1, ⌊extent/n⌋) uniform steps (extent the
//!   larger side of the control points' box); path arcs become cubic
//!   Béziers of at most 90° each; consecutive duplicate points are dropped.
//! - Pixel centres sit on integer coordinates. A pixel inside the polygon
//!   (by the fill rule, every subpath implicitly closed) is fully filled;
//!   outside, within distance d < 1 of an edge, it gets (1 − d)².
//! - Strokes up to 2 wide are drawn from the distance d to the path's
//!   edges (the closing edge only when the path is closed): fully within
//!   w/2 − 0.25, else (1 − (d − w/2 + 0.25))²; a pixel on the path is
//!   always full. Wider strokes are filled as the stroke's outline (joins
//!   and caps), with the fill's anti-aliasing.
//! - Only pixels in the primitive's box grown by half the stroke width,
//!   rounded half up, are touched (the outline of a wide stroke is not
//!   grown). Fill is composited over the image, then the stroke.

use crate::image::Image;

pub type Pt = (f64, f64);

/// A subpath: its points and whether it is closed.
#[derive(Clone, Debug, Default)]
pub struct Sub {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

/// The points of an elliptical arc (angles in degrees).
pub fn ellipse_points(cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64) -> Vec<Pt> {
    let (rx, ry) = (rx.abs(), ry.abs());
    let mut a1 = a1;
    while a1 < a0 {
        a1 += 360.0;
    }
    let (s, e) = (a0.to_radians(), a1.to_radians());
    let r = rx.max(ry);
    let mut out = Vec::new();
    if r <= 0.0 {
        out.push((cx, cy));
        return out;
    }
    let step = 1.0 / (8.0 * r);
    let mut a = s;
    while a < e {
        out.push((cx + rx * a.cos(), cy + ry * a.sin()));
        a += step;
    }
    out.push((cx + rx * e.cos(), cy + ry * e.sin()));
    out
}

/// A Bézier curve of any degree, sampled as ImageMagick samples it.
pub fn bezier_points(cp: &[Pt]) -> Vec<Pt> {
    let n = cp.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![cp[0]];
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(x, y) in cp {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    let ext = (x1 - x0).max(y1 - y0);
    let k = ((ext / n as f64) as usize).max(1);
    let seg = n * k;
    let mut out = Vec::with_capacity(seg + 1);
    let mut q = vec![(0.0, 0.0); n];
    for i in 0..=seg {
        let t = i as f64 / seg as f64;
        q.copy_from_slice(cp);
        for m in (1..n).rev() {
            for j in 0..m {
                q[j] = (
                    q[j].0 + (q[j + 1].0 - q[j].0) * t,
                    q[j].1 + (q[j + 1].1 - q[j].1) * t,
                );
            }
        }
        out.push(q[0]);
    }
    out
}

/// An SVG elliptical arc as cubic Béziers of at most a quarter turn.
#[allow(clippy::too_many_arguments)]
pub fn arc_beziers(
    p1: Pt,
    rx: f64,
    ry: f64,
    phi_deg: f64,
    large: bool,
    sweep: bool,
    p2: Pt,
) -> Vec<[Pt; 4]> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 || p1 == p2 {
        return vec![[p1, p1, p2, p2]];
    }
    let phi = phi_deg.to_radians();
    let (c, s) = (phi.cos(), phi.sin());
    let (dx, dy) = ((p1.0 - p2.0) / 2.0, (p1.1 - p2.1) / 2.0);
    let x1p = c * dx + s * dy;
    let y1p = -s * dx + c * dy;
    let lam = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut f = if den != 0.0 {
        (num / den).max(0.0).sqrt()
    } else {
        0.0
    };
    if large == sweep {
        f = -f;
    }
    let cxp = f * rx * y1p / ry;
    let cyp = -f * ry * x1p / rx;
    let cx = c * cxp - s * cyp + (p1.0 + p2.0) / 2.0;
    let cy = s * cxp + c * cyp + (p1.1 + p2.1) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dt = ang(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && dt > 0.0 {
        dt -= 2.0 * std::f64::consts::PI;
    }
    if sweep && dt < 0.0 {
        dt += 2.0 * std::f64::consts::PI;
    }
    let nseg = ((dt.abs() / (std::f64::consts::FRAC_PI_2 + 0.001)).ceil() as usize).max(1);
    let p = |a: f64| {
        (
            cx + rx * a.cos() * c - ry * a.sin() * s,
            cy + rx * a.cos() * s + ry * a.sin() * c,
        )
    };
    let d = |a: f64| {
        (
            -rx * a.sin() * c - ry * a.cos() * s,
            -rx * a.sin() * s + ry * a.cos() * c,
        )
    };
    (0..nseg)
        .map(|i| {
            let a1 = t1 + dt * i as f64 / nseg as f64;
            let a2 = t1 + dt * (i + 1) as f64 / nseg as f64;
            let h = 4.0 / 3.0 * ((a2 - a1) / 4.0).tan();
            let (q1, q2, d1, d2) = (p(a1), p(a2), d(a1), d(a2));
            [
                q1,
                (q1.0 + h * d1.0, q1.1 + h * d1.1),
                (q2.0 - h * d2.0, q2.1 - h * d2.1),
                q2,
            ]
        })
        .collect()
}

/// Line join / cap codes as the graphic context stores them.
#[derive(Clone, Copy, Debug)]
pub struct StrokeStyle {
    pub color: [f64; 4],
    pub width: f64,
    /// 1 miter, 2 round, 3 bevel
    pub join: i32,
    /// 1 butt, 2 round, 3 square
    pub cap: i32,
    pub miter_limit: f64,
}

/// Split subpaths into their dashes (device-space lengths).
pub fn dash_subs(subs: &[Sub], pattern: &[f64], offset: f64) -> Vec<Sub> {
    let pat: Vec<f64> = if pattern.len() % 2 == 1 {
        pattern.iter().chain(pattern.iter()).copied().collect()
    } else {
        pattern.to_vec()
    };
    let total: f64 = pat.iter().sum();
    let mut out = Vec::new();
    if total <= 0.0 {
        return subs.to_vec();
    }
    for s in subs {
        let mut pts = s.pts.clone();
        if s.closed && pts.len() > 1 && pts[0] != pts[pts.len() - 1] {
            pts.push(pts[0]);
        }
        // where in the pattern the subpath starts
        let mut k = 0usize;
        let mut left = pat[0];
        let mut skip = offset.rem_euclid(total);
        while skip > 0.0 {
            if skip >= left {
                skip -= left;
                k = (k + 1) % pat.len();
                left = pat[k];
            } else {
                left -= skip;
                skip = 0.0;
            }
        }
        let mut cur: Option<Vec<Pt>> = if k.is_multiple_of(2) {
            Some(vec![pts[0]])
        } else {
            None
        };
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            let mut t = 0.0;
            while len - t > 1e-12 {
                let step = left.min(len - t);
                t += step;
                left -= step;
                let p = (a.0 + (b.0 - a.0) * t / len, a.1 + (b.1 - a.1) * t / len);
                if let Some(c) = cur.as_mut() {
                    c.push(p);
                }
                if left <= 1e-12 {
                    // the dash or gap ends here
                    if let Some(c) = cur.take() {
                        out.push(Sub {
                            pts: c,
                            closed: false,
                        });
                    }
                    k = (k + 1) % pat.len();
                    left = pat[k];
                    if k.is_multiple_of(2) {
                        cur = Some(vec![p]);
                    }
                }
            }
        }
        if let Some(c) = cur {
            if c.len() > 1 {
                out.push(Sub {
                    pts: c,
                    closed: false,
                });
            }
        }
    }
    out
}

fn seg_dist(px: f64, py: f64, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = dx * dx + dy * dy;
    let t = if l > 0.0 {
        (((px - a.0) * dx + (py - a.1) * dy) / l).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - (a.0 + t * dx)).hypot(py - (a.1 + t * dy))
}

fn rnd(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

/// Composite `color` (Q16 RGBA) at coverage `a` over the pixel.
fn over(img: &mut Image, x: usize, y: usize, color: &[f64; 4], a: f64) {
    if a <= 0.0 {
        return;
    }
    let sa = a * color[3] / 65535.0;
    if sa <= 0.0 {
        return;
    }
    let i = (y * img.columns + x) * 4;
    let p = &mut img.pixels[i..i + 4];
    let da = if img.alpha {
        p[3] as f64 / 65535.0
    } else {
        1.0
    };
    let ra = sa + da * (1.0 - sa);
    for c in 0..3 {
        let v = if ra > 0.0 {
            (sa * color[c] + da * (1.0 - sa) * p[c] as f64) / ra
        } else {
            0.0
        };
        p[c] = crate::color::clamp_to_quantum(v) as u16;
    }
    if img.alpha {
        p[3] = crate::color::clamp_to_quantum(ra * 65535.0) as u16;
    }
}

struct Window {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
}

impl Window {
    fn new(img: &Image, x0: i64, y0: i64, x1: i64, y1: i64) -> Option<Window> {
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(img.columns as i64 - 1);
        let y1 = y1.min(img.rows as i64 - 1);
        if x1 < x0 || y1 < y0 {
            return None;
        }
        Some(Window {
            x0,
            y0,
            w: (x1 - x0 + 1) as usize,
            h: (y1 - y0 + 1) as usize,
        })
    }
}

/// Minimum distances from the window's pixels to the segments, each
/// segment touching only pixels within `reach` of it.
fn distances(win: &Window, segs: &[(Pt, Pt)], reach: f64) -> Vec<f64> {
    let mut d = vec![f64::INFINITY; win.w * win.h];
    for &(a, b) in segs {
        let bx0 = ((a.0.min(b.0) - reach).floor() as i64).max(win.x0);
        let bx1 = ((a.0.max(b.0) + reach).ceil() as i64).min(win.x0 + win.w as i64 - 1);
        let by0 = ((a.1.min(b.1) - reach).floor() as i64).max(win.y0);
        let by1 = ((a.1.max(b.1) + reach).ceil() as i64).min(win.y0 + win.h as i64 - 1);
        for y in by0..=by1 {
            let row = (y - win.y0) as usize * win.w;
            for x in bx0..=bx1 {
                let k = row + (x - win.x0) as usize;
                let v = seg_dist(x as f64, y as f64, a, b);
                if v < d[k] {
                    d[k] = v;
                }
            }
        }
    }
    d
}

/// Whether each pixel of the window is inside the closed subpaths.
fn insideness(win: &Window, segs: &[(Pt, Pt)], nonzero: bool) -> Vec<bool> {
    let mut out = vec![false; win.w * win.h];
    let mut cross: Vec<(f64, i32)> = Vec::new();
    for yy in 0..win.h {
        let py = (win.y0 + yy as i64) as f64;
        cross.clear();
        for &((x1, y1), (x2, y2)) in segs {
            let dir = if y1 <= py && y2 > py {
                1
            } else if y2 <= py && y1 > py {
                -1
            } else {
                continue;
            };
            let xi = x1 + (py - y1) * (x2 - x1) / (y2 - y1);
            cross.push((xi, dir));
        }
        if cross.is_empty() {
            continue;
        }
        cross.sort_by(|a, b| a.0.total_cmp(&b.0));
        // winding of a pixel: the crossings to its right
        let mut wn: i32 = cross.iter().map(|c| c.1).sum();
        let mut parity = cross.len() % 2 == 1;
        let mut k = 0;
        for xx in 0..win.w {
            let px = (win.x0 + xx as i64) as f64;
            while k < cross.len() && cross[k].0 <= px {
                wn -= cross[k].1;
                parity = !parity;
                k += 1;
            }
            out[yy * win.w + xx] = if nonzero { wn != 0 } else { parity };
        }
    }
    out
}

fn dedupe(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

/// Draw the subpaths (device coordinates): the fill, then the stroke.
pub fn draw(
    img: &mut Image,
    subs: &[Sub],
    fill: Option<([f64; 4], bool)>,
    stroke: Option<StrokeStyle>,
    stroke_width: f64,
) {
    draw_dashed(img, subs, fill, stroke, stroke_width, None)
}

/// [`draw`] with a dash pattern (device lengths, offset): every dash is
/// stroked as its outline, whatever the width.
pub fn draw_dashed(
    img: &mut Image,
    subs: &[Sub],
    fill: Option<([f64; 4], bool)>,
    stroke: Option<StrokeStyle>,
    stroke_width: f64,
    dash: Option<(&[f64], f64)>,
) {
    let subs: Vec<Sub> = subs
        .iter()
        .map(|s| Sub {
            pts: dedupe(&s.pts),
            closed: s.closed,
        })
        .filter(|s| !s.pts.is_empty())
        .collect();
    if subs.is_empty() || img.columns == 0 || img.rows == 0 {
        return;
    }
    let mid = stroke_width.max(0.0) / 2.0;
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for s in &subs {
        for &(x, y) in &s.pts {
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    let Some(win) = Window::new(
        img,
        rnd(x0 - mid),
        rnd(y0 - mid),
        rnd(x1 + mid),
        rnd(y1 + mid),
    ) else {
        return;
    };
    let mut all = Vec::new();
    let mut stroked = Vec::new();
    for s in &subs {
        let p = &s.pts;
        for i in 0..p.len().saturating_sub(1) {
            all.push((p[i], p[i + 1]));
            stroked.push((p[i], p[i + 1]));
        }
        if p.len() == 1 {
            all.push((p[0], p[0]));
            stroked.push((p[0], p[0]));
        } else if p[0] != p[p.len() - 1] {
            all.push((p[p.len() - 1], p[0]));
            if s.closed {
                stroked.push((p[p.len() - 1], p[0]));
            }
        }
    }
    let thin = stroke.filter(|s| dash.is_none() && s.width / 2.0 <= 1.0 && s.color[3] > 0.0);
    let fill = fill.filter(|f| f.0[3] > 0.0);
    if fill.is_some() || thin.is_some() {
        let fa: Option<Vec<f64>> = fill.map(|(_, nonzero)| {
            let d = distances(&win, &all, 1.0);
            let ins = insideness(&win, &all, nonzero);
            d.iter()
                .zip(&ins)
                .map(|(&d, &i)| {
                    if i || d == 0.0 {
                        1.0
                    } else if d < 1.0 {
                        (1.0 - d) * (1.0 - d)
                    } else {
                        0.0
                    }
                })
                .collect()
        });
        let sa: Option<Vec<f64>> = thin.map(|st| {
            let hw = st.width / 2.0 - 0.25;
            let d = distances(&win, &stroked, hw.max(0.0) + 1.0);
            d.iter()
                .map(|&d| {
                    let x = d - hw;
                    if x <= 0.0 || d == 0.0 {
                        1.0
                    } else if x < 1.0 {
                        (1.0 - x) * (1.0 - x)
                    } else {
                        0.0
                    }
                })
                .collect()
        });
        for yy in 0..win.h {
            for xx in 0..win.w {
                let k = yy * win.w + xx;
                let (x, y) = (win.x0 as usize + xx, win.y0 as usize + yy);
                if let (Some(f), Some((c, _))) = (&fa, &fill) {
                    over(img, x, y, c, f[k]);
                }
                if let (Some(s), Some(st)) = (&sa, &thin) {
                    over(img, x, y, &st.color, s[k]);
                }
            }
        }
    }
    if let Some(st) = stroke.filter(|s| s.color[3] > 0.0) {
        if let Some((pat, off)) = dash {
            let dashes = dash_subs(&subs, pat, off);
            stroke_outline(img, &dashes, &st);
        } else if st.width / 2.0 > 1.0 {
            stroke_outline(img, &subs, &st);
        }
    }
    img.colors = None;
}

enum Piece {
    Poly(Vec<Pt>),
    Disc(Pt, f64),
}

fn piece_dist(p: &Piece, x: f64, y: f64) -> f64 {
    match p {
        Piece::Disc(c, r) => ((x - c.0).hypot(y - c.1) - r).max(0.0),
        Piece::Poly(v) => {
            let n = v.len();
            let mut sign = 0i32;
            let mut inside = n >= 3;
            let mut d = f64::INFINITY;
            for i in 0..n {
                let (a, b) = (v[i], v[(i + 1) % n]);
                let cr = (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
                let s = if cr > 0.0 {
                    1
                } else if cr < 0.0 {
                    -1
                } else {
                    0
                };
                if s != 0 {
                    if sign == 0 {
                        sign = s;
                    } else if s != sign {
                        inside = false;
                    }
                }
                d = d.min(seg_dist(x, y, a, b));
            }
            if inside { 0.0 } else { d }
        }
    }
}

/// A wide stroke: the union of its segment bodies, joins and caps, with
/// the fill's anti-aliasing, over its own box.
fn stroke_outline(img: &mut Image, subs: &[Sub], st: &StrokeStyle) {
    let hw = st.width / 2.0;
    let mut pieces: Vec<Piece> = Vec::new();
    for s in subs {
        let mut p = s.pts.clone();
        let closed = s.closed || (p.len() > 2 && p[0] == p[p.len() - 1]);
        if closed && p.len() > 1 && p[0] == p[p.len() - 1] {
            p.pop();
        }
        let n = p.len();
        if n == 1 {
            match st.cap {
                2 => pieces.push(Piece::Disc(p[0], hw)),
                3 => {
                    let (x, y) = p[0];
                    pieces.push(Piece::Poly(vec![
                        (x - hw, y - hw),
                        (x + hw, y - hw),
                        (x + hw, y + hw),
                        (x - hw, y + hw),
                    ]));
                }
                _ => {}
            }
            continue;
        }
        let nseg = if closed { n } else { n - 1 };
        let dir = |i: usize| {
            let (a, b) = (p[i % n], p[(i + 1) % n]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = dx.hypot(dy);
            (dx / l, dy / l)
        };
        for i in 0..nseg {
            let (a, b) = (p[i % n], p[(i + 1) % n]);
            let (ux, uy) = dir(i);
            let (nx, ny) = (-uy * hw, ux * hw);
            let (mut a, mut b) = (a, b);
            if !closed && st.cap == 3 {
                if i == 0 {
                    a = (a.0 - ux * hw, a.1 - uy * hw);
                }
                if i == nseg - 1 {
                    b = (b.0 + ux * hw, b.1 + uy * hw);
                }
            }
            pieces.push(Piece::Poly(vec![
                (a.0 + nx, a.1 + ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
                (a.0 - nx, a.1 - ny),
            ]));
        }
        // joins at the interior vertices (all of them when closed)
        let joins: Vec<usize> = if closed {
            (0..n).collect()
        } else {
            (1..n - 1).collect()
        };
        for &j in &joins {
            let (ix, iy) = dir((j + n - 1) % n);
            let (ox, oy) = dir(j % n);
            let v = p[j];
            let cross = ix * oy - iy * ox;
            if cross.abs() < 1e-12 && ix * ox + iy * oy > 0.0 {
                continue;
            }
            if st.join == 2 {
                pieces.push(Piece::Disc(v, hw));
                continue;
            }
            // the outer side of the turn
            let sgn = if cross > 0.0 { -1.0 } else { 1.0 };
            let a = (v.0 - iy * hw * sgn, v.1 + ix * hw * sgn);
            let b = (v.0 - oy * hw * sgn, v.1 + ox * hw * sgn);
            let cos_t = (ix * ox + iy * oy).clamp(-1.0, 1.0);
            // miter length over half width: 1/sin(θ/2), θ the angle between
            // the segments
            let half = ((1.0 + cos_t) / 2.0).max(0.0).sqrt(); // cos of half the turn
            let ratio = if half > 1e-12 {
                1.0 / half
            } else {
                f64::INFINITY
            };
            if st.join == 3 || ratio > st.miter_limit {
                pieces.push(Piece::Poly(vec![v, a, b]));
            } else {
                let (bx, by) = ((a.0 + b.0) / 2.0 - v.0, (a.1 + b.1) / 2.0 - v.1);
                let bl = bx.hypot(by);
                let tip = if bl > 0.0 {
                    let m = hw * ratio;
                    (v.0 + bx / bl * m, v.1 + by / bl * m)
                } else {
                    a
                };
                pieces.push(Piece::Poly(vec![v, a, tip, b]));
            }
        }
        if !closed && st.cap == 2 {
            pieces.push(Piece::Disc(p[0], hw));
            pieces.push(Piece::Disc(p[n - 1], hw));
        }
    }
    if pieces.is_empty() {
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for pc in &pieces {
        match pc {
            Piece::Poly(v) => {
                for &(x, y) in v {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
            Piece::Disc(c, r) => {
                x0 = x0.min(c.0 - r);
                x1 = x1.max(c.0 + r);
                y0 = y0.min(c.1 - r);
                y1 = y1.max(c.1 + r);
            }
        }
    }
    let Some(win) = Window::new(img, rnd(x0), rnd(y0), rnd(x1), rnd(y1)) else {
        return;
    };
    let mut d = vec![f64::INFINITY; win.w * win.h];
    for pc in &pieces {
        let (bx0, bx1, by0, by1) = match pc {
            Piece::Poly(v) => v
                .iter()
                .fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |acc, &(x, y)| {
                    (acc.0.min(x), acc.1.max(x), acc.2.min(y), acc.3.max(y))
                }),
            Piece::Disc(c, r) => (c.0 - r, c.0 + r, c.1 - r, c.1 + r),
        };
        let bx0 = ((bx0 - 1.0).floor() as i64).max(win.x0);
        let bx1 = ((bx1 + 1.0).ceil() as i64).min(win.x0 + win.w as i64 - 1);
        let by0 = ((by0 - 1.0).floor() as i64).max(win.y0);
        let by1 = ((by1 + 1.0).ceil() as i64).min(win.y0 + win.h as i64 - 1);
        for y in by0..=by1 {
            for x in bx0..=bx1 {
                let k = (y - win.y0) as usize * win.w + (x - win.x0) as usize;
                if d[k] == 0.0 {
                    continue;
                }
                let v = piece_dist(pc, x as f64, y as f64);
                if v < d[k] {
                    d[k] = v;
                }
            }
        }
    }
    for yy in 0..win.h {
        for xx in 0..win.w {
            let v = d[yy * win.w + xx];
            let a = if v <= 0.0 {
                1.0
            } else if v < 1.0 {
                (1.0 - v) * (1.0 - v)
            } else {
                0.0
            };
            over(
                img,
                win.x0 as usize + xx,
                win.y0 as usize + yy,
                &st.color,
                a,
            );
        }
    }
}

/// Exact area coverage of closed outlines (line segments, device space,
/// pixel (x, y) covering [x, x+1]×[y, y+1]) over a `w`×`h` grid at
/// (x0, y0), by signed-area accumulation (non-zero, non-overlapping).
pub fn area_coverage(segs: &[(Pt, Pt)], x0: i64, y0: i64, w: usize, h: usize) -> Vec<f64> {
    let mut acc = vec![0f64; w * h + 2];
    let stride = w;
    for &(a, b) in segs {
        let (p0, p1) = (
            (a.0 - x0 as f64, a.1 - y0 as f64),
            (b.0 - x0 as f64, b.1 - y0 as f64),
        );
        if p0.1 == p1.1 {
            continue;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 {
            (1.0, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        let ys = p0.1.max(0.0);
        if p0.1 < 0.0 {
            x -= p0.1 * dxdy;
        }
        let ye = p1.1.min(h as f64);
        if ys >= ye {
            continue;
        }
        let mut y = ys as usize;
        while (y as f64) < ye {
            let linestart = y * stride;
            let dy = ((y + 1) as f64).min(ye) - (y as f64).max(ys);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let xa_f = xa.floor();
            let xai = xa_f as i64;
            let xb_c = xb.ceil();
            let xbi = xb_c as i64;
            let put = |acc: &mut Vec<f64>, xi: i64, v: f64| {
                if xi < 0 {
                    acc[linestart] += v;
                } else if (xi as usize) < stride {
                    acc[linestart + xi as usize] += v;
                } else {
                    // right of the grid: no effect on it
                }
            };
            if xbi <= xai + 1 {
                let xmf = 0.5 * (x + xnext) - xa_f;
                put(&mut acc, xai, d - d * xmf);
                put(&mut acc, xai + 1, d * xmf);
            } else {
                let s = 1.0 / (xb - xa);
                let x0f = xa - xa_f;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = xb - xb_c + 1.0;
                let am = 0.5 * s * x1f * x1f;
                put(&mut acc, xai, d * a0);
                if xbi == xai + 2 {
                    put(&mut acc, xai + 1, d * (1.0 - a0 - am));
                } else {
                    let a1 = s * (1.5 - x0f);
                    put(&mut acc, xai + 1, d * (a1 - a0));
                    for xi in xai + 2..xbi - 1 {
                        put(&mut acc, xi, d * s);
                    }
                    let a2 = a1 + (xbi - xai - 3) as f64 * s;
                    put(&mut acc, xbi - 1, d * (1.0 - a2 - am));
                }
                put(&mut acc, xbi, d * am);
            }
            x = xnext;
            y += 1;
        }
    }
    let mut out = vec![0f64; w * h];
    for yy in 0..h {
        let mut s = 0f64;
        for xx in 0..w {
            s += acc[yy * stride + xx];
            out[yy * w + xx] = s.abs().min(1.0);
        }
    }
    out
}

/// Composite a coverage grid (8-bit levels, as FreeType renders them) of
/// `color` over the image.
pub fn composite_coverage(
    img: &mut Image,
    cov: &[f64],
    x0: i64,
    y0: i64,
    w: usize,
    color: &[f64; 4],
) {
    let h = cov.len().checked_div(w).unwrap_or(0);
    for yy in 0..h {
        let y = y0 + yy as i64;
        if y < 0 || y >= img.rows as i64 {
            continue;
        }
        for xx in 0..w {
            let x = x0 + xx as i64;
            if x < 0 || x >= img.columns as i64 {
                continue;
            }
            let level = (cov[yy * w + xx] * 256.0 + 1e-9).floor().min(255.0);
            if level > 0.0 {
                over(img, x as usize, y as usize, color, level / 255.0);
            }
        }
    }
}
