//! Readers for the formats beyond the common web ones: PCX, DDS, JPEG XL
//! and Radiance HDR on their OxideAV crates; MIFF, SGI, XPM and PSD
//! here, from their published descriptions (ImageMagick's MIFF page,
//! the SGI image file format, the XPM manual, Adobe's PSD specification).

use crate::codec::corrupt;
use crate::error::MResult;
use crate::image::Image;
use crate::types::*;

fn rgba8_image(w: u32, h: u32, rgba: &[u8], alpha: bool) -> Image {
    let mut img = Image::blank(w as usize, h as usize);
    img.alpha = alpha;
    img.set_from_rgba8(rgba);
    img.depth = 8;
    img
}

fn has_alpha8(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|p| p[3] != 255)
}

pub fn decode_pcx(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_pcx::decode_rgba8(data).map_err(|e| corrupt("PCX", e))?;
    let alpha = has_alpha8(&im.data);
    Ok(vec![rgba8_image(im.width, im.height, &im.data, alpha)])
}

pub fn decode_dds(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_dds::decode_rgba8(data).map_err(|e| corrupt("DDS", e))?;
    let alpha = has_alpha8(&im.data);
    Ok(vec![rgba8_image(im.width, im.height, &im.data, alpha)])
}

pub fn decode_jxl(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_jpegxl::decode_rgba8(data).map_err(|e| corrupt("JXL", e))?;
    let alpha = has_alpha8(&im.data);
    Ok(vec![rgba8_image(im.width, im.height, &im.data, alpha)])
}

/// Radiance RGBE: linear floats, stored clamped at Q16, in the RGB
/// (linear) colourspace.
pub fn decode_hdr(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_hdr::decode(data).map_err(|e| corrupt("HDR", e))?;
    let (w, h) = (im.width as usize, im.height as usize);
    let plane = &im.planes[0];
    let mut img = Image::blank(w, h);
    for y in 0..h {
        let row = &plane.data[y * plane.stride..];
        for x in 0..w {
            let f = |c: usize| {
                let o = (x * 3 + c) * 4;
                f32::from_le_bytes([row[o], row[o + 1], row[o + 2], row[o + 3]]) as f64
            };
            let q = |v: f64| crate::color::clamp_to_quantum(v * 65535.0) as u16;
            img.set_px(x, y, [q(f(0)), q(f(1)), q(f(2)), 65535]);
        }
    }
    img.depth = 32;
    img.colorspace = Colorspace::RGB;
    Ok(vec![img])
}
