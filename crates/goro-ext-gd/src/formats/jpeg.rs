//! JPEG through the oxideav-mjpeg codec (ITU-T T.81 / T.871 JFIF), as
//! truecolor GD images. The JFIF density gives the resolution.

use super::{DecodeError, DecodeResult};
use crate::image::{Image, tc, tc_b, tc_g, tc_r};

/// Whether the marker segments before the end of the data include a
/// frame and a scan (libjpeg's "JPEG datastream contains no image").
fn has_scan(d: &[u8]) -> bool {
    let mut i = 2;
    let mut sof = false;
    while i + 1 < d.len() {
        if d[i] != 0xff {
            i += 1;
            continue;
        }
        let m = d[i + 1];
        match m {
            0xff | 0x00 | 0x01 | 0xd0..=0xd7 => {
                i += 1;
                continue;
            }
            0xd9 => return false,
            0xda => return sof,
            0xc0..=0xcf if m != 0xc4 && m != 0xc8 && m != 0xcc => sof = true,
            _ => {}
        }
        if i + 3 >= d.len() {
            return false;
        }
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        i += 2 + len;
    }
    false
}

fn density(d: &[u8]) -> Option<(u32, u32)> {
    // JFIF APP0: units 1 = dots per inch, 2 = dots per cm.
    let mut i = 2;
    while i + 4 < d.len() && d[i] == 0xff {
        let m = d[i + 1];
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        if m == 0xe0 && len >= 14 && d.len() >= i + 4 + 12 && &d[i + 4..i + 9] == b"JFIF\0" {
            let units = d[i + 11];
            let x = u16::from_be_bytes([d[i + 12], d[i + 13]]) as u32;
            let y = u16::from_be_bytes([d[i + 14], d[i + 15]]) as u32;
            return match units {
                1 => Some((x, y)),
                2 => Some((
                    (x as f64 * 2.54 + 0.5) as u32,
                    (y as f64 * 2.54 + 0.5) as u32,
                )),
                _ => None,
            };
        }
        if m == 0xda {
            break;
        }
        i += 2 + len;
    }
    None
}

pub fn decode(d: &[u8]) -> DecodeResult {
    let fail = |msg: &str| {
        DecodeError::with(&format!(
            "gd-jpeg: JPEG library reports unrecoverable error: {msg}"
        ))
    };
    if d.len() < 2 || d[0] != 0xff || d[1] != 0xd8 {
        return Err(fail("Not a JPEG file: starts with 0x%02x 0x%02x"));
    }
    if !has_scan(d) {
        return Err(fail("JPEG datastream contains no image"));
    }
    let opts = oxideav_mjpeg::DecodeOptions::libjpeg_compatible();
    let img = oxideav_mjpeg::decode_rgb8_with(d, &opts).map_err(|e| fail(&e.to_string()))?;
    let mut im = Image::new_truecolor(img.width as i32, img.height as i32)
        .ok_or_else(|| DecodeError::with(super::OVERFLOW_MSG))?;
    for (p, c) in im.tpixels.iter_mut().zip(img.data.as_chunks::<3>().0) {
        *p = tc(c[0] as i32, c[1] as i32, c[2] as i32, 0);
    }
    if let Some((x, y)) = density(d) {
        im.res_x = x;
        im.res_y = y;
    }
    Ok(im)
}

/// imagejpeg: `quality` -1 is libjpeg's default (75); `progressive` for
/// interlaced images.
pub fn encode(im: &Image, quality: i32) -> Result<Vec<u8>, String> {
    let (w, h) = (im.sx as usize, im.sy as usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let p = im.get_truecolor_pixel(x, y);
            rgb.extend_from_slice(&[tc_r(p) as u8, tc_g(p) as u8, tc_b(p) as u8]);
        }
    }
    let q = if quality < 0 {
        75
    } else {
        quality.clamp(1, 100)
    } as u8;
    let mut opts = oxideav_mjpeg::EncodeOptions::default().with_quality(q);
    if im.interlace {
        opts = opts.with_progressive(true);
    }
    let mut out =
        oxideav_mjpeg::encode_rgb8(w as u32, h as u32, &rgb, &opts).map_err(|e| e.to_string())?;
    set_density(&mut out, im.res_x, im.res_y);
    Ok(out)
}

/// Writes the resolution into the JFIF APP0 segment (inserting one when
/// the encoder wrote none).
fn set_density(out: &mut Vec<u8>, x: u32, y: u32) {
    let (x, y) = (x.min(65535) as u16, y.min(65535) as u16);
    if out.len() > 20 && out[2] == 0xff && out[3] == 0xe0 && &out[6..11] == b"JFIF\0" {
        out[13] = 1;
        out[14..16].copy_from_slice(&x.to_be_bytes());
        out[16..18].copy_from_slice(&y.to_be_bytes());
        return;
    }
    let mut app0 = vec![0xff, 0xe0, 0, 16];
    app0.extend_from_slice(b"JFIF\0");
    app0.extend_from_slice(&[1, 1, 1]);
    app0.extend_from_slice(&x.to_be_bytes());
    app0.extend_from_slice(&y.to_be_bytes());
    app0.extend_from_slice(&[0, 0]);
    out.splice(2..2, app0);
}
