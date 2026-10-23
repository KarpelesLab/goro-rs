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

/// The skew angle (degrees) of an image's text lines / edges, as the
/// oxideav Deskew filter estimates it.
pub fn deskew_angle(img: &Image, threshold: u8) -> f64 {
    use oxideav_core::{PixelFormat, VideoFrame, VideoPlane};
    let frame = VideoFrame {
        pts: None,
        planes: vec![VideoPlane {
            stride: img.columns * 4,
            data: img.to_rgba8(),
        }],
    };
    let params = oxideav_image_filter::VideoStreamParams {
        format: PixelFormat::Rgba,
        width: img.columns as u32,
        height: img.rows as u32,
    };
    oxideav_image_filter::Deskew::new(threshold)
        .estimate_angle(&frame, params)
        .unwrap_or(0.0) as f64
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
