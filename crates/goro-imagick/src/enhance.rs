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
