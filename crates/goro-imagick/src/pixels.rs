//! ExportImagePixels() / ImportImagePixels(): pixel areas as samples in a
//! channel map ("RGBA", "I", "CMYK", "BGRP"...).

use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use crate::types::Colorspace;

fn bad_map(map: &str) -> MagickError {
    MagickError::with_desc(
        severity::OPTION_ERROR,
        "UnrecognizedPixelMap",
        map,
        "error/pixel.c/ExportImagePixels",
    )
}

fn check_area(img: &Image, x: i64, y: i64, w: usize, h: usize) -> MResult<()> {
    if x < 0
        || y < 0
        || w == 0
        || h == 0
        || x as usize + w > img.columns
        || y as usize + h > img.rows
    {
        return Err(MagickError::with_desc(
            severity::OPTION_ERROR,
            "GeometryDoesNotContainImage",
            &format!("`{}'", img.filename),
            "error/cache.c/GetVirtualPixelCache",
        ));
    }
    Ok(())
}

/// Valid map characters.
pub fn valid_map(map: &str) -> bool {
    !map.is_empty() && map.bytes().all(|c| b"RGBAOCMYKIPrgbaocmykip".contains(&c))
}

/// The samples of an area, in quantum scale (u16), channel by `map`.
pub fn export_q16(img: &Image, x: i64, y: i64, w: usize, h: usize, map: &str) -> MResult<Vec<u16>> {
    if !valid_map(map) {
        return Err(bad_map(map));
    }
    check_area(img, x, y, w, h)?;
    let map: Vec<u8> = map.bytes().map(|c| c.to_ascii_uppercase()).collect();
    let mut out = Vec::with_capacity(w * h * map.len());
    for yy in 0..h {
        for xx in 0..w {
            let px_x = x as usize + xx;
            let px_y = y as usize + yy;
            let p = img.px(px_x, px_y);
            let k = img
                .black
                .as_ref()
                .map_or(0, |k| k[px_y * img.columns + px_x]);
            let a = if img.alpha { p[3] } else { 65535 };
            for &c in &map {
                out.push(match c {
                    b'R' | b'C' => p[0],
                    b'G' | b'M' => p[1],
                    b'B' | b'Y' => p[2],
                    b'A' => a,
                    b'O' => 65535 - a,
                    b'K' => k,
                    b'I' => {
                        if img.colorspace == Colorspace::Gray {
                            p[0]
                        } else {
                            crate::color::to_q16(Image::intensity(&p))
                        }
                    }
                    _ => 0,
                });
            }
        }
    }
    Ok(out)
}

/// Set the pixels of an area from samples (quantum scale) in `map` order.
pub fn import_q16(
    img: &mut Image,
    x: i64,
    y: i64,
    w: usize,
    h: usize,
    map: &str,
    samples: &[u16],
) -> MResult<()> {
    if !valid_map(map) {
        return Err(bad_map(map));
    }
    check_area(img, x, y, w, h)?;
    let map: Vec<u8> = map.bytes().map(|c| c.to_ascii_uppercase()).collect();
    if samples.len() < w * h * map.len() {
        return Err(MagickError::with_desc(
            severity::OPTION_ERROR,
            "InsufficientImageData",
            "",
            "error/pixel.c/ImportImagePixels",
        ));
    }
    let has_alpha = map.iter().any(|&c| c == b'A' || c == b'O');
    let gray = map
        .iter()
        .all(|&c| c == b'I' || c == b'A' || c == b'O' || c == b'P');
    let cmyk = map.contains(&b'K');
    if has_alpha {
        img.enable_alpha();
    }
    if cmyk && img.black.is_none() {
        img.black = Some(vec![0; img.columns * img.rows]);
        img.colorspace = Colorspace::CMYK;
    }
    if gray && map.contains(&b'I') {
        img.colorspace = Colorspace::Gray;
    }
    let mut i = 0;
    for yy in 0..h {
        for xx in 0..w {
            let px_x = x as usize + xx;
            let px_y = y as usize + yy;
            let mut p = img.px(px_x, px_y);
            for &c in &map {
                let v = samples[i];
                i += 1;
                match c {
                    b'R' | b'C' => p[0] = v,
                    b'G' | b'M' => p[1] = v,
                    b'B' | b'Y' => p[2] = v,
                    b'A' => p[3] = v,
                    b'O' => p[3] = 65535 - v,
                    b'K' => {
                        if let Some(k) = &mut img.black {
                            k[px_y * img.columns + px_x] = v;
                        }
                    }
                    b'I' => {
                        p[0] = v;
                        p[1] = v;
                        p[2] = v;
                    }
                    _ => {}
                }
            }
            img.set_px(px_x, px_y, p);
        }
    }
    Ok(())
}
