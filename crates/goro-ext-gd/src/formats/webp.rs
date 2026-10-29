//! WebP through the oxideav-webp codec (RFC 9649), as truecolor images.
//! imagewebp: quality 0..100 is lossy, IMG_WEBP_LOSSLESS (101) lossless,
//! -1 the default lossy quality 80 (PHP manual, imagewebp).

use super::{DecodeError, DecodeResult};
use crate::image::{Image, tc, tc_a, tc_b, tc_g, tc_r};

pub const LOSSLESS: i32 = 101;

pub fn decode(d: &[u8]) -> DecodeResult {
    if !oxideav_webp::probe(d) {
        return Err(DecodeError::with("gd-webp cannot get webp info"));
    }
    let img = oxideav_webp::decode_rgba8(d)
        .map_err(|_| DecodeError::with("gd-webp cannot get webp info"))?;
    let mut im = Image::new_truecolor(img.width as i32, img.height as i32)
        .ok_or_else(|| DecodeError::with(super::OVERFLOW_MSG))?;
    for (p, c) in im.tpixels.iter_mut().zip(img.data.as_chunks::<4>().0) {
        *p = tc(
            c[0] as i32,
            c[1] as i32,
            c[2] as i32,
            127 - (c[3] as i32 >> 1),
        );
    }
    Ok(im)
}

pub fn encode(im: &Image, quality: i32) -> Result<Vec<u8>, String> {
    let (w, h) = (im.sx as usize, im.sy as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let p = im.get_truecolor_pixel(x, y);
            rgba.extend_from_slice(&[
                tc_r(p) as u8,
                tc_g(p) as u8,
                tc_b(p) as u8,
                super::png::alpha8(tc_a(p)),
            ]);
        }
    }
    let opts = if quality == LOSSLESS {
        oxideav_webp::EncodeOptions::default()
    } else {
        let q = if quality < 0 { 80 } else { quality.min(100) };
        oxideav_webp::EncodeOptions::default().with_quality(q as f32)
    };
    oxideav_webp::encode_rgba8(w as u32, h as u32, &rgba, &opts).map_err(|e| e.to_string())
}
