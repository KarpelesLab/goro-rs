//! SegmentImage(): histogram thresholding plus nearest-centre (fuzzy
//! c-means) classification, after Lim & Lee.
//!
//! Established black-box against ImageMagick 7.1.2 (`-verbose -segment`
//! reports the per-channel extents, the cluster counts and centres):
//!
//! - The image is converted to the requested colourspace, segmented on
//!   its 8-bit samples, and converted back.
//! - Each channel's histogram is split into intervals independently; the
//!   clusters are the cross product of the intervals (red-major order).
//! - A pixel belongs to the first cluster whose extents, widened by 3 on
//!   each side, contain it; the centre is the mean of its members.
//! - Pixels in no cluster go to the nearest centre (rounded to 8 bits),
//!   squared Euclidean distance — the argmax of the fuzzy membership.
//! - Every pixel becomes its cluster's rounded centre, scaled to Q16.
//!
//! Given ImageMagick's own intervals the rest reproduces its output
//! exactly. The interval detection itself (a multi-scale analysis of the
//! histogram's second derivative) is approximated here at a single scale:
//! the runs where the gaussian-smoothed (σ 4) histogram's second
//! difference is below −smoothing threshold, at least 3 bins long,
//! extended 3 bins to the left.
//!
//! The cluster threshold is approximated as a minimum count of half its
//! value (ImageMagick keeps every non-empty cluster at threshold 1).
//! With no cluster at all ImageMagick paints the image black.

use crate::image::Image;
use crate::types::Colorspace;

const TAU: f64 = 4.0;

fn intervals(hist: &[f64; 256], smooth_threshold: f64) -> Vec<(usize, usize)> {
    let norm = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * TAU);
    let mut s = [0f64; 256];
    for (i, v) in s.iter_mut().enumerate() {
        let mut acc = 0.0;
        for (j, h) in hist.iter().enumerate() {
            if *h != 0.0 {
                let d = i as f64 - j as f64;
                acc += h * (-(d * d) / (2.0 * TAU * TAU)).exp();
            }
        }
        *v = acc * norm;
    }
    let mut d2 = [0f64; 256];
    for i in 1..255 {
        d2[i] = s[i + 1] - 2.0 * s[i] + s[i - 1];
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < 256 {
        if d2[i] < -smooth_threshold {
            let mut j = i;
            while j + 1 < 256 && d2[j + 1] < -smooth_threshold {
                j += 1;
            }
            if j - i + 1 >= 3 {
                out.push((i.saturating_sub(3), j));
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// SegmentImage(colorspace, cluster_threshold, smoothing_threshold).
pub fn segment(img: &mut Image, cs: Colorspace, cluster_threshold: f64, smooth_threshold: f64) {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return;
    }
    let orig = img.colorspace;
    let convert = cs != orig && cs != Colorspace::Undefined;
    if convert {
        crate::colorspace::transform(img, cs);
    }
    let n = w * h;
    let px: Vec<[u8; 3]> = img
        .pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2].map(|c| crate::image::q_to_u8(p[c])))
        .collect();
    let ivs: Vec<Vec<(usize, usize)>> = (0..3)
        .map(|c| {
            let mut hist = [0f64; 256];
            for p in &px {
                hist[p[c] as usize] += 1.0;
            }
            intervals(&hist, smooth_threshold)
        })
        .collect();
    let mut boxes: Vec<[(usize, usize); 3]> = Vec::new();
    for r in &ivs[0] {
        for g in &ivs[1] {
            for b in &ivs[2] {
                boxes.push([*r, *g, *b]);
            }
        }
    }
    let widen =
        |b: &[(usize, usize); 3]| b.map(|(lo, hi)| (lo.saturating_sub(3), (hi + 3).min(255)));
    let first_match = |boxes: &[[(usize, usize); 3]], p: &[u8; 3]| {
        boxes.iter().position(|b| {
            let wb = widen(b);
            (0..3).all(|c| (p[c] as usize) >= wb[c].0 && (p[c] as usize) <= wb[c].1)
        })
    };
    // cluster selection on the first-match counts
    let mut count = vec![0usize; boxes.len()];
    for p in &px {
        if let Some(k) = first_match(&boxes, p) {
            count[k] += 1;
        }
    }
    let min = (cluster_threshold / 2.0).max(0.5);
    let kept: Vec<[(usize, usize); 3]> = boxes
        .iter()
        .zip(&count)
        .filter(|(_, c)| **c as f64 >= min)
        .map(|(b, _)| *b)
        .collect();
    if kept.is_empty() {
        // with no cluster every pixel takes the (zero) centre: black
        for p in img.pixels.chunks_exact_mut(4) {
            p[0] = 0;
            p[1] = 0;
            p[2] = 0;
        }
        img.colors = None;
        if convert {
            crate::colorspace::transform(img, orig);
        }
        return;
    }
    let mut member: Vec<Option<usize>> = Vec::with_capacity(n);
    let mut sums = vec![[0f64; 3]; kept.len()];
    let mut cnt = vec![0usize; kept.len()];
    for p in &px {
        let k = first_match(&kept, p);
        if let Some(k) = k {
            for c in 0..3 {
                sums[k][c] += p[c] as f64;
            }
            cnt[k] += 1;
        }
        member.push(k);
    }
    let centres: Vec<[f64; 3]> = sums
        .iter()
        .zip(&cnt)
        .map(|(s, &c)| {
            if c == 0 {
                [0.0; 3]
            } else {
                s.map(|v| (v / c as f64 + 0.5).floor())
            }
        })
        .collect();
    for (idx, (p, m)) in px.iter().zip(&member).enumerate() {
        let k = m.unwrap_or_else(|| {
            let mut best = (f64::INFINITY, 0);
            for (j, c) in centres.iter().enumerate() {
                let d: f64 = (0..3).map(|ch| (p[ch] as f64 - c[ch]).powi(2)).sum();
                if d < best.0 {
                    best = (d, j);
                }
            }
            best.1
        });
        for c in 0..3 {
            img.pixels[idx * 4 + c] = (centres[k][c] * 257.0) as u16;
        }
    }
    img.colors = None;
    if convert {
        crate::colorspace::transform(img, orig);
    }
}
