//! Image comparison metrics (GetImageDistortion() / CompareImages()), as
//! ImageMagick 7.1.2 reports them through the MagickWand API: the
//! normalised value (the one `magick compare` prints in parentheses).
//!
//! What black-box runs show:
//! - the images are compared over the union of their sizes, each image
//!   read through its virtual pixel method outside its own area;
//! - the channels are the first image's colour channels (gray images have
//!   one) plus its alpha channel; the colour samples are alpha-weighted
//!   (Sa·p − Da·q, an image without alpha being opaque) and the alpha
//!   channel counts only when both images have one, though it is still
//!   in the channel count;
//! - Fuzz is the mean squared difference over the samples that differ;
//! - NCC is (1 − r) / 2, r the mean over the channels of the channel
//!   covariance over the product of the images' composite (pooled)
//!   standard deviations, the deviations being those of the
//!   alpha-weighted samples from the plain channel means;
//! - SSIM and DSSIM both report the structural dissimilarity (1 − SSIM)/2
//!   averaged over the channels, from an 11×11 Gaussian window (σ 1.5,
//!   k1 0.01, k2 0.03) on the raw samples, a missing alpha reading 0;
//! - PHASH compares the image moment hashes (−log10 of the seven Hu
//!   invariants, 12 when not positive) of each image blurred by 0x1,
//!   in xyY and HSB: the squared differences summed, scaled by a constant.
//!   That is exact for flat images and close (a few %) for small photos;
//!   on large photos ImageMagick's values run higher, for reasons not yet
//!   pinned down. NCC of images with alpha is not exact either.

use crate::colorspace;
use crate::image::Image;
use crate::types::{Colorspace, MetricType, channel};

const Q: f64 = 65535.0;

/// The pixel channels (0..=2 colour, 3 alpha) a metric covers.
fn metric_channels(a: &Image, ch: i64) -> Vec<usize> {
    let all = ch == channel::ALL || ch == 0 || ch == 0x7fffffff;
    let colour: &[usize] = if a.colorspace == Colorspace::Gray {
        &[0]
    } else {
        &[0, 1, 2]
    };
    let mut v: Vec<usize> = if all {
        colour.to_vec()
    } else {
        let bits = [channel::RED, channel::GREEN, channel::BLUE];
        colour
            .iter()
            .copied()
            .filter(|&c| ch & bits[c] != 0)
            .collect()
    };
    if a.alpha && (all || ch & channel::ALPHA != 0) {
        v.push(3);
    }
    v
}

/// The two images' pixels over the union of their areas.
struct Pair<'a> {
    a: &'a Image,
    b: &'a Image,
    w: usize,
    h: usize,
}

impl<'a> Pair<'a> {
    fn new(a: &'a Image, b: &'a Image) -> Pair<'a> {
        Pair {
            a,
            b,
            w: a.columns.max(b.columns),
            h: a.rows.max(b.rows),
        }
    }

    fn px(&self, x: usize, y: usize) -> ([u16; 4], [u16; 4]) {
        (
            self.a.virtual_px(x as i64, y as i64),
            self.b.virtual_px(x as i64, y as i64),
        )
    }
}

/// The alpha-weighted difference of channel `c` (quantum scale).
#[inline]
fn weighted_diff(a: &Image, b: &Image, p: &[u16; 4], q: &[u16; 4], c: usize) -> f64 {
    let sa = if a.alpha { p[3] as f64 / Q } else { 1.0 };
    let da = if b.alpha { q[3] as f64 / Q } else { 1.0 };
    if c == 3 {
        if b.alpha {
            (p[3] as f64 - q[3] as f64) / Q
        } else {
            0.0
        }
    } else {
        (sa * p[c] as f64 - da * q[c] as f64) / Q
    }
}

/// The distortion of `b` against `a` by `metric` over the channels `ch`
/// selects.
pub fn distortion(a: &Image, b: &Image, metric: MetricType, ch: i64) -> f64 {
    let chans = metric_channels(a, ch);
    if chans.is_empty() {
        return 0.0;
    }
    match metric {
        MetricType::NormalizedCrossCorrelationError | MetricType::DotProductCorrelation => {
            // alpha correlates only when both images have it
            let chans: Vec<usize> = chans
                .iter()
                .copied()
                .filter(|&c| c < 3 || b.alpha)
                .collect();
            ncc(a, b, &chans)
        }
        MetricType::StructuralSimilarity | MetricType::StructuralDissimilarity => {
            dssim(a, b, &chans)
        }
        MetricType::PerceptualHash => phash(a, b, &chans),
        _ => pixel_metric(a, b, metric, &chans),
    }
}

fn pixel_metric(a: &Image, b: &Image, metric: MetricType, chans: &[usize]) -> f64 {
    let pr = Pair::new(a, b);
    let n = (pr.w * pr.h).max(1) as f64;
    let nch = chans.len() as f64;
    let fuzz = a.fuzz.max(b.fuzz) / Q;
    let (mut ae, mut sum_abs, mut sum_sq, mut peak) = (0usize, 0.0, 0.0, 0.0f64);
    let (mut fuzz_sq, mut fuzz_n) = (0.0, 0usize);
    for y in 0..pr.h {
        for x in 0..pr.w {
            let (p, q) = pr.px(x, y);
            let mut differs = false;
            for &c in chans {
                let d = weighted_diff(a, b, &p, &q, c);
                let ad = d.abs();
                if ad > fuzz {
                    differs = true;
                }
                if d != 0.0 {
                    fuzz_sq += d * d;
                    fuzz_n += 1;
                }
                sum_abs += ad;
                sum_sq += d * d;
                peak = peak.max(ad);
            }
            if differs {
                ae += 1;
            }
        }
    }
    let mse = sum_sq / (n * nch);
    match metric {
        MetricType::AbsoluteError => ae as f64 / n,
        MetricType::MeanAbsoluteError | MetricType::MeanErrorPerPixel => sum_abs / (n * nch),
        MetricType::PeakAbsoluteError => peak,
        MetricType::PeakSignalToNoiseRatio => {
            if mse == 0.0 {
                1.0
            } else {
                (10.0 * (1.0 / mse).log10() / 120.0).min(1.0)
            }
        }
        MetricType::Fuzz => {
            if fuzz_n == 0 {
                0.0
            } else {
                fuzz_sq / fuzz_n as f64
            }
        }
        MetricType::RootMeanSquaredError | MetricType::Undefined => mse.sqrt(),
        _ => mse,
    }
}

fn ncc(a: &Image, b: &Image, chans: &[usize]) -> f64 {
    let pr = Pair::new(a, b);
    let n = (pr.w * pr.h).max(1) as f64;
    let k = chans.len();
    let sample = |img: &Image, p: &[u16; 4], c: usize| -> f64 {
        if c == 3 {
            if img.alpha { p[3] as f64 / Q } else { 0.0 }
        } else {
            p[c] as f64 / Q
        }
    };
    let (mut ma, mut mb) = (vec![0.0; k], vec![0.0; k]);
    for y in 0..pr.h {
        for x in 0..pr.w {
            let (p, q) = pr.px(x, y);
            for (i, &c) in chans.iter().enumerate() {
                ma[i] += sample(a, &p, c);
                mb[i] += sample(b, &q, c);
            }
        }
    }
    for i in 0..k {
        ma[i] /= n;
        mb[i] /= n;
        // a gray image has no green or blue statistics: their mean is 0,
        // though its samples read as the gray
        if b.colorspace == Colorspace::Gray && (1..3).contains(&chans[i]) {
            mb[i] = 0.0;
        }
    }
    // the deviations are of the alpha-weighted colour samples from the
    // plain channel means
    let weighted = |img: &Image, p: &[u16; 4], c: usize| -> f64 {
        let s = sample(img, p, c);
        if c < 3 && img.alpha {
            s * p[3] as f64 / Q
        } else {
            s
        }
    };
    let (mut va, mut vb, mut cov) = (vec![0.0; k], vec![0.0; k], vec![0.0; k]);
    for y in 0..pr.h {
        for x in 0..pr.w {
            let (p, q) = pr.px(x, y);
            for (i, &c) in chans.iter().enumerate() {
                let da = weighted(a, &p, c) - ma[i];
                let db = weighted(b, &q, c) - mb[i];
                va[i] += da * da;
                vb[i] += db * db;
                cov[i] += da * db;
            }
        }
    }
    let sa = (va.iter().sum::<f64>() / (n * k as f64)).sqrt();
    let sb = (vb.iter().sum::<f64>() / (n * k as f64)).sqrt();
    let r = if sa * sb > 1e-12 {
        cov.iter().map(|c| c / n / (sa * sb)).sum::<f64>() / k as f64
    } else {
        0.0
    };
    (1.0 - r) / 2.0
}

/// Separable blur with a normalised kernel and edge virtual pixels.
fn blur_plane(src: &[f64], w: usize, h: usize, k: &[f64]) -> Vec<f64> {
    let r = (k.len() / 2) as i64;
    let mut tmp = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let sx = (x as i64 + j as i64 - r).clamp(0, w as i64 - 1) as usize;
                s += kv * src[y * w + sx];
            }
            tmp[y * w + x] = s;
        }
    }
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let sy = (y as i64 + j as i64 - r).clamp(0, h as i64 - 1) as usize;
                s += kv * tmp[sy * w + x];
            }
            out[y * w + x] = s;
        }
    }
    out
}

fn gaussian_1d(radius: usize, sigma: f64) -> Vec<f64> {
    let r = radius as i64;
    let k: Vec<f64> = (-r..=r)
        .map(|i| (-((i * i) as f64) / (2.0 * sigma * sigma)).exp())
        .collect();
    let s: f64 = k.iter().sum();
    k.into_iter().map(|v| v / s).collect()
}

/// A channel plane over the union area (raw samples in [0, 1]).
fn plane(img: &Image, c: usize, w: usize, h: usize) -> Vec<f64> {
    let mut v = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let p = img.virtual_px(x as i64, y as i64);
            v.push(if c == 3 && !img.alpha {
                0.0
            } else {
                p[c] as f64 / Q
            });
        }
    }
    v
}

fn dssim(a: &Image, b: &Image, chans: &[usize]) -> f64 {
    let (w, h) = (a.columns.max(b.columns), a.rows.max(b.rows));
    if w == 0 || h == 0 {
        return 0.0;
    }
    let k = gaussian_1d(5, 1.5);
    let (c1, c2) = (0.01f64 * 0.01, 0.03f64 * 0.03);
    let mut total = 0.0;
    for &c in chans {
        if c == 3 && !b.alpha {
            // a channel the reconstruction lacks is wholly dissimilar
            total += 0.5;
            continue;
        }
        let x = plane(a, c, w, h);
        let y = plane(b, c, w, h);
        let xx: Vec<f64> = x.iter().map(|v| v * v).collect();
        let yy: Vec<f64> = y.iter().map(|v| v * v).collect();
        let xy: Vec<f64> = x.iter().zip(&y).map(|(p, q)| p * q).collect();
        let (mx, my) = (blur_plane(&x, w, h, &k), blur_plane(&y, w, h, &k));
        let (sxx, syy, sxy) = (
            blur_plane(&xx, w, h, &k),
            blur_plane(&yy, w, h, &k),
            blur_plane(&xy, w, h, &k),
        );
        let mut s = 0.0;
        for i in 0..w * h {
            let (ux, uy) = (mx[i], my[i]);
            let vx = sxx[i] - ux * ux;
            let vy = syy[i] - uy * uy;
            let cxy = sxy[i] - ux * uy;
            s += (2.0 * ux * uy + c1) * (2.0 * cxy + c2)
                / ((ux * ux + uy * uy + c1) * (vx + vy + c2));
        }
        total += (1.0 - s / (w * h) as f64) / 2.0;
    }
    total / chans.len() as f64
}

/// The −log10 of the seven Hu moment invariants of a plane.
fn hu_hash(f: &[f64], w: usize, h: usize) -> [f64; 7] {
    let mut m00 = 0.0;
    let (mut m10, mut m01) = (0.0, 0.0);
    for y in 0..h {
        for x in 0..w {
            let v = f[y * w + x];
            m00 += v;
            m10 += x as f64 * v;
            m01 += y as f64 * v;
        }
    }
    let mut out = [12.0; 7];
    if m00 <= 0.0 {
        return out;
    }
    let (cx, cy) = (m10 / m00, m01 / m00);
    // central moments mu[p][q], p + q <= 3
    let mut mu = [[0.0f64; 4]; 4];
    for y in 0..h {
        let dy = y as f64 - cy;
        for x in 0..w {
            let v = f[y * w + x];
            if v == 0.0 {
                continue;
            }
            let dx = x as f64 - cx;
            let (dx2, dy2) = (dx * dx, dy * dy);
            mu[2][0] += dx2 * v;
            mu[0][2] += dy2 * v;
            mu[1][1] += dx * dy * v;
            mu[3][0] += dx2 * dx * v;
            mu[0][3] += dy2 * dy * v;
            mu[2][1] += dx2 * dy * v;
            mu[1][2] += dx * dy2 * v;
        }
    }
    let eta = |p: usize, q: usize| mu[p][q] / m00.powf(1.0 + (p + q) as f64 / 2.0);
    let (n20, n02, n11) = (eta(2, 0), eta(0, 2), eta(1, 1));
    let (n30, n03, n21, n12) = (eta(3, 0), eta(0, 3), eta(2, 1), eta(1, 2));
    let (s1, s2) = (n30 + n12, n21 + n03);
    let i = [
        n20 + n02,
        (n20 - n02).powi(2) + 4.0 * n11 * n11,
        (n30 - 3.0 * n12).powi(2) + (3.0 * n21 - n03).powi(2),
        s1 * s1 + s2 * s2,
        (n30 - 3.0 * n12) * s1 * (s1 * s1 - 3.0 * s2 * s2)
            + (3.0 * n21 - n03) * s2 * (3.0 * s1 * s1 - s2 * s2),
        (n20 - n02) * (s1 * s1 - s2 * s2) + 4.0 * n11 * s1 * s2,
        (3.0 * n21 - n03) * s1 * (s1 * s1 - 3.0 * s2 * s2)
            - (n30 - 3.0 * n12) * s2 * (3.0 * s1 * s1 - s2 * s2),
    ];
    for k in 0..7 {
        if i[k] > 1e-12 {
            out[k] = -i[k].log10();
        }
    }
    out
}

/// The 1-D kernel of BlurImage() for sigma 1: Gaussian weights each the
/// sum of three sub-pixel samples, radius 4.
fn phash_blur_kernel() -> Vec<f64> {
    let k: Vec<f64> = (-4i64..=4)
        .map(|x| {
            (-1..=1)
                .map(|j| {
                    let u = x as f64 + j as f64 / 3.0;
                    (-u * u / 2.0).exp()
                })
                .sum()
        })
        .collect();
    let s: f64 = k.iter().sum();
    k.into_iter().map(|v| v / s).collect()
}

/// An image's perceptual hash: [channel][moment][colourspace].
fn perceptual_hash(img: &Image) -> [[[f64; 2]; 7]; 3] {
    let (w, h) = (img.columns, img.rows);
    let k = phash_blur_kernel();
    let r = 4i64;
    let round = |v: f64| (v * Q).round().clamp(0.0, Q) / Q;
    // blur the sRGB samples (Q16 after each pass)
    let mut planes: Vec<Vec<f64>> = Vec::with_capacity(3);
    for c in 0..3 {
        let src: Vec<f64> = img
            .pixels
            .chunks_exact(4)
            .map(|p| p[c] as f64 / Q)
            .collect();
        let mut tmp = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for (j, kv) in k.iter().enumerate() {
                    let sx = (x as i64 + j as i64 - r).clamp(0, w as i64 - 1) as usize;
                    s += kv * src[y * w + sx];
                }
                tmp[y * w + x] = round(s);
            }
        }
        let mut out = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for (j, kv) in k.iter().enumerate() {
                    let sy = (y as i64 + j as i64 - r).clamp(0, h as i64 - 1) as usize;
                    s += kv * tmp[sy * w + x];
                }
                out[y * w + x] = round(s);
            }
        }
        planes.push(out);
    }
    let mut hash = [[[12.0; 2]; 7]; 3];
    for (ci, cs) in [Colorspace::XyY, Colorspace::HSB].into_iter().enumerate() {
        let mut conv = vec![vec![0.0; w * h]; 3];
        for i in 0..w * h {
            let c = [planes[0][i], planes[1][i], planes[2][i]];
            let v = colorspace::from_srgb(cs, c).unwrap_or(c);
            for k in 0..3 {
                conv[k][i] = round(v[k]);
            }
        }
        for k in 0..3 {
            let hv = hu_hash(&conv[k], w, h);
            for m in 0..7 {
                hash[k][m][ci] = hv[m];
            }
        }
    }
    hash
}

/// The scale ImageMagick applies to the summed squared hash differences
/// of a channel (black-box: constant over image sizes and values).
const PHASH_SCALE: f64 = 0.002_568_2;

fn phash(a: &Image, b: &Image, chans: &[usize]) -> f64 {
    let (ha, mut hb) = (perceptual_hash(a), perceptual_hash(b));
    let colour: Vec<usize> = chans.iter().copied().filter(|&c| c < 3).collect();
    if colour.is_empty() {
        return 0.0;
    }
    let a_gray = a.colorspace == Colorspace::Gray;
    if !a_gray && b.colorspace == Colorspace::Gray {
        // a gray reconstruction hashes its one channel; the others are 0
        hb[1] = [[0.0; 2]; 7];
        hb[2] = [[0.0; 2]; 7];
    }
    let mut total = 0.0;
    for &c in &colour {
        // a gray image is compared on its intensity (Y and B) hashes
        let (ca, cb) = if a_gray {
            (
                2,
                if b.colorspace == Colorspace::Gray {
                    2
                } else {
                    0
                },
            )
        } else {
            (c, c)
        };
        let mut s = 0.0;
        for m in 0..7 {
            for k in 0..2 {
                let d = ha[ca][m][k] - hb[cb][m][k];
                s += d * d;
            }
        }
        total += s * PHASH_SCALE;
    }
    total / colour.len() as f64
}
