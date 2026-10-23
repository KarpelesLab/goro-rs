//! Histogram and tone operations measured against ImageMagick 7.1.2 Q16:
//! contrast stretch / normalize, equalize, and ContrastImage().

use crate::color::{clamp_to_quantum, to_q16};
use crate::filters::channel_mask;
use crate::image::Image;
use crate::types::channel;

/// ContrastStretchImage(): the black point is the first histogram bin
/// whose cumulative count (from the bottom) exceeds `black`, the white
/// point the last bin whose count from the top exceeds `n - white`. With
/// the default channels the histogram is of the pixel intensity
/// (Rec. 709 luma, rounded to a bin) and one stretch applies to R, G and
/// B; otherwise every selected channel is stretched on its own
/// histogram. A degenerate range leaves the channel alone.
pub fn histogram_stretch(i: &mut Image, ch: i64, black: f64, white: f64) {
    let n = (i.columns * i.rows) as f64;
    let range = |hist: &[u32]| -> Option<(f64, f64)> {
        let mut acc = 0f64;
        let mut lo = 65535usize;
        for (v, &h) in hist.iter().enumerate() {
            acc += h as f64;
            if acc > black {
                lo = v;
                break;
            }
        }
        let mut acc = 0f64;
        let mut hi = 0usize;
        for v in (0..65536).rev() {
            acc += hist[v] as f64;
            if acc > n - white {
                hi = v;
                break;
            }
        }
        (hi > lo).then_some((lo as f64, hi as f64))
    };
    let all = ch == channel::DEFAULT || ch == 0 || ch == 0x7fffffff;
    let mut ranges = [None; 3];
    if all {
        let mut hist = vec![0u32; 65536];
        for p in i.pixels.chunks_exact(4) {
            hist[clamp_to_quantum(Image::intensity(p)) as usize] += 1;
        }
        let r = range(&hist);
        ranges = [r, r, r];
    } else {
        let mask = channel_mask(ch, i.alpha);
        for c in 0..3 {
            if mask[c] {
                let mut hist = vec![0u32; 65536];
                for p in i.pixels.chunks_exact(4) {
                    hist[p[c] as usize] += 1;
                }
                ranges[c] = range(&hist);
            }
        }
    }
    for p in i.pixels.chunks_exact_mut(4) {
        for c in 0..3 {
            if let Some((lo, hi)) = ranges[c] {
                p[c] = to_q16((p[c] as f64 - lo) * 65535.0 / (hi - lo));
            }
        }
    }
    i.colors = None;
}

/// NormalizeImage(): a 2% / 1% stretch.
pub fn normalize(i: &mut Image, ch: i64) {
    let n = (i.columns * i.rows) as f64;
    histogram_stretch(i, ch, 0.02 * n, 0.99 * n);
}

/// EqualizeImage(): one map for R, G and B, from the cumulative histogram
/// of the intensity, with the black bin's count taken off.
pub fn equalize(i: &mut Image) {
    let mut hist = vec![0u32; 65536];
    for p in i.pixels.chunks_exact(4) {
        hist[clamp_to_quantum(Image::intensity(p)) as usize] += 1;
    }
    let mut cdf = vec![0f64; 65536];
    let mut acc = 0f64;
    for (v, &h) in hist.iter().enumerate() {
        acc += h as f64;
        cdf[v] = acc;
    }
    let (lo, hi) = (cdf[0], cdf[65535]);
    if hi <= lo {
        return;
    }
    let map: Vec<u16> = cdf
        .iter()
        .map(|&c| to_q16((c - lo) * 65535.0 / (hi - lo)))
        .collect();
    for p in i.pixels.chunks_exact_mut(4) {
        for c in p.iter_mut().take(3) {
            *c = map[*c as usize];
        }
    }
    i.colors = None;
}

/// ContrastImage(): the HSB brightness is pushed away from (or toward)
/// mid-gray along a sine curve, hue and saturation kept, so the RGB
/// samples scale together.
pub fn contrast_hsb(i: &mut Image, sharpen: bool) {
    let sign = if sharpen { 0.5 } else { -0.5 };
    for p in i.pixels.chunks_exact_mut(4) {
        let v = p[0].max(p[1]).max(p[2]) as f64 / 65535.0;
        if v <= 0.0 {
            continue;
        }
        let s = 0.5 * ((std::f64::consts::PI * (v - 0.5)).sin() + 1.0);
        let nv = (v + sign * (s - v)).clamp(0.0, 1.0);
        let k = nv / v;
        for c in p.iter_mut().take(3) {
            *c = to_q16(*c as f64 * k);
        }
    }
    i.colors = None;
}

/// GrayscaleImage() with the default intensity: Rec. 709 luma of the
/// stored samples, rounded, in the Gray colourspace.
pub fn grayscale(i: &mut Image) {
    for p in i.pixels.chunks_exact_mut(4) {
        let v = clamp_to_quantum(Image::intensity(p)) as u16;
        p[0] = v;
        p[1] = v;
        p[2] = v;
    }
    i.colorspace = crate::types::Colorspace::Gray;
    i.colors = None;
}

/// AutoThresholdImage(): a threshold bin from the 256-bin histogram of
/// the rounded intensity (Kapur 1, Otsu 2, Triangle 3); pixels whose
/// intensity exceeds bin·257 go white, the rest black, in all channels.
pub fn auto_threshold(i: &mut Image, method: i64) {
    let mut h = [0f64; 256];
    for p in i.pixels.chunks_exact(4) {
        h[(Image::intensity(p) / 257.0 + 0.5)
            .floor()
            .clamp(0.0, 255.0) as usize] += 1.0;
    }
    let n: f64 = h.iter().sum();
    if n == 0.0 {
        return;
    }
    let p: Vec<f64> = h.iter().map(|v| v / n).collect();
    let t = match method {
        1 => {
            // Kapur: the largest sum of the two classes' entropies
            let (mut best, mut bt) = (f64::MIN, 0usize);
            for t in 0..256 {
                let w0: f64 = p[..=t].iter().sum();
                let w1 = 1.0 - w0;
                if w0 <= 0.0 || w1 <= 0.0 {
                    continue;
                }
                let ent = |s: &[f64], w: f64| {
                    s.iter()
                        .filter(|&&v| v > 0.0)
                        .map(|&v| -(v / w) * (v / w).ln())
                        .sum::<f64>()
                };
                let e = ent(&p[..=t], w0) + ent(&p[t + 1..], w1);
                if e > best {
                    best = e;
                    bt = t;
                }
            }
            bt
        }
        3 => {
            // Triangle: the bin farthest from the line joining the peak to
            // the far end of the histogram (one past its outermost
            // non-empty bin)
            let first = h.iter().position(|&v| v > 0.0).unwrap_or(0);
            let last = h.iter().rposition(|&v| v > 0.0).unwrap_or(255);
            let peak = (0..256).fold(0, |b, k| if h[k] > h[b] { k } else { b });
            let (x0, x1) = if peak - first > last - peak {
                (first.saturating_sub(1), peak)
            } else {
                (peak, (last + 1).min(255))
            };
            let (y0, y1) = (h[x0], h[x1]);
            let (dx, dy) = ((x1 - x0) as f64, y1 - y0);
            let (mut best, mut bt) = (-1.0, x0);
            for x in x0..=x1 {
                let d = (dy * x as f64 - dx * h[x] + x1 as f64 * y0 - y1 * x0 as f64).abs();
                if d > best {
                    best = d;
                    bt = x;
                }
            }
            bt
        }
        _ => {
            // Otsu: the largest between-class variance
            let (mut best, mut bt) = (-1.0, 0usize);
            for t in 0..256 {
                let w0: f64 = p[..=t].iter().sum();
                let w1 = 1.0 - w0;
                if w0 <= 0.0 || w1 <= 0.0 {
                    continue;
                }
                let m0: f64 = (0..=t).map(|k| k as f64 * p[k]).sum::<f64>() / w0;
                let m1: f64 = (t + 1..256).map(|k| k as f64 * p[k]).sum::<f64>() / w1;
                let v = w0 * w1 * (m0 - m1) * (m0 - m1);
                if v > best {
                    best = v;
                    bt = t;
                }
            }
            bt
        }
    };
    let thr = t as f64 * 257.0;
    for px in i.pixels.chunks_exact_mut(4) {
        let v = if Image::intensity(px) > thr { 65535 } else { 0 };
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }
    i.colors = None;
}
