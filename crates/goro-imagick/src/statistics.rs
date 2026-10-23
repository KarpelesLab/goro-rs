//! Channel statistics (GetImageStatistics(), GetImageMean(),
//! GetImageRange(), GetImageKurtosis()), as ImageMagick 7 reports them:
//! per pixel channel (red / gray / cyan = 0, green / magenta = 1,
//! blue / yellow = 2, black = 3, alpha = 4), with the sample standard
//! deviation, and a composite that averages the selected channels.

use crate::image::Image;
use crate::types::{Colorspace, channel};

/// The pixel channels a statistics array reports (PixelChannel 0..=8).
pub const STAT_CHANNELS: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelStats {
    pub mean: f64,
    pub standard_deviation: f64,
    pub minima: f64,
    pub maxima: f64,
    pub kurtosis: f64,
    pub skewness: f64,
    /// The smallest depth that holds every sample exactly.
    pub depth: u32,
}

impl ChannelStats {
    /// The statistics of a channel the image does not have.
    pub fn absent() -> ChannelStats {
        ChannelStats {
            mean: 0.0,
            standard_deviation: 0.0,
            minima: f64::MAX,
            maxima: -f64::MAX,
            kurtosis: 0.0,
            skewness: 0.0,
            depth: 1,
        }
    }
}

/// The smallest depth (1..=16) whose levels include a Q16 sample
/// exactly.
fn sample_depth(v: u16) -> u32 {
    for d in 1..16u32 {
        if (v as u64 * ((1u64 << d) - 1)).is_multiple_of(65535) {
            return d;
        }
    }
    16
}

fn stats_of(values: impl Iterator<Item = u16> + Clone) -> ChannelStats {
    let mut n = 0usize;
    let mut sum = 0.0;
    let mut min = f64::MAX;
    let mut max = -f64::MAX;
    let mut seen = vec![0u64; 65536 / 64];
    for v in values.clone() {
        let x = v as f64;
        n += 1;
        sum += x;
        min = min.min(x);
        max = max.max(x);
        seen[(v as usize) >> 6] |= 1u64 << (v & 63);
    }
    if n == 0 {
        return ChannelStats::absent();
    }
    // the depth of each distinct value
    let mut depth = 1u32;
    for (w, &bits) in seen.iter().enumerate() {
        let mut b = bits;
        while b != 0 && depth < 16 {
            let v = (w * 64 + b.trailing_zeros() as usize) as u16;
            depth = depth.max(sample_depth(v));
            b &= b - 1;
        }
    }
    let nf = n as f64;
    let mean = sum / nf;
    let (mut m2, mut m3, mut m4) = (0.0, 0.0, 0.0);
    for v in values {
        let d = v as f64 - mean;
        let d2 = d * d;
        m2 += d2;
        m3 += d2 * d;
        m4 += d2 * d2;
    }
    let sd = if n > 1 { (m2 / (nf - 1.0)).sqrt() } else { 0.0 };
    let (kurtosis, skewness) = if sd > 0.0 {
        (
            m4 / nf / (sd * sd * sd * sd) - 3.0,
            m3 / nf / (sd * sd * sd),
        )
    } else {
        (0.0, 0.0)
    };
    ChannelStats {
        mean,
        standard_deviation: sd,
        minima: min,
        maxima: max,
        kurtosis,
        skewness,
        depth,
    }
}

/// Which pixel channels (0..=4) an image has.
pub fn present_channels(img: &Image) -> [bool; 5] {
    let gray = matches!(img.colorspace, Colorspace::Gray | Colorspace::LinearGray);
    let cmyk = img.colorspace == Colorspace::CMYK && img.black.is_some();
    [true, !gray, !gray, cmyk, img.alpha]
}

/// The statistics of each pixel channel (0..STAT_CHANNELS); `None` for
/// the channels the image does not have.
pub fn channel_statistics(img: &Image) -> Vec<Option<ChannelStats>> {
    let present = present_channels(img);
    let mut out = vec![None; STAT_CHANNELS];
    for (c, slot) in out.iter_mut().enumerate().take(5) {
        if !present[c] {
            continue;
        }
        *slot = Some(match c {
            3 => stats_of(img.black.as_deref().unwrap_or(&[]).iter().copied()),
            4 => stats_of(img.pixels.chunks_exact(4).map(|p| p[3])),
            _ => stats_of(img.pixels.chunks_exact(4).map(move |p| p[c])),
        });
    }
    out
}

/// The pixel channels (0..=4) a ChannelType selects.
pub fn selected_channels(ch: i64) -> [bool; 5] {
    if ch == channel::ALL || ch == 0 || ch == 0x7fffffff || ch == -1 {
        return [true; 5];
    }
    [
        ch & channel::RED != 0,
        ch & channel::GREEN != 0,
        ch & channel::BLUE != 0,
        ch & channel::BLACK != 0,
        ch & channel::ALPHA != 0,
    ]
}

/// The composite statistics of the selected channels: the mean of the
/// per-channel means, deviations, kurtoses and skewnesses, the least
/// minimum and the greatest maximum.
pub fn composite_statistics(img: &Image, ch: i64) -> ChannelStats {
    let all = channel_statistics(img);
    let sel = selected_channels(ch);
    let picked: Vec<ChannelStats> = (0..5).filter(|&c| sel[c]).filter_map(|c| all[c]).collect();
    if picked.is_empty() {
        return ChannelStats::absent();
    }
    let n = picked.len() as f64;
    let avg = |f: fn(&ChannelStats) -> f64| picked.iter().map(f).sum::<f64>() / n;
    ChannelStats {
        mean: avg(|s| s.mean),
        standard_deviation: avg(|s| s.standard_deviation),
        minima: picked.iter().map(|s| s.minima).fold(f64::MAX, f64::min),
        maxima: picked.iter().map(|s| s.maxima).fold(-f64::MAX, f64::max),
        kurtosis: avg(|s| s.kurtosis),
        skewness: avg(|s| s.skewness),
        depth: picked.iter().map(|s| s.depth).max().unwrap_or(1),
    }
}
