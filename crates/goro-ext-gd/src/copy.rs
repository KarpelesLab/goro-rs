//! Copying between images: imagecopy, imagecopymerge,
//! imagecopymergegray, imagecopyresized, imagecopyresampled, and the
//! palette ⇄ truecolor conversions.
//!
//! Behaviour sources: the PHP manual pages of these functions, libgd's
//! API documentation, and the ext/gd tests (copy.phpt, copyresized.phpt,
//! imagecopymerge_basic.phpt: merged channels are truncated,
//! imagecopyresampled_variation1.phpt: colors are averaged weighted by
//! opacity, alpha averaged plainly).

use crate::image::*;

/// Maps a source pixel to a destination color.
fn map_color(dst: &mut Image, src: &Image, p: i32, cache: &mut [i32; 256]) -> i32 {
    if dst.truecolor {
        if src.truecolor {
            p
        } else {
            let i = (p & 0xff) as usize;
            tc(src.red[i], src.green[i], src.blue[i], src.alpha[i])
        }
    } else if src.truecolor {
        dst.color_resolve(tc_r(p), tc_g(p), tc_b(p), tc_a(p))
    } else {
        let i = (p & 0xff) as usize;
        if cache[i] < 0 {
            cache[i] = dst.color_resolve(src.red[i], src.green[i], src.blue[i], src.alpha[i]);
        }
        cache[i]
    }
}

/// imagecopy. `src` may be a snapshot of `dst` itself.
#[allow(clippy::too_many_arguments)]
pub fn copy(dst: &mut Image, src: &Image, dx: i32, dy: i32, sx: i32, sy: i32, w: i32, h: i32) {
    let mut cache = [-1i32; 256];
    let same_palette = !dst.truecolor && !src.truecolor && same_colors(dst, src);
    for y in 0..h {
        let syy = sy + y;
        if syy < 0 || syy >= src.sy {
            continue;
        }
        for x in 0..w {
            let sxx = sx + x;
            if sxx < 0 || sxx >= src.sx {
                continue;
            }
            let p = src.get_pixel(sxx, syy);
            if p == src.transparent {
                continue;
            }
            let c = if same_palette {
                p
            } else {
                map_color(dst, src, p, &mut cache)
            };
            dst.set_pixel(dx + x, dy + y, c);
        }
    }
}

fn same_colors(a: &Image, b: &Image) -> bool {
    std::ptr::eq(a, b)
}

/// imagecopymerge: `pct` percent of the source over the destination.
#[allow(clippy::too_many_arguments)]
pub fn copy_merge(
    dst: &mut Image,
    src: &Image,
    dx: i32,
    dy: i32,
    sx: i32,
    sy: i32,
    w: i32,
    h: i32,
    pct: i32,
) {
    merge_impl(dst, src, dx, dy, sx, sy, w, h, pct, false)
}

/// imagecopymergegray: as imagecopymerge, with the destination pixel
/// turned to its gray level first.
#[allow(clippy::too_many_arguments)]
pub fn copy_merge_gray(
    dst: &mut Image,
    src: &Image,
    dx: i32,
    dy: i32,
    sx: i32,
    sy: i32,
    w: i32,
    h: i32,
    pct: i32,
) {
    merge_impl(dst, src, dx, dy, sx, sy, w, h, pct, true)
}

#[allow(clippy::too_many_arguments)]
fn merge_impl(
    dst: &mut Image,
    src: &Image,
    dx: i32,
    dy: i32,
    sx: i32,
    sy: i32,
    w: i32,
    h: i32,
    pct: i32,
    gray: bool,
) {
    for y in 0..h {
        let syy = sy + y;
        let tyy = dy + y;
        for x in 0..w {
            let sxx = sx + x;
            let txx = dx + x;
            if !src.in_bounds(sxx, syy) || !dst.in_bounds(txx, tyy) {
                continue;
            }
            let p = src.get_pixel(sxx, syy);
            if p == src.transparent {
                continue;
            }
            let (sr, sg, sb) = (src.red_of(p), src.green_of(p), src.blue_of(p));
            if pct == 100 && !gray {
                let c = if dst.truecolor {
                    tc(sr, sg, sb, src.alpha_of(p))
                } else {
                    dst.color_resolve(sr, sg, sb, src.alpha_of(p))
                };
                dst.set_pixel(txx, tyy, c);
                continue;
            }
            let dc = dst.get_pixel(txx, tyy);
            let (mut dr, mut dg, mut db) = (
                dst.red_of(dc) as f64,
                dst.green_of(dc) as f64,
                dst.blue_of(dc) as f64,
            );
            if gray {
                let g = 0.299 * dr + 0.587 * dg + 0.114 * db;
                dr = g;
                dg = g;
                db = g;
            }
            let f = pct as f64 / 100.0;
            let mix = |s: i32, d: f64| (s as f64 * f + d * (1.0 - f)) as i32;
            let (nr, ng, nb) = (mix(sr, dr), mix(sg, dg), mix(sb, db));
            let c = if dst.truecolor {
                tc(nr, ng, nb, src.alpha_of(p))
            } else {
                let e = dst.color_exact(nr, ng, nb, 0);
                if e == -1 {
                    let a = dst.color_allocate(nr, ng, nb, 0);
                    if a == -1 {
                        dst.color_closest(nr, ng, nb, 0)
                    } else {
                        a
                    }
                } else {
                    e
                }
            };
            dst.set_pixel(txx, tyy, c);
        }
    }
}

/// imagecopyresized: nearest-neighbour scaling.
#[allow(clippy::too_many_arguments)]
pub fn copy_resized(
    dst: &mut Image,
    src: &Image,
    dx: i32,
    dy: i32,
    sx: i32,
    sy: i32,
    dw: i32,
    dh: i32,
    sw: i32,
    sh: i32,
) {
    if dw <= 0 || dh <= 0 || sw <= 0 || sh <= 0 {
        return;
    }
    // How many destination pixels each source column/row covers.
    let stx: Vec<i32> = (0..sw)
        .map(|i| ((i as i64 + 1) * dw as i64 / sw as i64 - i as i64 * dw as i64 / sw as i64) as i32)
        .collect();
    let sty: Vec<i32> = (0..sh)
        .map(|i| ((i as i64 + 1) * dh as i64 / sh as i64 - i as i64 * dh as i64 / sh as i64) as i32)
        .collect();
    let mut cache = [-1i32; 256];
    let mut toy = dy;
    for (yi, &ry) in sty.iter().enumerate() {
        let syy = sy + yi as i32;
        for _ in 0..ry {
            let mut tox = dx;
            for (xi, &rx) in stx.iter().enumerate() {
                let sxx = sx + xi as i32;
                if rx == 0 {
                    continue;
                }
                if !src.in_bounds(sxx, syy) {
                    tox += rx;
                    continue;
                }
                let p = src.get_pixel(sxx, syy);
                if p == src.transparent && !(dst.truecolor && src.truecolor) {
                    tox += rx;
                    continue;
                }
                let c = if !dst.truecolor && !src.truecolor && std::ptr::eq(dst, src) {
                    p
                } else {
                    map_color(dst, src, p, &mut cache)
                };
                if src.truecolor && p == src.transparent {
                    tox += rx;
                    continue;
                }
                for _ in 0..rx {
                    dst.set_pixel(tox, toy, c);
                    tox += 1;
                }
            }
            toy += 1;
        }
    }
}

/// imagecopyresampled: area-averaging scale with opacity-weighted color.
#[allow(clippy::too_many_arguments)]
pub fn copy_resampled(
    dst: &mut Image,
    src: &Image,
    dx: i32,
    dy: i32,
    sx: i32,
    sy: i32,
    dw: i32,
    dh: i32,
    sw: i32,
    sh: i32,
) {
    if !dst.truecolor {
        copy_resized(dst, src, dx, dy, sx, sy, dw, dh, sw, sh);
        return;
    }
    if dw <= 0 || dh <= 0 || sw == 0 || sh == 0 {
        return;
    }
    for y in dy..dy + dh {
        let sy1 = (y - dy) as f64 * sh as f64 / dh as f64;
        let sy2 = (y + 1 - dy) as f64 * sh as f64 / dh as f64;
        for x in dx..dx + dw {
            if !dst.in_bounds(x, y) {
                continue;
            }
            let sx1 = (x - dx) as f64 * sw as f64 / dw as f64;
            let sx2 = (x + 1 - dx) as f64 * sw as f64 / dw as f64;
            let (mut red, mut green, mut blue, mut alpha) = (0.0, 0.0, 0.0, 0.0);
            let mut alpha_sum = 0.0;
            let mut contrib_sum = 0.0;
            let mut syy = sy1;
            loop {
                let yportion = if syy.floor() == sy1.floor() {
                    let p = 1.0 - (syy - syy.floor());
                    let p = if p > sy2 - sy1 { sy2 - sy1 } else { p };
                    syy = syy.floor();
                    p
                } else if syy == sy2.floor() {
                    sy2 - sy2.floor()
                } else {
                    1.0
                };
                let mut sxx = sx1;
                loop {
                    let xportion = if sxx.floor() == sx1.floor() {
                        let p = 1.0 - (sxx - sxx.floor());
                        let p = if p > sx2 - sx1 { sx2 - sx1 } else { p };
                        sxx = sxx.floor();
                        p
                    } else if sxx == sx2.floor() {
                        sx2 - sx2.floor()
                    } else {
                        1.0
                    };
                    let pcontribution = xportion * yportion;
                    let px = sxx as i32 + sx;
                    let py = syy as i32 + sy;
                    let p = src.get_truecolor_pixel(px.min(src.sx - 1), py.min(src.sy - 1));
                    let alpha_factor = (ALPHA_MAX - tc_a(p)) as f64 * pcontribution;
                    red += tc_r(p) as f64 * alpha_factor;
                    green += tc_g(p) as f64 * alpha_factor;
                    blue += tc_b(p) as f64 * alpha_factor;
                    alpha += tc_a(p) as f64 * pcontribution;
                    alpha_sum += alpha_factor;
                    contrib_sum += pcontribution;
                    sxx += 1.0;
                    if sxx >= sx2 {
                        break;
                    }
                }
                syy += 1.0;
                if syy >= sy2 {
                    break;
                }
            }
            let (r, g, b) = if alpha_sum != 0.0 {
                (red / alpha_sum, green / alpha_sum, blue / alpha_sum)
            } else {
                (0.0, 0.0, 0.0)
            };
            let a = if contrib_sum != 0.0 {
                alpha / contrib_sum
            } else {
                0.0
            };
            let clampc = |v: f64| -> i32 { (v + 0.5).clamp(0.0, 255.0) as i32 };
            let a = (a + 0.5).clamp(0.0, ALPHA_MAX as f64) as i32;
            let c = tc(clampc(r), clampc(g), clampc(b), a);
            dst.set_pixel(x, y, c);
        }
    }
}

/// imagepalettetotruecolor.
pub fn palette_to_truecolor(im: &mut Image) {
    if im.truecolor {
        return;
    }
    let n = (im.sx as usize) * (im.sy as usize);
    let mut t = Vec::with_capacity(n);
    for &p in &im.pixels {
        let i = p as usize;
        let c = if im.transparent == p as i32 {
            tc(im.red[i], im.green[i], im.blue[i], ALPHA_TRANSPARENT)
        } else {
            tc(im.red[i], im.green[i], im.blue[i], im.alpha[i])
        };
        t.push(c);
    }
    im.tpixels = t;
    im.pixels = Vec::new();
    im.truecolor = true;
    im.alpha_blending = EFFECT_ALPHABLEND;
    im.save_alpha = true;
    if im.transparent >= 0 {
        let i = im.transparent as usize;
        im.transparent = tc(im.red[i], im.green[i], im.blue[i], ALPHA_TRANSPARENT);
    }
}
