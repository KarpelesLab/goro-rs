//! gd_transform.c: flipping.

use crate::Image;

/// gdImageFlipVertical()
pub fn flip_vertical(im: &mut Image) {
    let sy = im.sy.max(0) as usize;
    if im.true_color {
        for y in 0..sy / 2 {
            im.tpixels.swap(y, sy - 1 - y);
        }
    } else {
        for y in 0..sy / 2 {
            im.pixels.swap(y, sy - 1 - y);
        }
    }
}

/// gdImageFlipHorizontal()
pub fn flip_horizontal(im: &mut Image) {
    let sx = im.sx.max(0) as usize;
    if im.true_color {
        for row in im.tpixels.iter_mut() {
            row[..sx].reverse();
        }
    } else {
        for row in im.pixels.iter_mut() {
            row[..sx].reverse();
        }
    }
}

/// gdImageFlipBoth()
pub fn flip_both(im: &mut Image) {
    flip_vertical(im);
    flip_horizontal(im);
}
