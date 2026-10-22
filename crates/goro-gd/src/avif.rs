//! gd_avif.c on top of the pure-Rust OxideAV AVIF codec (oxideav-avif
//! with its oxideav-av1 pixel codec).
//!
//! The AV1 decode is bit-exact with dav1d; the YUV -> RGB conversion is
//! oxideav-avif's, not libavif's (libyuv), so decoded pixels can differ
//! from php-src's by a few levels. The encoder knobs follow libgd: the
//! 0..100 quality maps to libavif's 0..63 quantizer (then to the AV1
//! qindex the way libaom does), quality 100 is lossless (identity matrix,
//! 4:4:4), qualities from 90 use 4:4:4, others 4:2:0. The speed has no
//! counterpart in the OxideAV encoder and is ignored.

use crate::*;

const QUANTIZER_DEFAULT: i32 = 30;
const QUALITY_DEFAULT: i32 = -1;
const MAX_QUALITY: i32 = 100;
const HIGH_QUALITY_SUBSAMPLING_THRESHOLD: i32 = 90;
/// AVIF_QUANTIZER_WORST_QUALITY
const QUANTIZER_WORST: i32 = 63;

/// quality2Quantizer()
fn quality_to_quantizer(quality: i32) -> i32 {
    let q = quality.clamp(0, MAX_QUALITY);
    let scale = QUANTIZER_WORST as f32 / MAX_QUALITY as f32;
    (scale * (MAX_QUALITY - q) as f32).round() as i32
}

/// libaom's quantizer_to_qindex[] (av1/encoder): the AV1 base_q_idx a
/// libavif min/max quantizer selects.
fn quantizer_to_qindex(q: i32) -> u8 {
    if q >= 63 { 255 } else { (q.max(0) * 4) as u8 }
}

/// isAvifSrgbImage()
fn is_srgb(c: &oxideav_avif::ColorInfo) -> bool {
    (c.primaries == 1 || c.primaries == 2) && (c.transfer == 13 || c.transfer == 2)
}

/// gdImageCreateFromAvifCtx()
pub fn create_from_avif(data: &[u8]) -> Option<Image> {
    if oxideav_avif::info(data).is_err() {
        gd_error("avif error - Could not parse image: BMFF parsing failed\n");
        return None;
    }
    let img = match oxideav_avif::decode(data) {
        Ok(i) => i,
        Err(_) => {
            gd_error("avif error - Could not decode image: Decoding of color planes failed\n");
            return None;
        }
    };
    if !is_srgb(&img.color) {
        gd_error_ex(GD_NOTICE, "Image's color profile is not sRGB");
    }
    let rgba = match img.try_to_rgba8() {
        Ok(p) => p,
        Err(_) => img.to_rgba8(),
    };
    let Some(mut im) = Image::create_true_color(img.width() as i32, img.height() as i32) else {
        gd_error("avif error - Could not create GD truecolor image");
        return None;
    };
    im.save_alpha_flag = 1;
    let w = img.width() as usize;
    for (y, row) in im.tpixels.iter_mut().enumerate() {
        for (x, px) in row.iter_mut().enumerate() {
            let p = &rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
            let a = ALPHA_MAX - (p[3] as i32 >> 1);
            *px = true_color_alpha(p[0] as i32, p[1] as i32, p[2] as i32, a);
        }
    }
    Some(im)
}

/// gdImageAvifCtx(): None when nothing is written
pub fn image_avif(im: &Image, quality: i32, speed: i32) -> Option<Vec<u8>> {
    let lossless = quality == 100;
    if !im.true_color {
        gd_error("avif error - avif doesn't support palette images");
        return None;
    }
    if im.sx == 0 || im.sy == 0 {
        gd_error("avif error - image dimensions must not be zero");
        return None;
    }
    if overflow2(im.sx, im.sy) {
        gd_error("avif error - image dimensions are too large");
        return None;
    }
    let _speed = speed.clamp(0, 10);
    let (w, h) = (im.sx as u32, im.sy as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in &im.tpixels {
        for &c in row {
            let a = true_color_get_alpha(c);
            rgba.push(true_color_get_red(c) as u8);
            rgba.push(true_color_get_green(c) as u8);
            rgba.push(true_color_get_blue(c) as u8);
            rgba.push(if a == 127 {
                0
            } else {
                (255 - ((a << 1) + (a >> 6))) as u8
            });
        }
    }
    // libavif leaves out an opaque alpha plane
    let opaque = rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let quantizer = if quality == QUALITY_DEFAULT {
        QUANTIZER_DEFAULT
    } else {
        quality_to_quantizer(quality)
    };
    let qindex = quantizer_to_qindex(quantizer);
    let opts = oxideav_avif::EncodeOptions::default()
        .with_base_q_idx(qindex)
        .with_alpha_q_idx(qindex);
    let r = if lossless {
        let pic = if opaque {
            let rgb: Vec<u8> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            oxideav_avif::AvifImage::from_rgb8(w, h, rgb)
        } else {
            oxideav_avif::AvifImage::from_rgba8(w, h, rgba)
        };
        oxideav_avif::encode(&pic, &opts)
    } else {
        let chroma = if quality >= HIGH_QUALITY_SUBSAMPLING_THRESHOLD {
            oxideav_avif::StillChroma::Yuv444
        } else {
            oxideav_avif::StillChroma::Yuv420
        };
        let opts = opts.with_chroma(chroma);
        if opaque {
            let rgb: Vec<u8> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            oxideav_avif::encode_rgb8(w, h, &rgb, &opts)
        } else {
            oxideav_avif::encode_rgba8(w, h, &rgba, &opts)
        }
    };
    match r {
        Ok(out) => Some(out),
        Err(_) => {
            gd_error("avif error - Could not encode image: Encoding of color planes failed\n");
            None
        }
    }
}
