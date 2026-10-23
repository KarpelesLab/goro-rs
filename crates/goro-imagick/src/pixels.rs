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
    !map.is_empty() && map.bytes().all(|c| b"RGBAOCMYKIP".contains(&c))
}

/// The samples of an area in quantum scale, channel by `map`; outside the
/// image the virtual pixel method answers. Intensity ("I") is not rounded.
pub fn export_f64(img: &Image, x: i64, y: i64, w: usize, h: usize, map: &str) -> MResult<Vec<f64>> {
    export_samples(img, x, y, w, h, map, true)
}

fn export_samples(
    img: &Image,
    x: i64,
    y: i64,
    w: usize,
    h: usize,
    map: &str,
    separated_check: bool,
) -> MResult<Vec<f64>> {
    if map.is_empty() {
        return Err(MagickError::new(
            severity::RESOURCE_LIMIT_ERROR,
            "memory allocation failed `' @ error/pixel.c/ExportImagePixels/1989",
        ));
    }
    if !valid_map(map) {
        return Err(bad_map(map));
    }
    if separated_check && img.colorspace != Colorspace::CMYK {
        // the first colour-separation channel names the failing check
        if let Some(line) = map.bytes().find_map(|c| match c {
            b'C' => Some(2016),
            b'K' => Some(2039),
            b'M' => Some(2050),
            b'Y' => Some(2079),
            _ => None,
        }) {
            return Err(MagickError::with_desc(
                severity::IMAGE_ERROR,
                "color separated image required",
                map,
                &format!("error/pixel.c/ExportImagePixels/{line}"),
            ));
        }
    }
    let gray = img.colorspace == Colorspace::Gray;
    let map = map.as_bytes();
    let mut out = Vec::with_capacity(w * h * map.len());
    for yy in 0..h {
        for xx in 0..w {
            let (px_x, px_y) = (x + xx as i64, y + yy as i64);
            let inside = px_x >= 0
                && px_y >= 0
                && (px_x as usize) < img.columns
                && (px_y as usize) < img.rows;
            let p = img.virtual_px(px_x, px_y);
            let k = if inside {
                img.black
                    .as_ref()
                    .map_or(0, |k| k[px_y as usize * img.columns + px_x as usize])
            } else {
                0
            };
            let a = if img.alpha { p[3] } else { 65535 };
            for &c in map {
                out.push(match c {
                    b'R' | b'C' => p[0] as f64,
                    b'G' | b'M' => p[if gray { 0 } else { 1 }] as f64,
                    b'B' | b'Y' => p[if gray { 0 } else { 2 }] as f64,
                    b'A' | b'O' => a as f64,
                    b'K' => k as f64,
                    b'I' => {
                        if gray {
                            p[0] as f64
                        } else {
                            // Rec. 709 luma of the stored samples
                            0.212656 * p[0] as f64 + 0.715158 * p[1] as f64 + 0.072186 * p[2] as f64
                        }
                    }
                    _ => 0.0,
                });
            }
        }
    }
    Ok(out)
}

/// The samples of an area, in quantum scale (u16), channel by `map`.
pub fn export_q16(img: &Image, x: i64, y: i64, w: usize, h: usize, map: &str) -> MResult<Vec<u16>> {
    check_area(img, x, y, w, h)?;
    Ok(export_samples(img, x, y, w, h, map, false)?
        .into_iter()
        .map(|v| v.round().clamp(0.0, 65535.0) as u16)
        .collect())
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
    let map: Vec<u8> = map.bytes().collect();
    if samples.len() < w * h * map.len() {
        return Err(MagickError::with_desc(
            severity::OPTION_ERROR,
            "InsufficientImageData",
            "",
            "error/pixel.c/ImportImagePixels",
        ));
    }
    let has_alpha = map.iter().any(|&c| c == b'A' || c == b'O');
    let cmyk = map.iter().any(|c| b"CMYK".contains(c));
    if has_alpha && !img.alpha {
        // the alpha channel an import adds starts out transparent
        img.alpha = true;
        for p in img.pixels.chunks_exact_mut(4) {
            p[3] = 0;
        }
    }
    if cmyk {
        if img.black.is_none() {
            img.black = Some(vec![0; img.columns * img.rows]);
        }
        img.colorspace = Colorspace::CMYK;
    }
    if map.contains(&b'I') && img.colorspace != Colorspace::Gray {
        // the image turns gray: its first channel is the gray
        img.colorspace = Colorspace::Gray;
        crate::properties::colorspace_attributes(img, Colorspace::Gray);
        for p in img.pixels.chunks_exact_mut(4) {
            p[1] = p[0];
            p[2] = p[0];
        }
    }
    let gray = img.colorspace == Colorspace::Gray;
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
                    // a gray image's one channel takes any of R, G, B
                    b'R' | b'G' | b'B' | b'I' if gray => {
                        p[0] = v;
                        p[1] = v;
                        p[2] = v;
                    }
                    b'R' | b'C' => p[0] = v,
                    b'G' | b'M' => p[1] = v,
                    b'B' | b'Y' => p[2] = v,
                    b'A' | b'O' => p[3] = v,
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
