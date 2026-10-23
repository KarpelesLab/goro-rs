//! InterpolatePixelChannels(): sampling an image between pixels with one
//! of the -interpolate methods.
//!
//! Positions are in pixel-index coordinates (pixel (i, j) is at exactly
//! (i, j)), as `-fx 'p{x,y}'` and the warping operations use them. Colour
//! is alpha-weighted where the method blends pixels.

use crate::QUANTUM_SCALE;
use crate::image::Image;
use crate::types::{PixelInterpolateMethod, VirtualPixelMethod};

/// The cubic B,C kernel (Mitchell-Netravali family).
fn bc_cubic(b: f64, c: f64, x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        ((12.0 - 9.0 * b - 6.0 * c) * x * x * x
            + (-18.0 + 12.0 * b + 6.0 * c) * x * x
            + (6.0 - 2.0 * b))
            / 6.0
    } else if x < 2.0 {
        ((-b - 6.0 * c) * x * x * x
            + (6.0 * b + 30.0 * c) * x * x
            + (-12.0 * b - 48.0 * c) * x
            + (8.0 * b + 24.0 * c))
            / 6.0
    } else {
        0.0
    }
}

fn cubic_weights(t: f64, b: f64, c: f64) -> [f64; 4] {
    [
        bc_cubic(b, c, t + 1.0),
        bc_cubic(b, c, t),
        bc_cubic(b, c, 1.0 - t),
        bc_cubic(b, c, 2.0 - t),
    ]
}

/// Sums weighted pixels, alpha-weighting the colour.
struct Acc {
    c: [f64; 3],
    a: f64,
    wa: f64,
    w: f64,
}

impl Acc {
    fn new() -> Acc {
        Acc {
            c: [0.0; 3],
            a: 0.0,
            wa: 0.0,
            w: 0.0,
        }
    }
    fn add(&mut self, p: [u16; 4], w: f64) {
        let a = p[3] as f64 * QUANTUM_SCALE;
        for i in 0..3 {
            self.c[i] += w * a * p[i] as f64;
        }
        self.wa += w * a;
        self.a += w * p[3] as f64;
        self.w += w;
    }
    fn get(&self) -> [f64; 4] {
        let g = if self.wa.abs() > 1e-12 {
            1.0 / self.wa
        } else {
            0.0
        };
        [self.c[0] * g, self.c[1] * g, self.c[2] * g, self.a]
    }
}

/// The interpolated pixel at (x, y), virtual pixels by `vp` outside.
pub fn interpolate(
    img: &Image,
    method: PixelInterpolateMethod,
    x: f64,
    y: f64,
    vp: VirtualPixelMethod,
) -> [f64; 4] {
    let at = |i: i64, j: i64| img.virtual_px_with(i, j, vp);
    let conv = |p: [u16; 4]| [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64];
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ix, iy) = (x0 as i64, y0 as i64);
    match method {
        PixelInterpolateMethod::Integer => conv(at(ix, iy)),
        PixelInterpolateMethod::Nearest => {
            conv(at((x + 0.5).floor() as i64, (y + 0.5).floor() as i64))
        }
        PixelInterpolateMethod::Background => conv(img.background_color.to_rgba16()),
        PixelInterpolateMethod::Average
        | PixelInterpolateMethod::Average9
        | PixelInterpolateMethod::Average16 => {
            let (n, sx, sy) = match method {
                PixelInterpolateMethod::Average => (2, ix, iy),
                PixelInterpolateMethod::Average9 => (
                    3,
                    (x + 0.5).floor() as i64 - 1,
                    (y + 0.5).floor() as i64 - 1,
                ),
                _ => (4, ix - 1, iy - 1),
            };
            let mut acc = Acc::new();
            for j in 0..n {
                for i in 0..n {
                    acc.add(at(sx + i, sy + j), 1.0);
                }
            }
            let mut r = acc.get();
            r[3] /= (n * n) as f64;
            r
        }
        PixelInterpolateMethod::Catrom | PixelInterpolateMethod::Spline => {
            let (b, c) = if method == PixelInterpolateMethod::Catrom {
                (0.0, 0.5)
            } else {
                (1.0, 0.0)
            };
            let wx = cubic_weights(fx, b, c);
            let wy = cubic_weights(fy, b, c);
            let mut acc = Acc::new();
            for j in 0..4 {
                for i in 0..4 {
                    acc.add(at(ix - 1 + i, iy - 1 + j), wx[i as usize] * wy[j as usize]);
                }
            }
            acc.get()
        }
        PixelInterpolateMethod::Mesh => {
            let p = [
                at(ix, iy),
                at(ix + 1, iy),
                at(ix, iy + 1),
                at(ix + 1, iy + 1),
            ];
            let lum = |q: [u16; 4]| Image::intensity(&q);
            // split the cell along the diagonal of least change
            let w = if (lum(p[0]) - lum(p[3])).abs() < (lum(p[1]) - lum(p[2])).abs() {
                if fx >= fy {
                    [1.0 - fx, fx - fy, 0.0, fy]
                } else {
                    [1.0 - fy, 0.0, fy - fx, fx]
                }
            } else if fx + fy <= 1.0 {
                [1.0 - fx - fy, fx, fy, 0.0]
            } else {
                [0.0, 1.0 - fy, 1.0 - fx, fx + fy - 1.0]
            };
            let mut acc = Acc::new();
            for k in 0..4 {
                acc.add(p[k], w[k]);
            }
            acc.get()
        }
        PixelInterpolateMethod::Blend => {
            // the nearest 1, 2 or 4 pixels, averaged
            let mut acc = Acc::new();
            let near = |f: f64| -> (i64, i64) {
                if f < 0.25 {
                    (0, 0)
                } else if f > 0.75 {
                    (1, 1)
                } else {
                    (0, 1)
                }
            };
            let (xa, xb) = near(fx);
            let (ya, yb) = near(fy);
            let mut n = 0.0;
            for j in [ya, yb]
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
            {
                for i in [xa, xb]
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                {
                    acc.add(at(ix + i, iy + j), 1.0);
                    n += 1.0;
                }
            }
            let mut r = acc.get();
            r[3] /= n;
            r
        }
        _ => {
            // Bilinear (and Undefined)
            let w = [
                (1.0 - fx) * (1.0 - fy),
                fx * (1.0 - fy),
                (1.0 - fx) * fy,
                fx * fy,
            ];
            let p = [
                at(ix, iy),
                at(ix + 1, iy),
                at(ix, iy + 1),
                at(ix + 1, iy + 1),
            ];
            let mut acc = Acc::new();
            for k in 0..4 {
                acc.add(p[k], w[k]);
            }
            acc.get()
        }
    }
}

/// Rounds an interpolated pixel to Q16.
pub fn to_q16(p: [f64; 4]) -> [u16; 4] {
    [
        crate::color::to_q16(p[0]),
        crate::color::to_q16(p[1]),
        crate::color::to_q16(p[2]),
        crate::color::to_q16(p[3]),
    ]
}
