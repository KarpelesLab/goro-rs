//! DistortImage(): the distortion methods of ImageMagick's documentation
//! ("Distorting Images"), as inverse mappings from destination to source
//! pixels, resampled by EWA (ewa.rs) with the image's virtual pixel method.

use crate::error::{MResult, MagickError, severity};
use crate::geometry::Rect;
use crate::image::Image;

pub const AFFINE: i64 = 1;
pub const AFFINE_PROJECTION: i64 = 2;
pub const SRT: i64 = 3;
pub const PERSPECTIVE: i64 = 4;
pub const PERSPECTIVE_PROJECTION: i64 = 5;
pub const BILINEAR_FORWARD: i64 = 6;
pub const BILINEAR_REVERSE: i64 = 7;
pub const POLYNOMIAL: i64 = 8;
pub const ARC: i64 = 9;
pub const POLAR: i64 = 10;
pub const DEPOLAR: i64 = 11;
pub const CYLINDER2PLANE: i64 = 12;
pub const PLANE2CYLINDER: i64 = 13;
pub const BARREL: i64 = 14;
pub const BARREL_INVERSE: i64 = 15;
pub const SHEPARDS: i64 = 16;
pub const RESIZE: i64 = 17;
pub const RIGID_AFFINE: i64 = 19;

fn bad_args(name: &str) -> MagickError {
    MagickError::with_desc(
        severity::OPTION_ERROR,
        "InvalidArgument",
        &format!("{} : 'Needs more arguments'", name),
        "error/distort.c/GenerateCoefficients",
    )
}

/// Solve a linear system (Gauss-Jordan with partial pivoting).
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let piv =
            (col..n).max_by(|&i, &j| a[i][col].abs().partial_cmp(&a[j][col].abs()).unwrap())?;
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        for r in 0..n {
            if r != col {
                let f = a[r][col] / a[col][col];
                for c in col..n {
                    a[r][c] -= f * a[col][c];
                }
                b[r] -= f * b[col];
            }
        }
    }
    Some((0..n).map(|i| b[i] / a[i][i]).collect())
}

/// Least squares: rows of terms → target values.
fn least_squares(rows: &[Vec<f64>], vals: &[f64]) -> Option<Vec<f64>> {
    let n = rows[0].len();
    let mut ata = vec![vec![0.0; n]; n];
    let mut atb = vec![0.0; n];
    for (r, &v) in rows.iter().zip(vals) {
        for i in 0..n {
            atb[i] += r[i] * v;
            for j in 0..n {
                ata[i][j] += r[i] * r[j];
            }
        }
    }
    solve(ata, atb)
}

/// An affine map x' = a*x + b*y + c, y' = d*x + e*y + f.
#[derive(Clone, Copy, Debug)]
struct Aff([f64; 6]);

impl Aff {
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let m = &self.0;
        (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
    }
    fn inverse(&self) -> Option<Aff> {
        let m = &self.0;
        let det = m[0] * m[4] - m[1] * m[3];
        if det.abs() < 1e-12 {
            return None;
        }
        Some(Aff([
            m[4] / det,
            -m[1] / det,
            (m[1] * m[5] - m[4] * m[2]) / det,
            -m[3] / det,
            m[0] / det,
            (m[3] * m[2] - m[0] * m[5]) / det,
        ]))
    }
}

/// Control points (u, v) -> (x, y) from the argument list.
fn pairs(args: &[f64]) -> Vec<(f64, f64, f64, f64)> {
    args.chunks_exact(4)
        .map(|c| (c[0], c[1], c[2], c[3]))
        .collect()
}

/// The forward affine of the control points (source -> destination).
fn affine_from_points(p: &[(f64, f64, f64, f64)]) -> Option<Aff> {
    match p.len() {
        0 => None,
        1 => {
            let (u, v, x, y) = p[0];
            Some(Aff([1.0, 0.0, x - u, 0.0, 1.0, y - v]))
        }
        2 => {
            // scale, rotation and translation from two points
            let (u0, v0, x0, y0) = p[0];
            let (u1, v1, x1, y1) = p[1];
            let (du, dv) = (u1 - u0, v1 - v0);
            let (dx, dy) = (x1 - x0, y1 - y0);
            let d2 = du * du + dv * dv;
            if d2 == 0.0 {
                return None;
            }
            let a = (du * dx + dv * dy) / d2;
            let b = (du * dy - dv * dx) / d2;
            Some(Aff([
                a,
                -b,
                x0 - a * u0 + b * v0,
                b,
                a,
                y0 - b * u0 - a * v0,
            ]))
        }
        _ => {
            let rows: Vec<Vec<f64>> = p.iter().map(|&(u, v, _, _)| vec![u, v, 1.0]).collect();
            let xs: Vec<f64> = p.iter().map(|q| q.2).collect();
            let ys: Vec<f64> = p.iter().map(|q| q.3).collect();
            let cx = least_squares(&rows, &xs)?;
            let cy = least_squares(&rows, &ys)?;
            Some(Aff([cx[0], cx[1], cx[2], cy[0], cy[1], cy[2]]))
        }
    }
}

/// A perspective (homography) from 4+ point pairs: src -> dst.
fn perspective_from_points(p: &[(f64, f64, f64, f64)]) -> Option<[f64; 8]> {
    if p.len() < 4 {
        return None;
    }
    let mut rows = Vec::new();
    let mut vals = Vec::new();
    for &(u, v, x, y) in p {
        rows.push(vec![u, v, 1.0, 0.0, 0.0, 0.0, -u * x, -v * x]);
        vals.push(x);
        rows.push(vec![0.0, 0.0, 0.0, u, v, 1.0, -u * y, -v * y]);
        vals.push(y);
    }
    let c = least_squares(&rows, &vals)?;
    Some([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
}

fn persp_apply(c: &[f64; 8], x: f64, y: f64) -> (f64, f64) {
    let w = c[6] * x + c[7] * y + 1.0;
    if w.abs() < 1e-12 {
        return (f64::NAN, f64::NAN);
    }
    (
        (c[0] * x + c[1] * y + c[2]) / w,
        (c[3] * x + c[4] * y + c[5]) / w,
    )
}

fn persp_inverse(c: &[f64; 8]) -> Option<[f64; 8]> {
    let m = [[c[0], c[1], c[2]], [c[3], c[4], c[5]], [c[6], c[7], 1.0]];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / det,
        ],
    ];
    let s = inv[2][2];
    if s.abs() < 1e-12 {
        return None;
    }
    Some([
        inv[0][0] / s,
        inv[0][1] / s,
        inv[0][2] / s,
        inv[1][0] / s,
        inv[1][1] / s,
        inv[1][2] / s,
        inv[2][0] / s,
        inv[2][1] / s,
    ])
}

/// The polynomial terms of an order (1..=5).
fn poly_terms(order: usize, x: f64, y: f64) -> Vec<f64> {
    let mut t = Vec::new();
    for total in 0..=order {
        for j in 0..=total {
            let i = total - j;
            t.push(x.powi(i as i32) * y.powi(j as i32));
        }
    }
    t
}

/// Destination -> source mapping.
enum Map {
    Affine(Aff),
    Perspective([f64; 8]),
    Bilinear([f64; 8]),
    Poly(usize, Vec<f64>, Vec<f64>),
    Arc {
        cx: f64,
        cy: f64,
        start: f64,
        span: f64,
        r_top: f64,
        r_bottom: f64,
        w: f64,
        h: f64,
    },
    Polar {
        cx: f64,
        cy: f64,
        rmin: f64,
        rmax: f64,
        a0: f64,
        a1: f64,
        w: f64,
        h: f64,
        to_polar: bool,
        ox: f64,
        oy: f64,
    },
    Barrel {
        a: [f64; 4],
        b: [f64; 4],
        cx: f64,
        cy: f64,
        rscale: f64,
        inverse: bool,
    },
    Shepards(Vec<(f64, f64, f64, f64)>),
}

impl Map {
    /// The reverse map's partial derivatives at (x, y).
    fn jacobian(&self, x: f64, y: f64) -> [[f64; 2]; 2] {
        if let Map::Affine(a) = self {
            let m = &a.0;
            return [[m[0], m[1]], [m[3], m[4]]];
        }
        const H: f64 = 1e-4;
        let (ax, ay) = self.apply(x + H, y);
        let (bx, by) = self.apply(x - H, y);
        let (cx, cy) = self.apply(x, y + H);
        let (dx, dy) = self.apply(x, y - H);
        [
            [(ax - bx) / (2.0 * H), (cx - dx) / (2.0 * H)],
            [(ay - by) / (2.0 * H), (cy - dy) / (2.0 * H)],
        ]
    }

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        match self {
            Map::Affine(a) => a.apply(x, y),
            Map::Perspective(c) => persp_apply(c, x, y),
            Map::Bilinear(c) => (
                c[0] + c[1] * x + c[2] * y + c[3] * x * y,
                c[4] + c[5] * x + c[6] * y + c[7] * x * y,
            ),
            Map::Poly(order, cx, cy) => {
                let t = poly_terms(*order, x, y);
                (
                    t.iter().zip(cx).map(|(a, b)| a * b).sum(),
                    t.iter().zip(cy).map(|(a, b)| a * b).sum(),
                )
            }
            Map::Arc {
                cx,
                cy,
                start,
                span,
                r_top,
                r_bottom,
                w,
                h,
            } => {
                let (dx, dy) = (x - cx, y - cy);
                let r = (dx * dx + dy * dy).sqrt();
                let mut a = dy.atan2(dx) - start;
                while a < 0.0 {
                    a += 2.0 * std::f64::consts::PI;
                }
                let u = a / span * w;
                let v = (r_top - r) / (r_top - r_bottom) * h;
                (u, v)
            }
            Map::Polar {
                cx,
                cy,
                rmin,
                rmax,
                a0,
                a1,
                w,
                h,
                to_polar,
                ox,
                oy,
            } => {
                if *to_polar {
                    // destination polar picture of the source columns
                    let (dx, dy) = (x - ox, y - oy);
                    let r = (dx * dx + dy * dy).sqrt();
                    let mut a = dx.atan2(-dy) - a0;
                    while a < 0.0 {
                        a += 2.0 * std::f64::consts::PI;
                    }
                    let u = a / (a1 - a0) * w;
                    let v = (r - rmin) / (rmax - rmin) * h;
                    (u, v)
                } else {
                    // depolar: the destination rectangle unrolls the circle
                    let a = a0 + x / w * (a1 - a0);
                    let r = rmin + y / h * (rmax - rmin);
                    (cx + r * a.sin(), cy - r * a.cos())
                }
            }
            Map::Barrel {
                a,
                b,
                cx,
                cy,
                rscale,
                inverse,
            } => {
                let (dx, dy) = ((x - cx) / rscale, (y - cy) / rscale);
                let r = (dx * dx + dy * dy).sqrt();
                let fx = a[0] * r * r * r + a[1] * r * r + a[2] * r + a[3];
                let fy = b[0] * r * r * r + b[1] * r * r + b[2] * r + b[3];
                if *inverse {
                    let fx = if fx.abs() < 1e-12 { 1e-12 } else { fx };
                    let fy = if fy.abs() < 1e-12 { 1e-12 } else { fy };
                    (cx + dx / fx * rscale, cy + dy / fy * rscale)
                } else {
                    (cx + dx * fx * rscale, cy + dy * fy * rscale)
                }
            }
            Map::Shepards(p) => {
                // inverse distance weighted displacement (power 2)
                let mut wsum = 0.0;
                let (mut sx, mut sy) = (0.0, 0.0);
                for &(u, v, px, py) in p {
                    let d2 = (x - px) * (x - px) + (y - py) * (y - py);
                    if d2 < 1e-9 {
                        return (u, v);
                    }
                    let w = 1.0 / d2;
                    wsum += w;
                    sx += w * (u - px);
                    sy += w * (v - py);
                }
                if wsum > 0.0 {
                    (x + sx / wsum, y + sy / wsum)
                } else {
                    (x, y)
                }
            }
        }
    }
}

/// DistortImage(): the distorted image (and its page).
pub fn distort(img: &Image, method: i64, args: &[f64], bestfit: bool) -> MResult<Image> {
    let (w, h) = (img.columns as f64, img.rows as f64);
    let mut out_rect = Rect::new(img.columns, img.rows, 0, 0);
    // forward map of the corners, for bestfit
    let mut forward: Option<Box<dyn Fn(f64, f64) -> (f64, f64)>> = None;
    let map = match method {
        AFFINE | RIGID_AFFINE => {
            let fwd = affine_from_points(&pairs(args)).ok_or_else(|| bad_args("Affine"))?;
            let inv = fwd.inverse().ok_or_else(|| bad_args("Affine"))?;
            forward = Some(Box::new(move |x, y| fwd.apply(x, y)));
            Map::Affine(inv)
        }
        AFFINE_PROJECTION => {
            if args.len() < 6 {
                return Err(bad_args("AffineProjection"));
            }
            // sx, rx, ry, sy, tx, ty
            let fwd = Aff([args[0], args[2], args[4], args[1], args[3], args[5]]);
            let inv = fwd.inverse().ok_or_else(|| bad_args("AffineProjection"))?;
            forward = Some(Box::new(move |x, y| fwd.apply(x, y)));
            Map::Affine(inv)
        }
        SRT => {
            let a = args;
            let (cx, cy) = (w / 2.0, h / 2.0);
            let (ox, oy, sx, sy, ang, nx, ny) = match a.len() {
                1 => (cx, cy, 1.0, 1.0, a[0], cx, cy),
                2 => (cx, cy, a[0], a[0], a[1], cx, cy),
                3 => (a[0], a[1], 1.0, 1.0, a[2], a[0], a[1]),
                4 => (a[0], a[1], a[2], a[2], a[3], a[0], a[1]),
                5 => (a[0], a[1], a[2], a[3], a[4], a[0], a[1]),
                6 => (a[0], a[1], a[2], a[2], a[3], a[4], a[5]),
                n if n >= 7 => (a[0], a[1], a[2], a[3], a[4], a[5], a[6]),
                _ => return Err(bad_args("ScaleRotateTranslate")),
            };
            let (s, c) = ang.to_radians().sin_cos();
            let fwd = Aff([
                sx * c,
                -sy * s,
                nx - (sx * c * ox - sy * s * oy),
                sx * s,
                sy * c,
                ny - (sx * s * ox + sy * c * oy),
            ]);
            let inv = fwd
                .inverse()
                .ok_or_else(|| bad_args("ScaleRotateTranslate"))?;
            forward = Some(Box::new(move |x, y| fwd.apply(x, y)));
            Map::Affine(inv)
        }
        PERSPECTIVE => {
            let p = pairs(args);
            let fwd = perspective_from_points(&p).ok_or_else(|| bad_args("Perspective"))?;
            let inv = persp_inverse(&fwd).ok_or_else(|| bad_args("Perspective"))?;
            forward = Some(Box::new(move |x, y| persp_apply(&fwd, x, y)));
            Map::Perspective(inv)
        }
        PERSPECTIVE_PROJECTION => {
            if args.len() < 8 {
                return Err(bad_args("PerspectiveProjection"));
            }
            let fwd = [
                args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
            ];
            let inv = persp_inverse(&fwd).ok_or_else(|| bad_args("PerspectiveProjection"))?;
            forward = Some(Box::new(move |x, y| persp_apply(&fwd, x, y)));
            Map::Perspective(inv)
        }
        BILINEAR_FORWARD | BILINEAR_REVERSE => {
            let p = pairs(args);
            if p.len() < 4 {
                return Err(bad_args("Bilinear"));
            }
            // fit dest -> src (reverse) bilinearly
            let rows: Vec<Vec<f64>> = p
                .iter()
                .map(|&(_, _, x, y)| vec![1.0, x, y, x * y])
                .collect();
            let us: Vec<f64> = p.iter().map(|q| q.0).collect();
            let vs: Vec<f64> = p.iter().map(|q| q.1).collect();
            let cu = least_squares(&rows, &us).ok_or_else(|| bad_args("Bilinear"))?;
            let cv = least_squares(&rows, &vs).ok_or_else(|| bad_args("Bilinear"))?;
            if let Some(f) = affine_from_points(&p) {
                forward = Some(Box::new(move |x, y| f.apply(x, y)));
            }
            Map::Bilinear([cu[0], cu[1], cu[2], cu[3], cv[0], cv[1], cv[2], cv[3]])
        }
        POLYNOMIAL => {
            if args.is_empty() {
                return Err(bad_args("Polynomial"));
            }
            let mut order = (args[0].floor() as usize).clamp(1, 5);
            let p = pairs(&args[1..]);
            // too few control points for the order: a lower order
            while order > 1 && p.len() < (order + 1) * (order + 2) / 2 {
                order -= 1;
            }
            if p.len() < 3 {
                return Err(bad_args("Polynomial"));
            }
            let rows: Vec<Vec<f64>> = p
                .iter()
                .map(|&(_, _, x, y)| poly_terms(order, x, y))
                .collect();
            let us: Vec<f64> = p.iter().map(|q| q.0).collect();
            let vs: Vec<f64> = p.iter().map(|q| q.1).collect();
            let cu = least_squares(&rows, &us).ok_or_else(|| bad_args("Polynomial"))?;
            let cv = least_squares(&rows, &vs).ok_or_else(|| bad_args("Polynomial"))?;
            if let Some(f) = affine_from_points(&p) {
                forward = Some(Box::new(move |x, y| f.apply(x, y)));
            }
            Map::Poly(order, cu, cv)
        }
        ARC => {
            if args.is_empty() {
                return Err(bad_args("Arc"));
            }
            let span = args[0].to_radians().abs().max(1e-6);
            let rot = args.get(1).copied().unwrap_or(0.0).to_radians();
            let r_top = args
                .get(2)
                .copied()
                .unwrap_or(w / span * 0.5 + h / 2.0)
                .max(1.0);
            let r_bottom = args.get(3).copied().unwrap_or(r_top - h).max(0.0);
            let start = -std::f64::consts::FRAC_PI_2 - span / 2.0 + rot;
            let size = (2.0 * r_top).ceil() as usize + 2;
            out_rect = Rect::new(size, size, -(size as i64) / 2, -(size as i64) / 2);
            let c = size as f64 / 2.0;
            let m = Map::Arc {
                cx: c,
                cy: c,
                start,
                span,
                r_top,
                r_bottom,
                w,
                h,
            };
            return render(img, &m, out_rect, true);
        }
        POLAR | DEPOLAR => {
            let rmax = args
                .first()
                .copied()
                .filter(|v| *v > 0.0)
                .unwrap_or((w.min(h)) / 2.0);
            let rmin = args.get(1).copied().unwrap_or(0.0);
            let cx = args.get(2).copied().unwrap_or(w / 2.0);
            let cy = args.get(3).copied().unwrap_or(h / 2.0);
            let a0 = args.get(4).copied().unwrap_or(-180.0).to_radians();
            let a1 = args.get(5).copied().unwrap_or(180.0).to_radians();
            if method == POLAR {
                let size = (2.0 * rmax).ceil().max(1.0) as usize;
                out_rect = Rect::new(size, size, 0, 0);
                let m = Map::Polar {
                    cx,
                    cy,
                    rmin,
                    rmax,
                    a0,
                    a1,
                    w,
                    h,
                    to_polar: true,
                    ox: size as f64 / 2.0,
                    oy: size as f64 / 2.0,
                };
                return render(img, &m, out_rect, true);
            }
            let ow = ((a1 - a0) * rmax).abs().ceil().max(1.0);
            let oh = (rmax - rmin).abs().ceil().max(1.0);
            out_rect = Rect::new(ow as usize, oh as usize, 0, 0);
            let m = Map::Polar {
                cx,
                cy,
                rmin,
                rmax,
                a0,
                a1,
                w: ow,
                h: oh,
                to_polar: false,
                ox: 0.0,
                oy: 0.0,
            };
            return render(img, &m, out_rect, true);
        }
        BARREL | BARREL_INVERSE => {
            if args.len() < 3 {
                return Err(bad_args("Barrel"));
            }
            let (a, b, cx, cy) = match args.len() {
                3 => (
                    [args[0], args[1], args[2], 1.0 - args[0] - args[1] - args[2]],
                    [args[0], args[1], args[2], 1.0 - args[0] - args[1] - args[2]],
                    w / 2.0,
                    h / 2.0,
                ),
                4 | 5 => (
                    [args[0], args[1], args[2], args[3]],
                    [args[0], args[1], args[2], args[3]],
                    w / 2.0,
                    h / 2.0,
                ),
                6 => (
                    [args[0], args[1], args[2], args[3]],
                    [args[0], args[1], args[2], args[3]],
                    args[4],
                    args[5],
                ),
                _ => (
                    [args[0], args[1], args[2], args[3]],
                    [args[4], args[5], args[6], args[7]],
                    args.get(8).copied().unwrap_or(w / 2.0),
                    args.get(9).copied().unwrap_or(h / 2.0),
                ),
            };
            Map::Barrel {
                a,
                b,
                cx,
                cy,
                rscale: w.min(h) / 2.0,
                inverse: method == BARREL_INVERSE,
            }
        }
        SHEPARDS => {
            let p = pairs(args);
            if p.is_empty() {
                return Err(bad_args("Shepards"));
            }
            Map::Shepards(p)
        }
        RESIZE => {
            if args.len() < 2 {
                return Err(bad_args("Resize"));
            }
            let (nw, nh) = (args[0].max(1.0), args[1].max(1.0));
            out_rect = Rect::new(nw as usize, nh as usize, 0, 0);
            let m = Map::Affine(Aff([w / nw, 0.0, 0.0, 0.0, h / nh, 0.0]));
            return render(img, &m, out_rect, false);
        }
        CYLINDER2PLANE | PLANE2CYLINDER => {
            // a horizontal stretch around the centre
            let fov = args.first().copied().unwrap_or(90.0).to_radians();
            let r = w / fov;
            let cx = w / 2.0;
            let inverse = method == CYLINDER2PLANE;
            let m = Map::Barrel {
                a: [0.0, 0.0, 0.0, 1.0],
                b: [0.0, 0.0, 0.0, 1.0],
                cx,
                cy: h / 2.0,
                rscale: r.max(1.0),
                inverse,
            };
            m.apply(0.0, 0.0);
            return render(img, &m, out_rect, false);
        }
        _ => {
            return Err(MagickError::with_desc(
                severity::OPTION_ERROR,
                "UnrecognizedDistortMethod",
                &method.to_string(),
                "error/distort.c/DistortImage",
            ));
        }
    };
    if bestfit {
        if let Some(f) = &forward {
            let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for &(x, y) in &corners {
                let (fx, fy) = f(x, y);
                if fx.is_finite() && fy.is_finite() {
                    x0 = x0.min(fx);
                    y0 = y0.min(fy);
                    x1 = x1.max(fx);
                    y1 = y1.max(fy);
                }
            }
            if x0 < x1 && y0 < y1 && (x1 - x0) < 100_000.0 && (y1 - y0) < 100_000.0 {
                // the bounds grow by half a pixel each way
                let (xa, ya) = ((x0 - 0.5).floor(), (y0 - 0.5).floor());
                out_rect = Rect::new(
                    ((x1 + 0.5).ceil() - xa).max(1.0) as usize,
                    ((y1 + 0.5).ceil() - ya).max(1.0) as usize,
                    xa as i64,
                    ya as i64,
                );
            }
        }
    }
    render(img, &map, out_rect, false)
}

fn render(img: &Image, map: &Map, r: Rect, own_coords: bool) -> MResult<Image> {
    let mut src = img.clone();
    src.cmyk_to_rgb();
    let mut out = img.clone_attributes(r.width, r.height);
    out.colorspace = src.colorspace;
    out.black = None;
    let transparent_vp = matches!(
        src.virtual_pixel,
        crate::types::VirtualPixelMethod::Transparent
            | crate::types::VirtualPixelMethod::Background
    );
    if transparent_vp || src.alpha {
        out.alpha = true;
    }
    let ewa = crate::ewa::Ewa::new();
    let vp = src.virtual_pixel;
    for y in 0..r.height {
        for x in 0..r.width {
            let (dx, dy) = if own_coords {
                (x as f64 + 0.5, y as f64 + 0.5)
            } else {
                (x as f64 + r.x as f64 + 0.5, y as f64 + r.y as f64 + 0.5)
            };
            let (u, v) = map.apply(dx, dy);
            let p = if u.is_finite() && v.is_finite() {
                let j = map.jacobian(dx, dy);
                let s = ewa.sample(&src, u, v, j, vp);
                [
                    crate::color::to_q16(s[0]),
                    crate::color::to_q16(s[1]),
                    crate::color::to_q16(s[2]),
                    crate::color::to_q16(s[3]),
                ]
            } else {
                src.background_color.to_rgba16()
            };
            out.set_px(x, y, p);
        }
    }
    out.page = r;
    out.colors = None;
    Ok(out)
}
