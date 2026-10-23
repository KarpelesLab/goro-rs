//! Pixel filters. Most run through oxideav-image-filter on 8-bit RGBA
//! (its classic filters have no 16-bit path yet); the simple point
//! operations are done here directly on the Q16 samples.

use crate::color::clamp_to_quantum;
use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use crate::types::channel;

/// Run an oxideav-image-filter filter on the image (as 8-bit RGBA).
pub fn apply_oxideav<F: oxideav_image_filter::ImageFilter>(img: &mut Image, f: &F) -> MResult<()> {
    use oxideav_core::{PixelFormat, VideoFrame, VideoPlane};
    use oxideav_image_filter::VideoStreamParams;
    let cmyk = img.colorspace == crate::types::Colorspace::CMYK;
    if cmyk {
        img.cmyk_to_rgb();
    }
    let rgba = img.to_rgba8();
    let frame = VideoFrame {
        pts: None,
        planes: vec![VideoPlane {
            stride: img.columns * 4,
            data: rgba,
        }],
    };
    let params = VideoStreamParams {
        format: PixelFormat::Rgba,
        width: img.columns as u32,
        height: img.rows as u32,
    };
    let out = f.apply(&frame, params).map_err(|e| {
        MagickError::with_desc(
            severity::FILTER_ERROR,
            &format!("{}", e),
            "",
            "error/effect.c/Filter",
        )
    })?;
    let fmt = f
        .output_pixel_format(PixelFormat::Rgba)
        .unwrap_or(PixelFormat::Rgba);
    let plane = &out.planes[0];
    let w = img.columns;
    let h = img.rows;
    let mut rgba = vec![0u8; w * h * 4];
    match fmt {
        PixelFormat::Gray8 => {
            for y in 0..h {
                for x in 0..w {
                    let v = plane.data[y * plane.stride + x];
                    let d = (y * w + x) * 4;
                    rgba[d..d + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
            img.colorspace = crate::types::Colorspace::SRGB;
        }
        PixelFormat::Rgb24 => {
            for y in 0..h {
                for x in 0..w {
                    let s = &plane.data[y * plane.stride + x * 3..];
                    let d = (y * w + x) * 4;
                    rgba[d..d + 4].copy_from_slice(&[s[0], s[1], s[2], 255]);
                }
            }
        }
        _ => {
            for y in 0..h {
                let s = &plane.data[y * plane.stride..y * plane.stride + w * 4];
                rgba[y * w * 4..(y + 1) * w * 4].copy_from_slice(s);
            }
        }
    }
    img.set_from_rgba8(&rgba);
    img.colors = None;
    if cmyk {
        img.rgb_to_cmyk();
    }
    Ok(())
}

/// DeskewImage()'s skew angle (degrees): a fast (recursive, Brady-style)
/// Radon transform of the pixels darker than `threshold` (quantum scale)
/// over slopes k/N, N the width rounded up to a power of two, for both
/// directions (the second on the image flipped vertically). The slope
/// whose projection has the largest sum of squared differences between
/// adjacent bins wins; the angle is −atan(k/N).
pub fn deskew_angle(img: &Image, threshold: f64) -> f64 {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return 0.0;
    }
    let mut n = 1usize;
    while n < w {
        n *= 2;
    }
    let len = h + n;
    let bit = |x: usize, y: usize| -> i64 { (Image::intensity(&img.px(x, y)) < threshold) as i64 };
    let mut best = (i64::MIN, 0i64);
    for flip in [false, true] {
        // one block per column, each holding one slope
        let mut blocks: Vec<Vec<Vec<i64>>> = (0..n)
            .map(|x| {
                let mut c = vec![0i64; len];
                if x < w {
                    for y in 0..h {
                        c[y] = bit(x, if flip { h - 1 - y } else { y });
                    }
                }
                vec![c]
            })
            .collect();
        let mut size = 1;
        while size < n {
            let mut next = Vec::with_capacity(blocks.len() / 2);
            let mut it = blocks.into_iter();
            while let (Some(l), Some(r)) = (it.next(), it.next()) {
                let mut out = Vec::with_capacity(2 * size);
                for k in 0..2 * size {
                    let (a, b) = (&l[k / 2], &r[k / 2]);
                    let d = k.div_ceil(2);
                    let mut s = a.clone();
                    for y in 0..len - d {
                        s[y] += b[y + d];
                    }
                    out.push(s);
                }
                next.push(out);
            }
            blocks = next;
            size *= 2;
        }
        // ImageMagick only searches slopes below N/8 (about 7°)
        for (k, p) in blocks[0].iter().enumerate().take(n / 8) {
            let score: i64 = p.windows(2).map(|q| (q[1] - q[0]) * (q[1] - q[0])).sum();
            let kk = if flip { -(k as i64) } else { k as i64 };
            if score > best.0 {
                best = (score, kk);
            }
        }
    }
    -(180.0 * (best.1 as f64 / n as f64).atan() / std::f64::consts::PI)
}

/// The channels a ChannelType selects among R, G, B, A.
pub fn channel_mask(ch: i64, alpha: bool) -> [bool; 4] {
    let all = ch == channel::ALL || ch == 0 || ch == 0x7fffffff;
    if all {
        return [true, true, true, alpha && false];
    }
    [
        ch & channel::RED != 0,
        ch & channel::GREEN != 0,
        ch & channel::BLUE != 0,
        ch & channel::ALPHA != 0,
    ]
}

/// Apply a function to the selected channels of every pixel (Q16).
pub fn map_channels(img: &mut Image, ch: i64, f: impl Fn(f64) -> f64) {
    let mask = channel_mask(ch, img.alpha);
    for p in img.pixels.chunks_exact_mut(4) {
        for c in 0..4 {
            if mask[c] {
                p[c] = clamp_to_quantum(f(p[c] as f64)) as u16;
            }
        }
    }
    img.colors = None;
}

/// NegateImage(); `gray`: only gray pixels.
pub fn negate(img: &mut Image, gray: bool, ch: i64) {
    let mask = channel_mask(ch, img.alpha);
    for p in img.pixels.chunks_exact_mut(4) {
        if gray && !(p[0] == p[1] && p[1] == p[2]) {
            continue;
        }
        for c in 0..4 {
            if mask[c] {
                p[c] = 65535 - p[c];
            }
        }
    }
    if let Some(k) = &mut img.black {
        if ch == channel::ALL || ch & channel::BLACK != 0 {
            for v in k.iter_mut() {
                *v = 65535 - *v;
            }
        }
    }
    img.colors = None;
}
