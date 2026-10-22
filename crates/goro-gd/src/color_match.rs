//! gd_color_match.c: bring the palette colors of im2 closer to im1.

use crate::*;

/// gdImageColorMatch(): 0 on success; -1 if im1 is not truecolor, -2 if
/// im2 is not a palette image, -3 if the sizes differ, -4 if im2 has no
/// colors.
pub fn color_match(im1: &Image, im2: &mut Image) -> i32 {
    if !im1.true_color {
        return -1;
    }
    if im2.true_color {
        return -2;
    }
    if im1.sx != im2.sx || im1.sy != im2.sy {
        return -3;
    }
    if im2.colors_total < 1 {
        return -4;
    }
    let mut buf = vec![0u64; 5 * MAX_COLORS];
    for x in 0..im1.sx as usize {
        for y in 0..im1.sy as usize {
            let color = im2.pixels[y][x] as usize;
            let rgb = im1.tpixels[y][x];
            let bp = &mut buf[color * 5..color * 5 + 5];
            bp[0] += 1;
            bp[1] += true_color_get_red(rgb) as u64;
            bp[2] += true_color_get_green(rgb) as u64;
            bp[3] += true_color_get_blue(rgb) as u64;
            bp[4] += true_color_get_alpha(rgb) as u64;
        }
    }
    for color in 0..im2.colors_total as usize {
        let bp = &buf[color * 5..color * 5 + 5];
        let count = bp[0] as i32;
        if count > 0 {
            let c = count as u64;
            im2.red[color] = (bp[1] / c) as i32;
            im2.green[color] = (bp[2] / c) as i32;
            im2.blue[color] = (bp[3] / c) as i32;
            im2.alpha[color] = (bp[4] / c) as i32;
        }
    }
    0
}
