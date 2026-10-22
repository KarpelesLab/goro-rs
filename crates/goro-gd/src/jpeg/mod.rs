//! gd_jpeg.c on top of a pure Rust JPEG codec that reproduces
//! libjpeg-turbo (islow DCT, fancy upsampling, the standard / optimized
//! Huffman tables) so that files and decoded pixels match php-src builds
//! linked against libjpeg-turbo.

mod aritab;
mod decoder;
mod encoder;
mod tables;

use crate::*;

/// JPEG_LIB_VERSION of the emulated library (libjpeg-turbo built with
/// the libjpeg v6b API, as most distributions ship it)
pub const JPEG_LIB_VERSION: i32 = 62;

const GD_JPEG_VERSION: &str = "1.0";

/// gdJpegGetVersionString()
pub fn version_string() -> &'static str {
    match JPEG_LIB_VERSION {
        62 => "6b",
        70 => "7",
        80 => "8",
        90 => "9 compatible",
        _ => "unknown",
    }
}

/// gdImageJpegCtx(): None when libjpeg reported a fatal error
pub fn image_jpeg(im: &Image, quality: i32) -> Option<Vec<u8>> {
    image_jpeg_ex(im, quality, false)
}

/// image_jpeg() with optional 4:4:4 sampling (what libgd >= 2.2 does for
/// quality >= 90; used to check the codec against such builds)
#[doc(hidden)]
pub fn image_jpeg_ex(im: &Image, quality: i32, sampling_444: bool) -> Option<Vec<u8>> {
    let w = im.sx.max(0) as usize;
    let h = im.sy.max(0) as usize;
    if w == 0 || h == 0 || w > 65500 || h > 65500 {
        gd_error_ex(
            GD_WARNING,
            "gd-jpeg: JPEG library reports unrecoverable error: Maximum supported image dimension is 65500 pixels",
        );
        return None;
    }
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            if im.true_color {
                let v = im.tpixels[y][x];
                rgb.push(true_color_get_red(v) as u8);
                rgb.push(true_color_get_green(v) as u8);
                rgb.push(true_color_get_blue(v) as u8);
            } else {
                let idx = im.pixels[y][x] as usize;
                rgb.push(im.red[idx] as u8);
                rgb.push(im.green[idx] as u8);
                rgb.push(im.blue[idx] as u8);
            }
        }
    }
    let comment = if quality >= 0 {
        format!(
            "CREATOR: gd-jpeg v{} (using IJG JPEG v{}), quality = {}\n",
            GD_JPEG_VERSION, JPEG_LIB_VERSION, quality
        )
    } else {
        format!(
            "CREATOR: gd-jpeg v{} (using IJG JPEG v{}), default quality\n",
            GD_JPEG_VERSION, JPEG_LIB_VERSION
        )
    };
    let p = encoder::EncodeParams {
        width: w as u32,
        height: h as u32,
        quality,
        density_unit: 1,
        x_density: im.res_x as u16,
        y_density: im.res_y as u16,
        progressive: im.interlace != 0,
        sampling_444,
        comment: Some(comment.into_bytes()),
    };
    Some(encoder::encode(&p, &rgb))
}

/// CMYKToRGB(): a very basic conversion approach, TBB
fn cmyk_to_rgb(c: i32, m: i32, y: i32, k: i32, inverted: bool) -> i32 {
    let (c, m, y, k) = if inverted {
        (255 - c, 255 - m, 255 - y, 255 - k)
    } else {
        (c, m, y, k)
    };
    true_color(
        (255 - c) * (255 - k) / 255,
        (255 - m) * (255 - k) / 255,
        (255 - y) * (255 - k) / 255,
    )
}

/// gdImageCreateFromJpegCtxEx()
pub fn create_from_jpeg(data: &[u8], ignore_warning: bool) -> Option<Image> {
    let mut d = decoder::Decoder::new(data, ignore_warning);
    let fatal = |m: String| {
        gd_error_ex(
            GD_WARNING,
            format!("gd-jpeg: JPEG library reports unrecoverable error: {}", m),
        );
    };
    if let Err(m) = d.read_header() {
        fatal(m);
        return None;
    }
    let width = d.image_width;
    let height = d.image_height;
    let Some(mut im) = Image::create_true_color(width as i32, height as i32) else {
        gd_error("gd-jpeg error: cannot allocate gdImage struct");
        return None;
    };
    match d.density_unit {
        1 => {
            im.res_x = d.x_density as u32;
            im.res_y = d.y_density as u32;
        }
        2 => {
            im.res_x = (d.x_density as f64 * 2.54 + 0.5) as u32;
            im.res_y = (d.y_density as f64 * 2.54 + 0.5) as u32;
        }
        _ => {}
    }
    let cmyk = d.is_cmyk();
    let rows = match d.decompress(cmyk) {
        Ok(r) => r,
        Err(m) => {
            fatal(m);
            return None;
        }
    };
    let channels = if cmyk { 4 } else { 3 };
    let inverted = cmyk && d.saw_adobe;
    for y in 0..height as usize {
        let row = &rows[y * width as usize * channels..(y + 1) * width as usize * channels];
        let tp = &mut im.tpixels[y];
        for x in 0..width as usize {
            let p = &row[x * channels..];
            tp[x] = if cmyk {
                cmyk_to_rgb(p[0] as i32, p[1] as i32, p[2] as i32, p[3] as i32, inverted)
            } else {
                true_color(p[0] as i32, p[1] as i32, p[2] as i32)
            };
        }
    }
    if let Err(m) = d.finish() {
        fatal(m);
        return None;
    }
    if !ignore_warning && d.num_warnings > 0 {
        return None;
    }
    Some(im)
}
