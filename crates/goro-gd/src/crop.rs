//! gd_crop.c: cropping (explicit, automatic, threshold) and the
//! gdCropMode constants.

use crate::{EFFECT_REPLACE, Image, Rect, gd};

pub const DEFAULT: i32 = 0;
pub const TRANSPARENT: i32 = 1;
pub const BLACK: i32 = 2;
pub const WHITE: i32 = 3;
pub const SIDES: i32 = 4;
pub const THRESHOLD: i32 = 5;

/// gdImageCrop()
pub fn crop(src: &Image, rect: &Rect) -> Option<Image> {
    let mut dst = if src.true_color {
        Image::create_true_color(rect.width, rect.height)?
    } else {
        Image::create(rect.width, rect.height)?
    };
    let abf = dst.alpha_blending_flag;
    dst.set_alpha_blending(EFFECT_REPLACE);
    gd::copy(
        &mut dst,
        src,
        false,
        0,
        0,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
    );
    dst.set_alpha_blending(abf);
    Some(dst)
}

/// The scan shared by gdImageCropAuto() and gdImageCropThreshold():
/// `matches(pixel)` tells whether a pixel is background.
fn crop_scan(im: &Image, matches: impl Fn(i32) -> bool) -> Option<Image> {
    let width = im.sx;
    let height = im.sy;
    let mut r = Rect::default();

    let mut m = true;
    let mut y = 0;
    while m && y < height {
        let mut x = 0;
        while m && x < width {
            m = matches(im.get_pixel(x, y));
            x += 1;
        }
        y += 1;
    }
    if m {
        return None;
    }
    r.y = y - 1;

    m = true;
    y = height - 1;
    while m && y >= 0 {
        let mut x = 0;
        while m && x < width {
            m = matches(im.get_pixel(x, y));
            x += 1;
        }
        y -= 1;
    }
    r.height = y - r.y + 2;

    m = true;
    let mut x = 0;
    while m && x < width {
        let mut y = 0;
        while m && y < r.y + r.height {
            m = matches(im.get_pixel(x, y));
            y += 1;
        }
        x += 1;
    }
    r.x = x - 1;

    m = true;
    x = width - 1;
    while m && x >= 0 {
        let mut y = 0;
        while m && y < r.y + r.height {
            m = matches(im.get_pixel(x, y));
            y += 1;
        }
        x -= 1;
    }
    r.width = x - r.x + 2;

    crop(im, &r)
}

/// gdImageCropAuto()
pub fn crop_auto(im: &Image, mode: u32) -> Option<Image> {
    let color = match mode as i32 {
        TRANSPARENT => im.transparent,
        BLACK => im.color_closest_alpha(0, 0, 0, 0),
        WHITE => im.color_closest_alpha(255, 255, 255, 0),
        SIDES => guess_background_color_from_corners(im).0,
        _ => im.transparent,
    };
    crop_scan(im, |c| color == c)
}

/// gdImageCropThreshold()
pub fn crop_threshold(im: &Image, color: u32, threshold: f32) -> Option<Image> {
    if threshold > 100.0 {
        return None;
    }
    if !im.true_color && color >= im.colors_total as u32 {
        return None;
    }
    let color = color as i32;
    crop_scan(im, |c| color_match(im, color, c, threshold))
}

/// gdGuessBackgroundColorFromCorners(): (color, number of equal corners)
fn guess_background_color_from_corners(im: &Image) -> (i32, i32) {
    let tl = im.get_pixel(0, 0);
    let tr = im.get_pixel(im.sx - 1, 0);
    let bl = im.get_pixel(0, im.sy - 1);
    let br = im.get_pixel(im.sx - 1, im.sy - 1);
    if tr == bl && tr == br {
        (tr, 3)
    } else if (tl == bl && tl == br) || (tl == tr && tl == br) || (tl == tr && tl == bl) {
        (tl, 3)
    } else if tl == tr || tl == bl || tl == br {
        (tl, 2)
    } else if tr == bl || tr == br {
        (tr, 2)
    } else if br == bl {
        (bl, 2)
    } else {
        let avg = |f: &dyn Fn(i32) -> i32| -> i32 {
            (0.5f32 + ((f(tl) + f(tr) + f(bl) + f(br)) / 4) as f32) as i32
        };
        let r = avg(&|c| im.red_of(c));
        let g = avg(&|c| im.green_of(c));
        let b = avg(&|c| im.blue_of(c));
        let a = avg(&|c| im.alpha_of(c));
        (im.color_closest_alpha(r, g, b, a), 0)
    }
}

/// gdColorMatch()
fn color_match(im: &Image, col1: i32, col2: i32, threshold: f32) -> bool {
    let dr = im.red_of(col1) - im.red_of(col2);
    let dg = im.green_of(col1) - im.green_of(col2);
    let db = im.blue_of(col1) - im.blue_of(col2);
    let da = im.alpha_of(col1) - im.alpha_of(col2);
    let dist = dr * dr + dg * dg + db * db + da * da;
    (100.0 * dist as f64 / 195075.0) < threshold as f64
}
