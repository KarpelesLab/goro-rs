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

const MAGICK_EPSILON: f64 = 1.0e-12;

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
    /// Source -> destination bilinear, inverted per pixel.
    BilinearForward([f64; 8]),
    Poly(usize, Vec<f64>, Vec<f64>),
    /// ImageMagick's coefficients: c0 the centre angle, c1 columns per
    /// turn, c2 the top radius, c3 rows per radius unit, c4 the centre
    /// column.
    Arc([f64; 5]),
    /// rmax, rmin, centre x/y, start/end angle, then the column and row
    /// scales (ImageMagick's c0..c7).
    Polar([f64; 8]),
    DePolar([f64; 8]),
    /// Plane2Cylinder (p2c) or Cylinder2Plane: radius, destination and
    /// source centres.
    Cylinder {
        p2c: bool,
        r: f64,
        dc: (f64, f64),
        sc: (f64, f64),
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
        if let Map::Polar(c) = self {
            // ImageMagick scales the angular axis by c6/(2πr), a 2π
            // short of the true derivative
            let (ii, jj) = (x - c[2], y - c[3]);
            let r2 = (ii * ii + jj * jj).max(1e-12);
            let r = r2.sqrt();
            let k = c[6] / (2.0 * std::f64::consts::PI);
            return [[k * jj / r2, -k * ii / r2], [c[7] * ii / r, c[7] * jj / r]];
        }
        if let Map::Poly(order, cu, cv) = self {
            let mut j = [[0.0; 2]; 2];
            let mut k = 0;
            for total in 0..=*order {
                for py in 0..=total {
                    let px = total - py;
                    let fx = (px as f64).min(1.0);
                    let fy = (py as f64).min(1.0);
                    let dx = if px > 0 {
                        fx * x.powi(px as i32 - 1) * y.powi(py as i32)
                    } else {
                        0.0
                    };
                    let dy = if py > 0 {
                        fy * x.powi(px as i32) * y.powi(py as i32 - 1)
                    } else {
                        0.0
                    };
                    j[0][0] += cu[k] * dx;
                    j[0][1] += cu[k] * dy;
                    j[1][0] += cv[k] * dx;
                    j[1][1] += cv[k] * dy;
                    k += 1;
                }
            }
            return j;
        }
        if let Map::BilinearForward(_) | Map::Shepards(_) = self {
            return [[1.0, 0.0], [0.0, 1.0]];
        }
        if let Map::DePolar(_) = self {
            // no scaling: a unit-circle filter everywhere
            return [[1.0, 0.0], [0.0, 1.0]];
        }
        if let Map::Arc(c) = self {
            let (ii, jj) = (x, y);
            let r2 = (ii * ii + jj * jj).max(1e-12);
            let r = r2.sqrt();
            let k = c[1] / (2.0 * std::f64::consts::PI);
            return [
                [-k * jj / r2, k * ii / r2],
                [-c[3] * ii / r, -c[3] * jj / r],
            ];
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
            Map::Arc(c) => {
                // (c4 is the centre column as a pixel index; +0.5 makes it a
                // coordinate)
                let (ii, jj) = (x, y);
                let mut xx = (jj.atan2(ii) - c[0]) / (2.0 * std::f64::consts::PI);
                xx -= xx.round();
                (xx * c[1] + c[4] + 0.5, (c[2] - ii.hypot(jj)) * c[3])
            }
            Map::Polar(c) => {
                let (ii, jj) = (x - c[2], y - c[3]);
                let tau = 2.0 * std::f64::consts::PI;
                let mut xx = (ii.atan2(jj) - (c[4] + c[5]) / 2.0) / tau;
                xx -= xx.round();
                (
                    xx * tau * c[6] + c[6] * (c[5] - c[4]) / 2.0,
                    (ii.hypot(jj) - c[1]) * c[7],
                )
            }
            Map::BilinearForward(c) => {
                // x = a0 + a1·u + a2·v + a3·u·v (and b for y), solved for v
                let (ii, jj) = (x - c[0], y - c[4]);
                let (a1, a2, a3) = (c[1], c[2], c[3]);
                let (b1, b2, b3) = (c[5], c[6], c[7]);
                let qa = b2 * a3 - b3 * a2;
                let qb = b3 * ii - a3 * jj + (b2 * a1 - b1 * a2);
                let qc = b1 * ii - a1 * jj;
                let v = if qa.abs() < 1e-12 {
                    if qb.abs() < 1e-300 {
                        return (f64::NAN, f64::NAN);
                    }
                    -qc / qb
                } else {
                    let rt = qb * qb - 4.0 * qa * qc;
                    if rt < 0.0 {
                        return (f64::NAN, f64::NAN);
                    }
                    (-qb + rt.sqrt()) / (2.0 * qa)
                };
                ((ii - a2 * v) / (a1 + a3 * v), v)
            }
            Map::Cylinder { p2c, r, dc, sc } => {
                let (ii, jj) = (x - dc.0, y - dc.1);
                if *p2c {
                    let a = ii / r;
                    (r * a.tan() + sc.0, jj / a.cos() + sc.1)
                } else {
                    let a = (ii / r).atan();
                    (r * a + sc.0, jj * a.cos() + sc.1)
                }
            }
            Map::DePolar(c) => {
                let aa = x * c[6] + c[4];
                let rr = y * c[7] + c[1];
                (rr * aa.sin() + c[2], rr * aa.cos() + c[3])
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
            let p = pairs(args);
            let fwd = if method == RIGID_AFFINE && p.len() >= 2 {
                // least-squares similarity: x = a·u − b·v + c, y = b·u + a·v + d
                let mut rows = Vec::new();
                let mut vals = Vec::new();
                for &(u, v, x, y) in &p {
                    rows.push(vec![u, -v, 1.0, 0.0]);
                    vals.push(x);
                    rows.push(vec![v, u, 0.0, 1.0]);
                    vals.push(y);
                }
                let c = least_squares(&rows, &vals).ok_or_else(|| bad_args("RigidAffine"))?;
                Aff([c[0], -c[1], c[2], c[1], c[0], c[3]])
            } else {
                affine_from_points(&p).ok_or_else(|| bad_args("Affine"))?
            };
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
            let fit = |pts: &[(f64, f64, f64, f64)]| -> Option<[f64; 8]> {
                // (x, y) -> (u, v): c0 + c1·x + c2·y + c3·x·y
                let rows: Vec<Vec<f64>> = pts
                    .iter()
                    .map(|&(_, _, x, y)| vec![1.0, x, y, x * y])
                    .collect();
                let us: Vec<f64> = pts.iter().map(|q| q.0).collect();
                let vs: Vec<f64> = pts.iter().map(|q| q.1).collect();
                let cu = least_squares(&rows, &us)?;
                let cv = least_squares(&rows, &vs)?;
                Some([cu[0], cu[1], cu[2], cu[3], cv[0], cv[1], cv[2], cv[3]])
            };
            if method == BILINEAR_REVERSE {
                // (no best fit: ImageMagick keeps the image's size)
                Map::Bilinear(fit(&p).ok_or_else(|| bad_args("Bilinear"))?)
            } else {
                // the bilinear fits source -> destination; each destination
                // pixel solves its quadratic back
                let swapped: Vec<_> = p.iter().map(|&(u, v, x, y)| (x, y, u, v)).collect();
                Map::BilinearForward(fit(&swapped).ok_or_else(|| bad_args("Bilinear"))?)
            }
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
            // (no best fit, as for Bilinear, Shepards and Barrel)
            Map::Poly(order, cu, cv)
        }
        ARC => {
            if args.is_empty() {
                return Err(bad_args("Arc"));
            }
            if args[0] < MAGICK_EPSILON {
                return Err(MagickError::new(
                    severity::OPTION_ERROR,
                    "invalid argument for option Arc : 'Arc Angle Too Small' @ error/distort.c/GenerateCoefficients/1215",
                ));
            }
            let span = args[0].to_radians();
            let c0 =
                -std::f64::consts::FRAC_PI_2 + args.get(1).copied().unwrap_or(0.0).to_radians();
            // the default radii put the image's width on the arc through
            // its middle row
            let rt_def = w / span + h / 2.0 - 0.5;
            let rb_def = w / span - h / 2.0 + 0.5;
            let (r_top, r_bottom) = match (args.get(2), args.get(3)) {
                (Some(&t), Some(&b)) => (t, b),
                (Some(&t), None) => (t, t * rb_def / rt_def),
                _ => (rt_def, rb_def),
            };
            let c = [
                c0,
                w * 2.0 * std::f64::consts::PI / span,
                r_top,
                h / (r_top - r_bottom),
                w / 2.0 - 0.5,
            ];
            // bounds: both radii at both ends, the top radius where the
            // arc crosses an axis
            let (a0, a1) = (c0 - span / 2.0, c0 + span / 2.0);
            let mut pts = vec![];
            for a in [a0, a1] {
                for r in [r_top, r_bottom] {
                    pts.push((r * a.cos(), r * a.sin()));
                }
            }
            let q = std::f64::consts::FRAC_PI_2;
            let mut k = (a0 / q).ceil();
            while k * q <= a1 {
                pts.push((r_top * (k * q).cos(), r_top * (k * q).sin()));
                k += 1.0;
            }
            let x0 = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
            let x1 = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
            let y0 = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
            let y1 = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
            let (xa, ya) = ((x0 - 0.5).floor(), (y0 - 0.5).floor());
            out_rect = Rect::new(
                ((x1 + 0.5).ceil() - xa).max(1.0) as usize,
                ((y1 + 0.5).ceil() - ya).max(1.0) as usize,
                xa as i64,
                ya as i64,
            );
            return render(img, &Map::Arc(c), out_rect, false);
        }
        POLAR | DEPOLAR => {
            let mut cx = args.get(2).copied().unwrap_or(w / 2.0);
            let mut cy = args.get(3).copied().unwrap_or(h / 2.0);
            // rmax 0: to the nearest edge; negative: to the farthest corner
            let rmax = match args.first().copied().unwrap_or(0.0) {
                r if r > 0.0 => r,
                r if r < 0.0 => (cx.max(w - cx)).hypot(cy.max(h - cy)),
                _ => cx.min(cy).min(w - cx).min(h - cy),
            };
            let rmin = args.get(1).copied().unwrap_or(0.0);
            let a0 = args.get(4).copied().unwrap_or(-180.0).to_radians();
            let a1 = match args.get(5) {
                Some(v) => v.to_radians(),
                None => a0 + 2.0 * std::f64::consts::PI,
            };
            if rmax <= rmin || (a1 - a0).abs() < MAGICK_EPSILON {
                return Err(bad_args(if method == POLAR { "Polar" } else { "DePolar" }));
            }
            if method == POLAR {
                if bestfit {
                    // the circle centres on the page origin unless placed
                    if args.len() < 3 {
                        cx = 0.0;
                        cy = 0.0;
                    }
                    let (xa, ya) = ((cx - rmax - 0.5).floor(), (cy - rmax - 0.5).floor());
                    out_rect = Rect::new(
                        ((cx + rmax + 0.5).ceil() - xa) as usize,
                        ((cy + rmax + 0.5).ceil() - ya) as usize,
                        xa as i64,
                        ya as i64,
                    );
                }
                let c = [rmax, rmin, cx, cy, a0, a1, w / (a1 - a0), h / (rmax - rmin)];
                return render(img, &Map::Polar(c), out_rect, false);
            }
            let (ow, oh) = if bestfit {
                (
                    ((a1 - a0) * (rmax - rmin) / 2.0).abs().ceil().max(1.0),
                    (rmax - rmin).ceil().max(1.0),
                )
            } else {
                (w, h)
            };
            out_rect = Rect::new(ow as usize, oh as usize, 0, 0);
            let c = [
                rmax,
                rmin,
                cx,
                cy,
                a0,
                a1,
                (a1 - a0) / ow,
                (rmax - rmin) / oh,
            ];
            return render(img, &Map::DePolar(c), out_rect, false);
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
            let fov = args.first().copied().unwrap_or(90.0).to_radians();
            let p2c = method == PLANE2CYLINDER;
            let half = fov / 2.0;
            if half.abs() < MAGICK_EPSILON {
                return Err(bad_args(if p2c {
                    "Plane2Cylinder"
                } else {
                    "Cylinder2Plane"
                }));
            }
            // the radius puts the image's half width at half the field of view
            let r = if p2c {
                w / 2.0 / half.tan()
            } else {
                w / 2.0 / half
            };
            let scx = args.get(1).copied().unwrap_or(w / 2.0);
            let scy = args.get(2).copied().unwrap_or(h / 2.0);
            let (mut dcx, mut dcy) = (scx, scy);
            if bestfit {
                // corners and edge midpoints, forward; the result keeps a
                // zero page with the centre in its middle
                let fwd = |u: f64, v: f64| -> (f64, f64) {
                    let (du, dv) = (u - scx, v - scy);
                    if p2c {
                        let a = (du / r).atan();
                        (r * a, dv * a.cos())
                    } else {
                        let a = du / r;
                        (r * a.tan(), dv / a.cos())
                    }
                };
                let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for (u, v) in [
                    (0.0, 0.0),
                    (w, 0.0),
                    (0.0, h),
                    (w, h),
                    (scx, 0.0),
                    (scx, h),
                    (0.0, scy),
                    (w, scy),
                ] {
                    let (x, y) = fwd(u, v);
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
                let nw = ((x1 + scx).ceil() - (x0 + scx).floor()).max(1.0);
                let nh = ((y1 + scy).ceil() - (y0 + scy).floor()).max(1.0);
                out_rect = Rect::new(nw as usize, nh as usize, 0, 0);
                dcx = nw / 2.0;
                dcy = nh / 2.0;
            }
            let m = Map::Cylinder {
                p2c,
                r,
                dc: (dcx, dcy),
                sc: (scx, scy),
            };
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
