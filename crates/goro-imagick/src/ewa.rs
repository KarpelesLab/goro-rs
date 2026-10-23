//! Elliptical weighted average resampling, as DistortImage() does it.
//!
//! Black-box facts this follows (ImageMagick 7.1.2 Q16):
//! - the filter is the cylindrical Robidoux cubic (B = 0.37821575509399867,
//!   C = 0.31089212245300067), support 2;
//! - the reverse-map Jacobian J is clamped so neither singular value is
//!   below 1 (magnification never shrinks the ellipse under a pixel);
//! - a source pixel centre at offset d contributes when
//!   Q = |Jc^-1 d|² < support², weighted by a 1024-entry table over Q
//!   (entry i = filter(sqrt(i·support²/1024)), indexed by
//!   floor(Q·1024/support²));
//! - the weighted sum is normalised by the sum of weights, colour
//!   alpha-weighted.

use crate::QUANTUM_SCALE;
use crate::image::Image;
use crate::types::VirtualPixelMethod;

const SUPPORT: f64 = 2.0;
const LUT_SIZE: usize = 1024;

fn robidoux(x: f64) -> f64 {
    const B: f64 = 0.378_215_755_093_998_7;
    const C: f64 = 0.310_892_122_453_000_7;
    let x = x.abs();
    if x < 1.0 {
        ((12.0 - 9.0 * B - 6.0 * C) * x * x * x
            + (-18.0 + 12.0 * B + 6.0 * C) * x * x
            + (6.0 - 2.0 * B))
            / 6.0
    } else if x < 2.0 {
        ((-B - 6.0 * C) * x * x * x
            + (6.0 * B + 30.0 * C) * x * x
            + (-12.0 * B - 48.0 * C) * x
            + (8.0 * B + 24.0 * C))
            / 6.0
    } else {
        0.0
    }
}

pub struct Ewa {
    lut: Vec<f64>,
}

impl Default for Ewa {
    fn default() -> Self {
        Self::new()
    }
}

impl Ewa {
    pub fn new() -> Ewa {
        let lut = (0..=LUT_SIZE)
            .map(|i| robidoux((i as f64 * SUPPORT * SUPPORT / LUT_SIZE as f64).sqrt()))
            .collect();
        Ewa { lut }
    }

    /// The resampled pixel at continuous source position (u, v) (pixel
    /// centres at +0.5), for the reverse-map Jacobian
    /// [[du/dx, du/dy], [dv/dx, dv/dy]].
    pub fn sample(
        &self,
        img: &Image,
        u: f64,
        v: f64,
        j: [[f64; 2]; 2],
        vp: VirtualPixelMethod,
    ) -> [f64; 4] {
        // A = J Jᵀ = U diag(s²) Uᵀ; clamp s ≥ 1
        let a = j[0][0] * j[0][0] + j[0][1] * j[0][1];
        let b = j[0][0] * j[1][0] + j[0][1] * j[1][1];
        let c = j[1][0] * j[1][0] + j[1][1] * j[1][1];
        let tr = a + c;
        let disc = (((a - c) * 0.5).powi(2) + b * b).sqrt();
        let l1 = (tr * 0.5 + disc).max(1.0);
        let l2 = (tr * 0.5 - disc).max(1.0);
        // eigenvector of l1
        // (the better-conditioned of the two eigenvector forms)
        let lam = tr * 0.5 + disc;
        let (x, y) = if a >= c { (lam - c, b) } else { (b, lam - a) };
        let n = (x * x + y * y).sqrt();
        let (e1x, e1y) = if n > 1e-300 {
            (x / n, y / n)
        } else {
            (1.0, 0.0)
        };
        let (e2x, e2y) = (-e1y, e1x);
        // quadratic form Q(d) = (e1·d)²/l1 + (e2·d)²/l2
        let qa = e1x * e1x / l1 + e2x * e2x / l2;
        let qb = 2.0 * (e1x * e1y / l1 + e2x * e2y / l2);
        let qc = e1y * e1y / l1 + e2y * e2y / l2;
        // bounding box of the ellipse Q = support²: Jc Jcᵀ diagonal
        let m00 = e1x * e1x * l1 + e2x * e2x * l2;
        let m11 = e1y * e1y * l1 + e2y * e2y * l2;
        let hx = m00.sqrt() * SUPPORT;
        let hy = m11.sqrt() * SUPPORT;
        let (cu, cv) = (u - 0.5, v - 0.5);
        let x0 = (cu - hx).ceil() as i64;
        let x1 = (cu + hx).floor() as i64;
        let y0 = (cv - hy).ceil() as i64;
        let y1 = (cv + hy).floor() as i64;
        let s2 = SUPPORT * SUPPORT;
        let scale = LUT_SIZE as f64 / s2;
        let (mut cr, mut cg, mut cb, mut ca, mut wa, mut wt) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for y in y0..=y1 {
            let dv = y as f64 - cv;
            for x in x0..=x1 {
                let du = x as f64 - cu;
                let q = qa * du * du + qb * du * dv + qc * dv * dv;
                if q >= s2 {
                    continue;
                }
                let w = self.lut[(q * scale) as usize];
                let p = img.virtual_px_with(x, y, vp);
                let al = p[3] as f64 * QUANTUM_SCALE;
                cr += w * al * p[0] as f64;
                cg += w * al * p[1] as f64;
                cb += w * al * p[2] as f64;
                ca += w * p[3] as f64;
                wa += w * al;
                wt += w;
            }
        }
        if wt.abs() < 1e-300 {
            let p = img.virtual_px_with(cu.round() as i64, cv.round() as i64, vp);
            return [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64];
        }
        let g = if wa.abs() > 1e-300 { 1.0 / wa } else { 0.0 };
        [cr * g, cg * g, cb * g, ca / wt]
    }
}
