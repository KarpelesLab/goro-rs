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

/// The kernel half-width: the integer part of `radius` (at least 1) when
/// it is positive, else from `sigma`, for a 1-D kernel.
pub fn kernel_radius(radius: f64, sigma: f64) -> usize {
    width_for(radius, sigma, false)
}

/// The same for a 2-D kernel, normalised over the plane.
pub fn kernel_radius_2d(radius: f64, sigma: f64) -> usize {
    width_for(radius, sigma, true)
}

fn width_for(radius: f64, sigma: f64, two_d: bool) -> usize {
    if radius > 0.0 {
        return (radius as usize).max(1);
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

/// EdgeImage(): a (2w+1)² kernel of -1 with the centre balancing it; the
/// half-width is ⌈radius⌉, or 2 for radius 0.
pub fn edge(img: &mut Image, radius: f64) {
    let w = if radius > 0.0 {
        radius.ceil() as usize
    } else {
        kernel_radius(0.0, 0.5)
    };
    let n = 2 * w + 1;
    let mut k = vec![-1.0; n * n];
    k[w * n + w] = (n * n) as f64 - 1.0;
    convolve(img, n, n, &k, crate::types::channel::DEFAULT, 0.0);
}

/// EmbossImage(): a gaussian-weighted anti-diagonal, positive at the
/// centre and negative elsewhere, normalised to a unit sum, then
/// EqualizeImage().
pub fn emboss(img: &mut Image, radius: f64, sigma: f64) {
    let w = kernel_radius(radius, sigma);
    let n = 2 * w + 1;
    let s = sigma.abs().max(1e-6);
    let mut k = vec![0.0; n * n];
    for v in 0..n {
        let dv = v as f64 - w as f64;
        let u = n - 1 - v;
        let du = u as f64 - w as f64;
        let g = (-(du * du + dv * dv) / (2.0 * s * s)).exp();
        k[v * n + u] = if u == w { g } else { -g };
    }
    let sum: f64 = k.iter().sum();
    if sum.abs() > 1e-12 {
        for x in &mut k {
            *x /= sum;
        }
    }
    convolve(img, n, n, &k, crate::types::channel::DEFAULT, 0.0);
    crate::enhance::equalize(img);
}

/// ShadeImage(): the normal from the 3×3 sums of the rounded intensity
/// (z = 2·QuantumRange) lit from (azimuth, elevation); `gray` gives the
/// shade itself, else it scales the pixel.
pub fn shade(img: &mut Image, gray: bool, azimuth: f64, elevation: f64) {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return;
    }
    let q = 65535.0;
    let (az, el) = (azimuth.to_radians(), elevation.to_radians());
    let light = [
        q * az.cos() * el.cos(),
        q * az.sin() * el.cos(),
        q * el.sin(),
    ];
    let (src, pw) = padded(img, 1, 1);
    let int: Vec<f64> = src
        .chunks_exact(4)
        .map(|p| clamp_to_quantum(0.212656 * p[0] + 0.715158 * p[1] + 0.072186 * p[2]))
        .collect();
    let at = |x: usize, y: usize| int[y * pw + x];
    let nz = 2.0 * q;
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x + 1, y + 1);
            let nx = at(cx - 1, cy - 1) + at(cx - 1, cy) + at(cx - 1, cy + 1)
                - at(cx + 1, cy - 1)
                - at(cx + 1, cy)
                - at(cx + 1, cy + 1);
            let ny = at(cx - 1, cy + 1) + at(cx, cy + 1) + at(cx + 1, cy + 1)
                - at(cx - 1, cy - 1)
                - at(cx, cy - 1)
                - at(cx + 1, cy - 1);
            let s = if nx == 0.0 && ny == 0.0 {
                light[2]
            } else {
                let dot = nx * light[0] + ny * light[1] + nz * light[2];
                if dot > 0.0 {
                    dot / (nx * nx + ny * ny + nz * nz).sqrt()
                } else {
                    0.0
                }
            };
            let idx = (y * w + x) * 4;
            for c in 0..3 {
                img.pixels[idx + c] = if gray {
                    clamp_to_quantum(s) as u16
                } else {
                    clamp_to_quantum(s * img.pixels[idx + c] as f64 / q) as u16
                };
            }
        }
    }
    img.colors = None;
}

/// SharpenImage(): a negated 2-D gaussian whose centre is twice the
/// gaussian's sum, normalised to a unit sum.
pub fn sharpen(img: &mut Image, radius: f64, sigma: f64, ch: i64) {
    let w = kernel_radius_2d(radius, sigma);
    let n = 2 * w + 1;
    let s = sigma.abs().max(1e-6);
    let mut k = vec![0.0; n * n];
    let mut total = 0.0;
    for v in 0..n {
        for u in 0..n {
            let (du, dv) = (u as f64 - w as f64, v as f64 - w as f64);
            let g = (-(du * du + dv * dv) / (2.0 * s * s)).exp();
            k[v * n + u] = -g;
            total += g;
        }
    }
    k[w * n + w] = 2.0 * total;
    let sum: f64 = k.iter().sum();
    for x in &mut k {
        *x /= sum;
    }
    convolve(img, n, n, &k, ch, 0.0);
}

/// UnsharpMaskImage(): BlurImage(radius, sigma); a channel whose
/// difference from the blur is at least half the threshold moves by
/// `amount` times that difference.
pub fn unsharp(img: &mut Image, radius: f64, sigma: f64, amount: f64, threshold: f64, ch: i64) {
    let mut b = img.clone();
    blur(&mut b, radius, sigma, ch);
    let mask = channel_mask(ch, img.alpha);
    let thr = threshold * 65535.0;
    for (p, q) in img.pixels.chunks_exact_mut(4).zip(b.pixels.chunks_exact(4)) {
        for c in 0..4 {
            if !mask[c] {
                continue;
            }
            let d = p[c] as f64 - q[c] as f64;
            if (2.0 * d).abs() >= thr {
                p[c] = clamp_to_quantum(p[c] as f64 + d * amount) as u16;
            }
        }
    }
    img.colors = None;
}

/// MotionBlurImage(): 2r+1 one-sided gaussian taps (or twice the
/// sigma-derived half-width plus one) along `angle`, at rounded offsets.
pub fn motion_blur(img: &mut Image, radius: f64, sigma: f64, angle: f64, ch: i64) {
    let n = 2 * kernel_radius(radius, sigma) + 1;
    let s = sigma.abs().max(1e-6);
    let mut k: Vec<f64> = (0..n)
        .map(|i| (-((i * i) as f64) / (2.0 * s * s)).exp())
        .collect();
    let sum: f64 = k.iter().sum();
    for x in &mut k {
        *x /= sum;
    }
    let t = angle.to_radians();
    let offs: Vec<(i64, i64)> = (0..n)
        .map(|i| {
            let f = i as f64;
            (
                (f * t.cos() - 0.5).ceil() as i64,
                (f * t.sin() - 0.5).ceil() as i64,
            )
        })
        .collect();
    let src = img.clone();
    let mask = channel_mask(ch, img.alpha);
    let (w, h) = (img.columns, img.rows);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f64; 4];
            for (kv, &(dx, dy)) in k.iter().zip(&offs) {
                let p = src.virtual_px(x as i64 + dx, y as i64 + dy);
                for c in 0..4 {
                    acc[c] += kv * p[c] as f64;
                }
            }
            let idx = (y * w + x) * 4;
            for c in 0..4 {
                if mask[c] {
                    img.pixels[idx + c] = clamp_to_quantum(acc[c]) as u16;
                }
            }
        }
    }
    img.colors = None;
}

/// CharcoalImage() as pecl's charcoalImage(radius, sigma) drives it:
/// EdgeImage(sigma), BlurImage(sigma, radius), NormalizeImage(),
/// NegateImage(), then grayscale.
pub fn charcoal(img: &mut Image, radius: f64, sigma: f64) {
    edge(img, sigma);
    blur(img, sigma, radius, crate::types::channel::DEFAULT);
    crate::enhance::normalize(img, crate::types::channel::DEFAULT);
    for p in img.pixels.chunks_exact_mut(4) {
        for c in p.iter_mut().take(3) {
            *c = 65535 - *c;
        }
    }
    crate::enhance::grayscale(img);
}

/// StatisticImage(): each selected channel replaced by a statistic of
/// its `w`×`h` neighbourhood (virtual pixels outside), in Q16. `kind` is
/// the StatisticType: gradient (max - min), maximum, mean, median,
/// minimum, mode, nonpeak, root mean square, standard deviation; any
/// other kind gives black, as ImageMagick 7.1.2's Contrast does.
pub fn statistic(img: &mut Image, kind: i64, w: usize, h: usize, ch: i64) {
    if img.columns == 0 || img.rows == 0 {
        return;
    }
    let mask = channel_mask(ch, img.alpha);
    let (ox, oy) = (w / 2, h / 2);
    let (src, pw) = padded(img, w, h);
    let (iw, ih) = (img.columns, img.rows);
    let n = w * h;
    let mut buf: Vec<u16> = Vec::with_capacity(n);
    for y in 0..ih {
        for x in 0..iw {
            let idx = (y * iw + x) * 4;
            for c in 0..4 {
                if !mask[c] {
                    continue;
                }
                buf.clear();
                for j in 0..h {
                    let row = (y + h + j - oy) * pw + x + w - ox;
                    for i in 0..w {
                        buf.push(src[(row + i) * 4 + c] as u16);
                    }
                }
                let v = stat_of(&mut buf, kind);
                img.pixels[idx + c] = clamp_to_quantum(v) as u16;
            }
        }
    }
    img.colors = None;
}

fn stat_of(buf: &mut [u16], kind: i64) -> f64 {
    let n = buf.len() as f64;
    match kind {
        1 | 2 | 5 | 4 | 6 | 7 => {
            buf.sort_unstable();
            let (lo, hi) = (buf[0] as f64, buf[buf.len() - 1] as f64);
            let median = buf[buf.len() / 2] as f64;
            match kind {
                1 => hi - lo,
                2 => hi,
                5 => lo,
                6 => {
                    // the most frequent value (the lowest of a tie)
                    let (mut best, mut best_n, mut k) = (buf[0], 0usize, 0usize);
                    while k < buf.len() {
                        let mut e = k;
                        while e < buf.len() && buf[e] == buf[k] {
                            e += 1;
                        }
                        if e - k > best_n {
                            best_n = e - k;
                            best = buf[k];
                        }
                        k = e;
                    }
                    best as f64
                }
                7 => {
                    // the median, stepped off the window's extremes
                    if median == hi {
                        buf.iter()
                            .rev()
                            .map(|&v| v as f64)
                            .find(|&v| v < hi)
                            .unwrap_or(median)
                    } else if median == lo {
                        buf.iter()
                            .map(|&v| v as f64)
                            .find(|&v| v > lo)
                            .unwrap_or(median)
                    } else {
                        median
                    }
                }
                _ => median,
            }
        }
        3 => buf.iter().map(|&v| v as f64).sum::<f64>() / n,
        8 => (buf.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / n).sqrt(),
        9 => {
            let m = buf.iter().map(|&v| v as f64).sum::<f64>() / n;
            let sq = buf.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / n;
            (sq - m * m).max(0.0).sqrt()
        }
        _ => 0.0,
    }
}
