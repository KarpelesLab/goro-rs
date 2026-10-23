//! Q16 convolution: the blur and gaussian kernels with ImageMagick's
//! widths and sampling, and a generic kernel convolution over the image's
//! virtual pixels. Measured against ImageMagick 7.1.2 Q16 black-box:
//!
//! - BlurImage() is separable: a 1-D gaussian whose taps average the
//!   curve at -1/3, 0 and +1/3 of a pixel, normalised; the horizontal
//!   pass is stored at Q16 before the vertical one.
//! - GaussianBlurImage() is one 2-D gaussian sampled at the pixel
//!   offsets, normalised.
//! - With radius 0 the half-width is the largest offset at which the
//!   continuous normalised gaussian is still at least 1/QuantumRange.
//! - Colour channels of images with alpha are alpha-weighted.

use crate::color::clamp_to_quantum;
use crate::filters::channel_mask;
use crate::image::Image;

/// The kernel half-width for `radius` (when ≥ 1) or from `sigma`, for a
/// 1-D kernel.
pub fn kernel_radius(radius: f64, sigma: f64) -> usize {
    width_for(radius, sigma, false)
}

/// The same for a 2-D kernel, normalised over the plane.
pub fn kernel_radius_2d(radius: f64, sigma: f64) -> usize {
    width_for(radius, sigma, true)
}

fn width_for(radius: f64, sigma: f64, two_d: bool) -> usize {
    if radius >= 1.0 {
        return radius as usize;
    }
    let s = sigma.abs().max(1e-6);
    let norm = if two_d {
        1.0 / (2.0 * std::f64::consts::PI * s * s)
    } else {
        1.0 / ((2.0 * std::f64::consts::PI).sqrt() * s)
    };
    let mut w = 1usize;
    while w < 1000 {
        let x = (w + 1) as f64;
        if (-(x * x) / (2.0 * s * s)).exp() * norm < 1.0 / 65535.0 {
            break;
        }
        w += 1;
    }
    w
}

/// BlurImage()'s 1-D kernel: each tap averages three sub-samples.
pub fn blur_kernel(w: usize, sigma: f64) -> Vec<f64> {
    let s = sigma.abs().max(1e-6);
    let mut k: Vec<f64> = (0..2 * w + 1)
        .map(|i| {
            let x = i as f64 - w as f64;
            [-1.0 / 3.0, 0.0, 1.0 / 3.0]
                .iter()
                .map(|d| {
                    let t = x + d;
                    (-(t * t) / (2.0 * s * s)).exp()
                })
                .sum::<f64>()
        })
        .collect();
    let sum: f64 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    k
}

/// A normalised 2-D gaussian sampled at the integer offsets.
pub fn gaussian_kernel(w: usize, sigma: f64) -> Vec<f64> {
    let s = sigma.abs().max(1e-6);
    let n = 2 * w + 1;
    let mut k = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f64 - w as f64, y as f64 - w as f64);
            k.push((-(dx * dx + dy * dy) / (2.0 * s * s)).exp());
        }
    }
    let sum: f64 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    k
}

/// The image's samples as f64 RGBA with `px`/`py` pixels of virtual
/// border on each side.
fn padded(img: &Image, px: usize, py: usize) -> (Vec<f64>, usize) {
    let (w, h) = (img.columns, img.rows);
    let pw = w + 2 * px;
    let ph = h + 2 * py;
    let mut out = vec![0f64; pw * ph * 4];
    for y in 0..ph {
        let sy = y as i64 - py as i64;
        for x in 0..pw {
            let sx = x as i64 - px as i64;
            let p = if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                img.px(sx as usize, sy as usize)
            } else {
                img.virtual_px(sx, sy)
            };
            let o = (y * pw + x) * 4;
            for c in 0..4 {
                out[o + c] = p[c] as f64;
            }
        }
    }
    (out, pw)
}

/// Convolve the selected channels with a `kw`×`kh` kernel (row-major,
/// origin at its centre), plus `bias` (in quantum units).
pub fn convolve(img: &mut Image, kw: usize, kh: usize, kernel: &[f64], ch: i64, bias: f64) {
    if img.columns == 0 || img.rows == 0 || kernel.len() != kw * kh {
        return;
    }
    let mask = channel_mask(ch, img.alpha);
    let (ox, oy) = (kw / 2, kh / 2);
    let (src, pw) = padded(img, ox, oy);
    let (w, h) = (img.columns, img.rows);
    let alpha = img.alpha;
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f64; 4];
            let mut gamma = 0f64;
            for j in 0..kh {
                let row = (y + j) * pw + x;
                for i in 0..kw {
                    let k = kernel[j * kw + i];
                    if k == 0.0 {
                        continue;
                    }
                    let o = (row + i) * 4;
                    if alpha {
                        let a = src[o + 3] / 65535.0;
                        acc[0] += k * a * src[o];
                        acc[1] += k * a * src[o + 1];
                        acc[2] += k * a * src[o + 2];
                        gamma += k * a;
                    } else {
                        acc[0] += k * src[o];
                        acc[1] += k * src[o + 1];
                        acc[2] += k * src[o + 2];
                    }
                    acc[3] += k * src[o + 3];
                }
            }
            if alpha {
                let ksum: f64 = kernel.iter().sum();
                let g = if gamma.abs() < 1e-12 {
                    1.0
                } else {
                    ksum / gamma
                };
                for v in acc.iter_mut().take(3) {
                    *v *= g;
                }
            }
            let idx = (y * w + x) * 4;
            for c in 0..4 {
                if mask[c] {
                    img.pixels[idx + c] = clamp_to_quantum(acc[c] + bias) as u16;
                }
            }
        }
    }
    img.colors = None;
}

/// BlurImage(): the separable sub-sampled gaussian.
pub fn blur(img: &mut Image, radius: f64, sigma: f64, ch: i64) {
    let w = kernel_radius(radius, sigma);
    let k = blur_kernel(w, sigma);
    convolve(img, k.len(), 1, &k, ch, 0.0);
    convolve(img, 1, k.len(), &k, ch, 0.0);
}

/// GaussianBlurImage(): the 2-D gaussian.
pub fn gaussian_blur(img: &mut Image, radius: f64, sigma: f64, ch: i64) {
    let w = kernel_radius_2d(radius, sigma);
    let k = gaussian_kernel(w, sigma);
    convolve(img, 2 * w + 1, 2 * w + 1, &k, ch, 0.0);
}
