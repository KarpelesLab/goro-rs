//! gd_rotate.c: skews and the right-angle rotations.

use crate::*;

fn src_pixel(src: &Image, x: i32, y: i32) -> i32 {
    if src.true_color {
        src.get_true_color_pixel(x, y)
    } else {
        src.get_pixel(x, y)
    }
}

/// gdImageSkewX()
pub fn skew_x(
    dst: &mut Image,
    src: &mut Image,
    u_row: i32,
    i_offset: i32,
    d_weight: f64,
    mut clr_back: i32,
    ignoretransparent: bool,
) {
    let mut pxl_old_left = clr_back;
    let mut pxl_left = 0;
    if !src.true_color {
        let r = src.red_of(clr_back);
        let g = src.green_of(clr_back);
        let b = src.blue_of(clr_back);
        let a = src.alpha_of(clr_back);
        clr_back = true_color_alpha(r, g, b, a);
    }
    let mut i = 0;
    while i < i_offset {
        dst.set_pixel(i, u_row, clr_back);
        i += 1;
    }
    if i < dst.sx {
        dst.set_pixel(i, u_row, clr_back);
    }
    i = 0;
    while i < src.sx {
        let mut pxl_src = src_pixel(src, i, u_row);
        let r = (src.red_of(pxl_src) as f64 * d_weight) as i32;
        let g = (src.green_of(pxl_src) as f64 * d_weight) as i32;
        let b = (src.blue_of(pxl_src) as f64 * d_weight) as i32;
        let a = (src.alpha_of(pxl_src) as f64 * d_weight) as i32;
        pxl_left = src.color_allocate_alpha(r, g, b, a);
        if pxl_left == -1 {
            pxl_left = src.color_closest_alpha(r, g, b, a);
        }
        let mut r = src.red_of(pxl_src) - (src.red_of(pxl_left) - src.red_of(pxl_old_left));
        let mut g = src.green_of(pxl_src) - (src.green_of(pxl_left) - src.green_of(pxl_old_left));
        let mut b = src.blue_of(pxl_src) - (src.blue_of(pxl_left) - src.blue_of(pxl_old_left));
        let mut a = src.alpha_of(pxl_src) - (src.alpha_of(pxl_left) - src.alpha_of(pxl_old_left));
        r = r.min(255);
        g = g.min(255);
        b = b.min(255);
        a = a.min(127);
        if ignoretransparent && pxl_src == dst.transparent {
            pxl_src = dst.transparent;
        } else {
            pxl_src = dst.color_allocate_alpha(r, g, b, a);
            if pxl_src == -1 {
                pxl_src = dst.color_closest_alpha(r, g, b, a);
            }
        }
        if i + i_offset >= 0 && i + i_offset < dst.sx {
            dst.set_pixel(i + i_offset, u_row, pxl_src);
        }
        pxl_old_left = pxl_left;
        i += 1;
    }
    i += i_offset;
    if i < dst.sx {
        dst.set_pixel(i, u_row, pxl_left);
    }
    dst.set_pixel(i_offset, u_row, clr_back);
    i -= 1;
    loop {
        i += 1;
        if i >= dst.sx {
            break;
        }
        dst.set_pixel(i, u_row, clr_back);
    }
}

/// gdImageSkewY()
pub fn skew_y(
    dst: &mut Image,
    src: &mut Image,
    u_col: i32,
    i_offset: i32,
    d_weight: f64,
    clr_back: i32,
    ignoretransparent: bool,
) {
    let mut i_y_pos = 0;
    let mut pxl_left = 0;
    let mut i = 0;
    while i <= i_offset {
        dst.set_pixel(u_col, i, clr_back);
        i += 1;
    }
    let r = (src.red_of(clr_back) as f64 * d_weight) as i32;
    let g = (src.green_of(clr_back) as f64 * d_weight) as i32;
    let b = (src.blue_of(clr_back) as f64 * d_weight) as i32;
    let a = (src.alpha_of(clr_back) as f64 * d_weight) as i32;
    let mut pxl_old_left = dst.color_allocate_alpha(r, g, b, a);
    i = 0;
    while i < src.sy {
        let mut pxl_src = src_pixel(src, u_col, i);
        i_y_pos = i + i_offset;
        let r = (src.red_of(pxl_src) as f64 * d_weight) as i32;
        let g = (src.green_of(pxl_src) as f64 * d_weight) as i32;
        let b = (src.blue_of(pxl_src) as f64 * d_weight) as i32;
        let a = (src.alpha_of(pxl_src) as f64 * d_weight) as i32;
        pxl_left = src.color_allocate_alpha(r, g, b, a);
        if pxl_left == -1 {
            pxl_left = src.color_closest_alpha(r, g, b, a);
        }
        let mut r = src.red_of(pxl_src) - (src.red_of(pxl_left) - src.red_of(pxl_old_left));
        let mut g = src.green_of(pxl_src) - (src.green_of(pxl_left) - src.green_of(pxl_old_left));
        let mut b = src.blue_of(pxl_src) - (src.blue_of(pxl_left) - src.blue_of(pxl_old_left));
        let mut a = src.alpha_of(pxl_src) - (src.alpha_of(pxl_left) - src.alpha_of(pxl_old_left));
        r = r.min(255);
        g = g.min(255);
        b = b.min(255);
        a = a.min(127);
        if ignoretransparent && pxl_src == dst.transparent {
            pxl_src = dst.transparent;
        } else {
            pxl_src = dst.color_allocate_alpha(r, g, b, a);
            if pxl_src == -1 {
                pxl_src = dst.color_closest_alpha(r, g, b, a);
            }
        }
        if i_y_pos >= 0 && i_y_pos < dst.sy {
            dst.set_pixel(u_col, i_y_pos, pxl_src);
        }
        pxl_old_left = pxl_left;
        i += 1;
    }
    i = i_y_pos;
    if i < dst.sy {
        dst.set_pixel(u_col, i, pxl_left);
    }
    i -= 1;
    loop {
        i += 1;
        if i >= dst.sy {
            break;
        }
        dst.set_pixel(u_col, i, clr_back);
    }
}

/// The body shared by gdImageRotate90/180/270(): `map(x, y, dst_sx,
/// dst_sy)` gives the destination of source pixel (x, y).
fn rotate_by(
    src: &Image,
    ignoretransparent: bool,
    swap: bool,
    map: impl Fn(i32, i32, i32, i32) -> (i32, i32),
) -> Option<Image> {
    let (w, h) = if swap {
        (src.sy, src.sx)
    } else {
        (src.sx, src.sy)
    };
    let mut dst = Image::create_true_color(w, h)?;
    let old_blendmode = dst.alpha_blending_flag;
    dst.alpha_blending_flag = 0;
    dst.save_alpha_flag = 1;
    dst.transparent = src.transparent;
    Image::palette_copy(&mut dst, src);
    for uy in 0..src.sy {
        for ux in 0..src.sx {
            let mut c = src_pixel(src, ux, uy);
            if !src.true_color {
                c = true_color_alpha(
                    src.red_of(c),
                    src.green_of(c),
                    src.blue_of(c),
                    src.alpha_of(c),
                );
            }
            let (dx, dy) = map(ux, uy, dst.sx, dst.sy);
            if ignoretransparent && c == dst.transparent {
                let t = dst.transparent;
                dst.set_pixel(dx, dy, t);
            } else {
                dst.set_pixel(dx, dy, c);
            }
        }
    }
    dst.alpha_blending_flag = old_blendmode;
    Some(dst)
}

/// gdImageRotate90(): counter clockwise
pub fn rotate90(src: &Image, ignoretransparent: bool) -> Option<Image> {
    rotate_by(src, ignoretransparent, true, |x, y, _, dsy| {
        (y, dsy - x - 1)
    })
}

/// gdImageRotate180()
pub fn rotate180(src: &Image, ignoretransparent: bool) -> Option<Image> {
    rotate_by(src, ignoretransparent, false, |x, y, dsx, dsy| {
        (dsx - x - 1, dsy - y - 1)
    })
}

/// gdImageRotate270()
pub fn rotate270(src: &Image, ignoretransparent: bool) -> Option<Image> {
    rotate_by(src, ignoretransparent, true, |x, y, dsx, _| {
        (dsx - y - 1, x)
    })
}
