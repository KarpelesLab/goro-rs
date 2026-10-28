//! AVIF through the oxideav-avif codec (AV1 Image File Format), as
//! truecolor images.

use super::{DecodeError, DecodeResult};
use crate::image::{Image, tc, tc_a, tc_b, tc_g, tc_r};

/// An ISOBMFF `ftyp` box with an AVIF major or compatible brand.
pub fn is_avif(d: &[u8]) -> bool {
    oxideav_avif::probe(d)
}

pub fn decode(d: &[u8]) -> DecodeResult {
    if !is_avif(d) {
        return Err(DecodeError::with("gd-avif error: not an AVIF file"));
    }
    let img = oxideav_avif::decode_rgba8(d)
        .map_err(|e| DecodeError::with(&format!("gd-avif error: {e}")))?;
    let mut im = Image::new_truecolor(img.width as i32, img.height as i32)
        .ok_or_else(|| DecodeError::with(super::OVERFLOW_MSG))?;
    for (p, c) in im.tpixels.iter_mut().zip(img.data.chunks_exact(4)) {
        *p = tc(c[0] as i32, c[1] as i32, c[2] as i32, 127 - (c[3] as i32 >> 1));
    }
    Ok(im)
}

/// imageavif: quality 0..100 (-1: 52, the default of PHP's AVIF
/// support), speed 0..10 (-1: 6).
pub fn encode(im: &Image, quality: i32, speed: i32) -> Result<Vec<u8>, String> {
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
    let q = if quality < 0 { 52 } else { quality.min(100) } as u8;
    let mut opts = oxideav_avif::EncodeOptions::default().with_quality(q);
    if q == 100 {
        opts = opts.with_chroma(oxideav_avif::StillChroma::Yuv444);
    } else if w % 2 == 1 || h % 2 == 1 {
        opts = opts.with_chroma(oxideav_avif::StillChroma::Yuv444);
    }
    let _ = speed;
    oxideav_avif::encode_rgba8(w as u32, h as u32, &rgba, &opts).map_err(|e| e.to_string())
}
