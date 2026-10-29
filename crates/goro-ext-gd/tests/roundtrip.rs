//! Codec round trips and a few drawing invariants.

use goro_ext_gd::formats::{bmp, gd, gif, png, wbmp, xbm};
use goro_ext_gd::image::{Image, tc};

fn sample_truecolor() -> Image {
    let mut im = Image::new_truecolor(37, 21).unwrap();
    for y in 0..21 {
        for x in 0..37 {
            im.put_raw(x, y, tc(x * 7 % 256, y * 11 % 256, (x + y) % 256, (x % 3) * 40));
        }
    }
    im.save_alpha = true;
    im
}

fn sample_palette() -> Image {
    let mut im = Image::new_palette(19, 13).unwrap();
    for i in 0..5 {
        im.color_allocate(i * 50, 255 - i * 50, i * 10, 0);
    }
    for y in 0..13 {
        for x in 0..19 {
            im.put_raw(x, y, (x + y) % 5);
        }
    }
    im
}

fn same_pixels(a: &Image, b: &Image) {
    assert_eq!((a.sx, a.sy), (b.sx, b.sy));
    for y in 0..a.sy {
        for x in 0..a.sx {
            assert_eq!(
                a.get_truecolor_pixel(x, y),
                b.get_truecolor_pixel(x, y),
                "pixel {x},{y}"
            );
        }
    }
}

#[test]
fn png_truecolor_and_palette() {
    for interlace in [false, true] {
        let mut a = sample_truecolor();
        a.interlace = interlace;
        let b = png::decode(&png::encode(&a, -1, -1)).unwrap();
        same_pixels(&a, &b);
        let mut p = sample_palette();
        p.interlace = interlace;
        let q = png::decode(&png::encode(&p, 9, -1)).unwrap();
        assert!(!q.truecolor);
        same_pixels(&p, &q);
    }
}

#[test]
fn gif_palette() {
    let p = sample_palette();
    let q = gif::decode(&gif::encode(&p)).unwrap();
    same_pixels(&p, &q);
}

#[test]
fn gd_and_gd2() {
    let a = sample_truecolor();
    same_pixels(&a, &gd::decode_gd(&gd::encode_gd(&a)).unwrap());
    for mode in [1, 2] {
        let b = gd::decode_gd2(&gd::encode_gd2(&a, 64, mode), None).unwrap();
        same_pixels(&a, &b);
    }
    let p = sample_palette();
    same_pixels(&p, &gd::decode_gd(&gd::encode_gd(&p)).unwrap());
}

#[test]
fn bmp_both_kinds() {
    let p = sample_palette();
    for compressed in [false, true] {
        same_pixels(&p, &bmp::decode(&bmp::encode(&p, compressed)).unwrap());
    }
    let mut a = sample_truecolor();
    // BMP keeps no alpha.
    for v in a.tpixels.iter_mut() {
        *v &= 0xffffff;
    }
    same_pixels(&a, &bmp::decode(&bmp::encode(&a, false)).unwrap());
}

#[test]
fn bilevel_formats() {
    let mut p = Image::new_palette(10, 3).unwrap();
    p.color_allocate(255, 255, 255, 0);
    p.color_allocate(0, 0, 0, 0);
    p.put_raw(3, 1, 1);
    let w = wbmp::decode(&wbmp::encode(&p, 1).unwrap()).unwrap();
    same_pixels(&p, &w);
    let x = xbm::decode(&xbm::encode(&p, "t", 1)).unwrap();
    same_pixels(&p, &x);
}

#[test]
fn ellipse_points_truncate_toward_the_centre() {
    // imagearc_basic.png: a 30x30 arc reaches x = 64 at 10 degrees.
    assert_eq!(goro_ext_gd::draw::arc_point(50, 50, 30, 30, 10), (64, 52));
    assert_eq!(goro_ext_gd::draw::arc_point(250, 250, 500, 250, 120), (125, 358));
}
