//! ResizeImage(): ImageMagick's separable two-pass resampling with its
//! filter catalogue (the kernels, windows, supports and blurs the
//! `filter:verbose` define of `magick` reports).
//!
//! Each output sample centred at `(x + ½) / factor` gathers the source
//! samples within `support · max(1, 1/factor)`, weighted by the filter
//! evaluated at the distance scaled by `min(1, factor)`; the weights are
//! normalised, colour channels are weighted by alpha, and each pass
//! stores Q16 samples.

use crate::image::Image;
use crate::types::FilterType;
use std::f64::consts::PI;

/// A resize filter: a kernel times a window over `support`.
#[derive(Clone, Copy, Debug)]
pub struct Filter {
    kernel: Kernel,
    window: Window,
    /// The kernel's support (before blur).
    pub support: f64,
    /// The window's support (the window is evaluated at x / win_support).
    win_support: f64,
    pub blur: f64,
}

#[derive(Clone, Copy, Debug)]
enum Kernel {
    Box,
    Triangle,
    Cubic(f64, f64),
    Sinc,
    Gaussian(f64),
    Quadratic,
    Jinc,
    Lagrange,
    CubicSpline,
}

#[derive(Clone, Copy, Debug)]
enum Window {
    Box,
    Hann,
    Hamming,
    Blackman,
    Welch,
    Bohman,
    Cosine,
    Triangle,
    Sinc,
    Kaiser(f64),
    /// Parzen: the cubic B-spline over twice the window domain.
    Spline,
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

fn cubic_bc(b: f64, c: f64, x: f64) -> f64 {
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

/// The modified Bessel function of the first kind, order 0.
fn bessel_i0(x: f64) -> f64 {
    let (mut s, mut t, mut k) = (1.0, 1.0, 1.0);
    let y = x * x / 4.0;
    while t > 1e-16 * s {
        t *= y / (k * k);
        s += t;
        k += 1.0;
    }
    s
}

/// The Bessel function of the first kind, order 1.
fn bessel_j1(x: f64) -> f64 {
    if x.abs() < 12.0 {
        let (mut s, mut t, mut k) = (0.0, x / 2.0, 0.0);
        let y = x * x / 4.0;
        while t.abs() > 1e-18 {
            s += t;
            k += 1.0;
            t *= -y / (k * (k + 1.0));
        }
        s
    } else {
        // Hankel asymptotic expansion
        let ax = x.abs();
        let mu = 4.0;
        let z = 8.0 * ax;
        let p = 1.0 - (mu - 1.0) * (mu - 9.0) / (2.0 * z * z)
            + (mu - 1.0) * (mu - 9.0) * (mu - 25.0) * (mu - 49.0) / (24.0 * z.powi(4));
        let q = (mu - 1.0) / z - (mu - 1.0) * (mu - 9.0) * (mu - 25.0) / (6.0 * z.powi(3));
        let chi = ax - 0.75 * PI;
        let v = (2.0 / (PI * ax)).sqrt() * (p * chi.cos() - q * chi.sin());
        if x < 0.0 { -v } else { v }
    }
}

impl Filter {
    /// The filter ImageMagick uses for a FilterType (Undefined: Lanczos
    /// to shrink an opaque image, Mitchell otherwise).
    pub fn new(ft: FilterType, shrink_opaque: bool) -> Filter {
        let f = |kernel, window, support: f64, win_support: f64, blur: f64| Filter {
            kernel,
            window,
            support,
            win_support,
            blur,
        };
        let sw = |w: Window, s: f64| f(Kernel::Sinc, w, s, s, 1.0);
        let cubic = |b: f64, c: f64| f(Kernel::Cubic(b, c), Window::Box, 2.0, 2.0, 1.0);
        match ft {
            FilterType::Point => f(Kernel::Box, Window::Box, 0.0, 0.0, 1.0),
            FilterType::Box => f(Kernel::Box, Window::Box, 0.5, 0.5, 1.0),
            FilterType::Triangle => f(Kernel::Triangle, Window::Box, 1.0, 1.0, 1.0),
            FilterType::Hermite => f(Kernel::Cubic(0.0, 0.0), Window::Box, 1.0, 1.0, 1.0),
            FilterType::Hann => sw(Window::Hann, 4.0),
            FilterType::Hamming => sw(Window::Hamming, 4.0),
            FilterType::Blackman => sw(Window::Blackman, 4.0),
            FilterType::Gaussian => f(Kernel::Gaussian(0.5), Window::Box, 2.0, 2.0, 1.0),
            FilterType::Quadratic => f(Kernel::Quadratic, Window::Box, 1.5, 1.5, 1.0),
            FilterType::Cubic | FilterType::Spline => cubic(1.0, 0.0),
            FilterType::Catrom => cubic(0.0, 0.5),
            FilterType::Mitchell => cubic(1.0 / 3.0, 1.0 / 3.0),
            FilterType::Jinc => f(
                Kernel::Jinc,
                Window::Box,
                3.2383154841662362,
                3.2383154841662362,
                1.0,
            ),
            FilterType::Sinc | FilterType::SincFast => sw(Window::Box, 4.0),
            FilterType::Kaiser => sw(Window::Kaiser(6.5), 4.0),
            FilterType::Welch => sw(Window::Welch, 3.0),
            FilterType::Parzen => sw(Window::Spline, 4.0),
            FilterType::Bohman => sw(Window::Bohman, 4.0),
            FilterType::Bartlett => sw(Window::Triangle, 4.0),
            FilterType::Lagrange => f(Kernel::Lagrange, Window::Box, 2.0, 2.0, 1.0),
            FilterType::LanczosSharp => f(Kernel::Sinc, Window::Sinc, 3.0, 3.0, 0.9812505644269356),
            FilterType::Lanczos2 => f(Kernel::Sinc, Window::Sinc, 2.0, 2.0, 1.0),
            FilterType::Lanczos2Sharp => {
                f(Kernel::Sinc, Window::Sinc, 2.0, 2.0, 0.9549963639785485)
            }
            FilterType::Robidoux => cubic(0.37821575509399867, 0.31089212245300067),
            FilterType::RobidouxSharp => cubic(0.2620145123990142, 0.3689927438004929),
            FilterType::Cosine => sw(Window::Cosine, 3.0),
            FilterType::CubicSpline => f(Kernel::CubicSpline, Window::Box, 2.0, 2.0, 1.0),
            FilterType::Undefined if !shrink_opaque => cubic(1.0 / 3.0, 1.0 / 3.0),
            // Lanczos, LanczosRadius, and the rest
            _ => f(Kernel::Sinc, Window::Sinc, 3.0, 3.0, 1.0),
        }
    }

    /// The weight at distance `x` (in filter units).
    pub fn weight(&self, x: f64) -> f64 {
        let x = (x / self.blur).abs();
        let k = match self.kernel {
            Kernel::Box => 1.0,
            Kernel::Triangle => (1.0 - x).max(0.0),
            Kernel::Cubic(b, c) => cubic_bc(b, c, x),
            Kernel::Sinc => sinc(x),
            Kernel::Gaussian(s) => (-x * x / (2.0 * s * s)).exp(),
            Kernel::Quadratic => {
                if x < 0.5 {
                    0.75 - x * x
                } else if x < 1.5 {
                    0.5 * (x - 1.5) * (x - 1.5)
                } else {
                    0.0
                }
            }
            Kernel::Jinc => {
                if x == 0.0 {
                    PI / 2.0
                } else {
                    bessel_j1(PI * x) / x
                }
            }
            Kernel::Lagrange => {
                let order = (2.0 * self.support - 1.0) as i64;
                if x >= self.support {
                    0.0
                } else {
                    let n = (self.support + x) as i64;
                    let mut v = 1.0;
                    for i in 0..=order {
                        if i != n {
                            v *= (n - i) as f64 - x;
                            v /= (n - i) as f64;
                        }
                    }
                    v
                }
            }
            Kernel::CubicSpline => {
                if x < 1.0 {
                    ((x - 1.8) * x - 0.2) * x + 1.0
                } else if x < 2.0 {
                    ((-x / 3.0 + 1.8) * x - 46.0 / 15.0) * x + 1.6
                } else {
                    0.0
                }
            }
        };
        if self.win_support <= 0.0 {
            return k;
        }
        let t = x / self.win_support;
        let w = match self.window {
            Window::Box => 1.0,
            Window::Hann => 0.5 + 0.5 * (PI * t).cos(),
            Window::Hamming => 0.54 + 0.46 * (PI * t).cos(),
            Window::Blackman => 0.42 + 0.5 * (PI * t).cos() + 0.08 * (2.0 * PI * t).cos(),
            Window::Welch => 1.0 - t * t,
            Window::Bohman => (1.0 - t) * (PI * t).cos() + (PI * t).sin() / PI,
            Window::Cosine => (PI * t / 2.0).cos(),
            Window::Triangle => 1.0 - t,
            Window::Sinc => sinc(t),
            Window::Kaiser(beta) => {
                bessel_i0(beta * (1.0 - t * t).max(0.0).sqrt()) / bessel_i0(beta)
            }
            Window::Spline => cubic_bc(1.0, 0.0, 2.0 * t),
        };
        k * w
    }

    /// The support scaled by the blur.
    pub fn practical_support(&self) -> f64 {
        self.support * self.blur
    }
}

/// One pass's contributions for each output index.
fn contributions(filter: &Filter, src: usize, dst: usize) -> Vec<(usize, Vec<f64>)> {
    let factor = dst as f64 / src as f64;
    let mut scale = (1.0 / factor).max(1.0);
    let mut support = scale * filter.practical_support();
    let point = support < 0.5;
    if point {
        support = 0.5;
        scale = 1.0;
    }
    let scale = 1.0 / scale;
    let mut out = Vec::with_capacity(dst);
    for x in 0..dst {
        // MagickEpsilon
        let bisect = (x as f64 + 0.5) / factor + 1.0e-12;
        let start = ((bisect - support + 0.5).max(0.0)) as usize;
        let stop = ((bisect + support + 0.5).min(src as f64)) as usize;
        let mut w: Vec<f64> = (start..stop.max(start))
            .map(|n| {
                if point {
                    1.0
                } else {
                    filter.weight(scale * (n as f64 - bisect + 0.5))
                }
            })
            .collect();
        let density: f64 = w.iter().sum();
        if density != 0.0 && density != 1.0 {
            for v in w.iter_mut() {
                *v /= density;
            }
        }
        out.push((start, w));
    }
    out
}

/// Resample `rows` lines of `len` samples (stride `step` between the
/// samples of a line, `line` between lines) into `dst` samples each.
fn pass(
    src: &[u16],
    alpha: bool,
    lines: usize,
    sample: impl Fn(usize, usize) -> usize,
    contrib: &[(usize, Vec<f64>)],
    mut put: impl FnMut(usize, usize, [u16; 4]),
) {
    let q = |v: f64| (v + 0.5).floor().clamp(0.0, 65535.0) as u16;
    for l in 0..lines {
        for (x, (start, w)) in contrib.iter().enumerate() {
            let mut acc = [0f64; 4];
            if alpha {
                let mut asum = 0.0;
                for (k, &wt) in w.iter().enumerate() {
                    let i = sample(l, start + k);
                    let a = src[i + 3] as f64 / 65535.0;
                    let aw = wt * a;
                    asum += aw;
                    acc[0] += aw * src[i] as f64;
                    acc[1] += aw * src[i + 1] as f64;
                    acc[2] += aw * src[i + 2] as f64;
                    acc[3] += wt * src[i + 3] as f64;
                }
                let g = if asum.abs() < f64::EPSILON {
                    1.0
                } else {
                    1.0 / asum
                };
                put(
                    l,
                    x,
                    [q(g * acc[0]), q(g * acc[1]), q(g * acc[2]), q(acc[3])],
                );
            } else {
                for (k, &wt) in w.iter().enumerate() {
                    let i = sample(l, start + k);
                    acc[0] += wt * src[i] as f64;
                    acc[1] += wt * src[i + 1] as f64;
                    acc[2] += wt * src[i + 2] as f64;
                }
                put(l, x, [q(acc[0]), q(acc[1]), q(acc[2]), 65535]);
            }
        }
    }
}

/// Resize the samples of an image (RGBA, plus black for CMYK, which is
/// carried along as a fifth channel through a second image).
pub fn resize_pixels(img: &Image, w: usize, h: usize, filter: &Filter) -> Vec<u16> {
    let (sw, sh) = (img.columns, img.rows);
    let alpha = img.alpha;
    let xc = contributions(filter, sw, w);
    let yc = contributions(filter, sh, h);
    // the order ImageMagick takes (it matters: each pass stores Q16)
    let horizontal_first = w * (sh + h) > h * (sw + w);
    if horizontal_first {
        let mut tmp = vec![0u16; w * sh * 4];
        pass(
            &img.pixels,
            alpha,
            sh,
            |l, n| (l * sw + n) * 4,
            &xc,
            |l, x, p| tmp[(l * w + x) * 4..(l * w + x) * 4 + 4].copy_from_slice(&p),
        );
        let mut out = vec![0u16; w * h * 4];
        pass(
            &tmp,
            alpha,
            w,
            |l, n| (n * w + l) * 4,
            &yc,
            |l, y, p| out[(y * w + l) * 4..(y * w + l) * 4 + 4].copy_from_slice(&p),
        );
        out
    } else {
        let mut tmp = vec![0u16; sw * h * 4];
        pass(
            &img.pixels,
            alpha,
            sw,
            |l, n| (n * sw + l) * 4,
            &yc,
            |l, y, p| tmp[(y * sw + l) * 4..(y * sw + l) * 4 + 4].copy_from_slice(&p),
        );
        let mut out = vec![0u16; w * h * 4];
        pass(
            &tmp,
            alpha,
            h,
            |l, n| (l * sw + n) * 4,
            &xc,
            |l, x, p| out[(l * w + x) * 4..(l * w + x) * 4 + 4].copy_from_slice(&p),
        );
        out
    }
}
