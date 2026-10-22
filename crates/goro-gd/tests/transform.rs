//! The php-src ext/gd phpt tests about scaling, rotation, affine
//! transformations, cropping and flipping, replayed on the library.

use goro_gd::*;

const TESTS: &str = "/home/magicaltux/php-src/ext/gd/tests";

fn load_png(name: &str) -> Image {
    let data = std::fs::read(format!("{}/{}", TESTS, name)).unwrap();
    png::create_from_png(&data).unwrap()
}

/// test_image_equals_file() of func.inc: number of differing pixels
fn diff_with_file(name: &str, actual: &Image) -> usize {
    let exp = load_png(name);
    assert!(exp.true_color && actual.true_color);
    assert_eq!((exp.sx, exp.sy), (actual.sx, actual.sy), "size of {}", name);
    let mut n = 0;
    for y in 0..exp.sy {
        for x in 0..exp.sx {
            if exp.get_pixel(x, y) != actual.get_pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

/// imagescale() of gd.c
fn php_scale(im: &mut Image, w: u32, h: u32, method: i32) -> Option<Image> {
    let old = im.interpolation_id;
    let r = if interpolation::set_interpolation_method(im, method) {
        interpolation::scale(im, w, h)
    } else {
        None
    };
    interpolation::set_interpolation_method(im, old);
    r
}

#[test]
fn bug24155_rotate_270() {
    let mut im = Image::create_true_color(30, 50).unwrap();
    im.fill(0, 0, 16777215 - 255);
    let im = interpolation::rotate_interpolated(&mut im, 270.0, 255).unwrap();
    let back = png::create_from_png(&png::image_png(&im, -1, -1).unwrap()).unwrap();
    assert_eq!(back.get_pixel(20, 20), 16776960);
}

#[test]
fn bug39366_rotate_180() {
    let mut im = Image::create_true_color(10, 10).unwrap();
    im.set_alpha_blending(0);
    im.filled_rectangle(0, 0, 8, 8, 0x32FF0000);
    let mut r = interpolation::rotate_interpolated(&mut im, 180.0, 0).unwrap();
    r.color_transparent(0);
    assert_eq!(r.get_pixel(5, 5), 0x32FF0000);
}

#[test]
fn bug70976_and_bug66882() {
    let mut im = Image::create(10, 10).unwrap();
    assert!(interpolation::rotate_interpolated(&mut im, 45.0, 0x7ffffff9).is_some());
    let mut im = Image::create(10, 10).unwrap();
    let r = interpolation::rotate_interpolated(&mut im, -90.0, 0).unwrap();
    assert_eq!((r.sx, r.sy), (10, 10));
    let mut im = Image::create(10, 10).unwrap();
    assert!(interpolation::rotate_interpolated(&mut im, 5.0, -9999999).is_none());
}

#[test]
fn bug65148_rotated_sizes() {
    let methods = [
        1, 2, 4, 5, 3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17, 18, 19, 16, 21, 20,
    ];
    let mut img = Image::create(40, 20).unwrap();
    for m in methods {
        interpolation::set_interpolation_method(&mut img, m);
        let t = img.color_allocate_alpha(0, 0, 0, 127);
        let r = interpolation::rotate_interpolated(&mut img, -5.0, t).unwrap();
        assert_eq!((r.sx, r.sy), (40, 23), "method {}", m);
    }
}

#[test]
fn bug68629_transparent_rotate() {
    let mut im = Image::create_true_color(200, 200).unwrap();
    let t = im.color_allocate_alpha(0, 0, 0, 127);
    im.fill(0, 0, t);
    let r = interpolation::rotate_interpolated(&mut im, 45.0, t).unwrap();
    for y in 0..r.sy {
        for x in 0..r.sx {
            assert_eq!(r.get_pixel(x, y), 0x7f000000);
        }
    }
}

#[test]
fn bug73272_scale_and_rotate() {
    let mut src = Image::create_true_color(100, 100).unwrap();
    src.filled_rectangle(0, 0, 99, 99, 0xFFFFFF);
    src.ellipse(49, 49, 40, 40, 0);
    interpolation::set_interpolation_method(&mut src, interp::NEAREST_NEIGHBOUR);
    php_scale(&mut src, 200, 200, interp::BILINEAR_FIXED);
    let dst = interpolation::rotate_interpolated(&mut src, 60.0, 0xFFFFFF).unwrap();
    assert_eq!(diff_with_file("bug73272.png", &dst), 0);
}

#[test]
fn gd223_bicubic_rotate() {
    let mut im = Image::create_true_color(64, 64).unwrap();
    for j in 0..64 {
        for i in 0..64 {
            im.set_pixel(
                i,
                j,
                if i % 2 != 0 || j % 2 != 0 {
                    0
                } else {
                    0xffffff
                },
            );
        }
    }
    interpolation::set_interpolation_method(&mut im, interp::BICUBIC);
    let im = interpolation::rotate_interpolated(&mut im, 45.0, 0xff0000).unwrap();
    assert_eq!(diff_with_file("gd223.png", &im), 0);
}

#[test]
fn gh10614_right_angles() {
    let mut input = load_png("gh10614.png");
    let expect: [[u32; 16]; 4] = {
        let o = 0u32;
        let t = 0x7f000000u32;
        [
            [o, o, o, o, o, o, o, o, t, t, t, t, t, t, t, t],
            [t, t, o, o, t, t, o, o, t, t, o, o, t, t, o, o],
            [t, t, t, t, t, t, t, t, o, o, o, o, o, o, o, o],
            [o, o, t, t, o, o, t, t, o, o, t, t, o, o, t, t],
        ]
    };
    let mut last = None;
    for (k, angle) in [0.0f32, 90.0, 180.0, 270.0].iter().enumerate() {
        let out = interpolation::rotate_interpolated(&mut input, *angle, 0).unwrap();
        let mut got = Vec::new();
        for i in 0..4 {
            for j in 0..4 {
                got.push(out.get_pixel(i, j) as u32);
            }
        }
        assert_eq!(got, expect[k].to_vec(), "angle {}", angle);
        last = Some(out);
    }
    // gh16559
    let mut out = last.unwrap();
    let h = 64 * out.sx / out.sy;
    assert!(php_scale(&mut out, h as u32, 64, interp::BICUBIC).is_some());
}

#[test]
fn scale_bugs() {
    // bug72227, bug69024
    let mut img = Image::create_true_color(100, 100).unwrap();
    assert!(php_scale(&mut img, 13, 1, interp::BICUBIC).is_some());
    let mut img = Image::create(256, 256).unwrap();
    assert!(php_scale(&mut img, 32, 32, interp::BICUBIC).is_some());
    // bug73279
    let mut src = Image::create(100, 100).unwrap();
    src.color_allocate(255, 255, 255);
    let dst = php_scale(&mut src, 200, 200, interp::BILINEAR_FIXED).unwrap();
    assert_eq!(dst.get_pixel(99, 99), 0xffffff);
    // bug73281
    for tc in [true, false] {
        let mut src = if tc {
            Image::create_true_color(100, 100).unwrap()
        } else {
            Image::create(100, 100).unwrap()
        };
        let white = src.color_allocate(255, 255, 255);
        src.filled_rectangle(0, 0, 99, 99, white);
        let dst = php_scale(&mut src, 200, 200, interp::BILINEAR_FIXED).unwrap();
        for (x, y) in [(0, 0), (0, 199), (199, 199), (199, 0)] {
            assert_eq!(dst.get_pixel(x, y), 0xffffff, "{} {} {}", tc, x, y);
        }
    }
    // bug79676
    let mut src = Image::create_true_color(32, 32).unwrap();
    src.filled_rectangle(0, 0, 31, 31, 0xff00ff);
    for (s, m) in [(64, 3), (16, 3), (64, 4), (16, 4)] {
        let d = php_scale(&mut src, s, s, m).unwrap();
        assert_ne!(d.get_pixel(d.sx - 1, d.sy - 1), 0, "{} {}", s, m);
    }
    // bug72512_0-like: palette with odd transparent
    let mut img = Image::create(13, 20).unwrap();
    img.color_allocate(1, 2, 3);
    assert!(php_scale(&mut img, 1, 600, interp::BILINEAR_FIXED).is_some());
}

#[test]
fn bug77272_huge_palette_two_pass() {
    // the C test uses 2**28 x 1; a smaller width hits the same check path
    // only for huge sizes, so just check the generic pass works.
    let mut img = Image::create(300, 10).unwrap();
    img.color_allocate(10, 20, 30);
    let r = php_scale(&mut img, 1, 1, interp::TRIANGLE).unwrap();
    assert_eq!(r.get_pixel(0, 0), 0x0a141e);
    let _ = take_errors();
}

#[test]
fn every_scale_method_runs() {
    let mut src = load_png("gh10614.png");
    for m in 1..=21 {
        let mut s = src.clone_image().unwrap();
        let d = php_scale(&mut s, 37, 23, m);
        // WEIGHTED4 has no kernel: gdImageScale() fails
        assert_eq!(d.is_some(), m != interp::WEIGHTED4, "method {}", m);
    }
    let d = php_scale(&mut src, 7, 9, interp::NEAREST_NEIGHBOUR).unwrap();
    assert_eq!((d.sx, d.sy), (7, 9));
}

#[test]
fn affine() {
    // bug79067: not invertible
    let mut src = Image::create_true_color(8, 8).unwrap();
    assert!(
        interpolation::transform_affine_get_image(&mut src, None, &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0])
            .is_none()
    );
    // bug79068: interpolation restored
    let mut src = Image::create_true_color(100, 100).unwrap();
    src.filled_rectangle(0, 0, 99, 99, 0xffffff);
    src.line(10, 10, 90, 90, 0);
    interpolation::set_interpolation_method(&mut src, interp::BSPLINE);
    interpolation::transform_affine_get_image(&mut src, None, &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(
        interpolation::get_interpolation_method(&src),
        interp::BSPLINE
    );
    // gh17984
    let mut src = Image::create_true_color(8, 8).unwrap();
    assert!(
        interpolation::transform_affine_get_image(&mut src, None, &[45.0, 1.0, 3.0, 1.0, 5.0, 1.0])
            .is_some()
    );
    let m = [45.0, 1.0, 3.0, 1.0, 5.0, 1.0];
    assert_eq!(
        matrix::concat(&m, &m),
        [2028.0, 46.0, 138.0, 4.0, 233.0, 7.0]
    );
}

/// The bounding box of the bundled libgd (width is one less than
/// libgd 2.3.3's)
#[test]
fn affine_rotate_size() {
    let mut im = Image::create_true_color(20, 10).unwrap();
    for y in 0..10 {
        for x in 0..20 {
            im.set_pixel(x, y, (x * 12) << 16 | (y * 25) << 8 | 0x40);
        }
    }
    let m = matrix::rotate(30.0);
    let d = interpolation::transform_affine_get_image(&mut im, None, &m).unwrap();
    assert_eq!((d.sx, d.sy), (21, 18));
    let r = interpolation::rotate_interpolated(&mut im, 30.0, 0x123456).unwrap();
    assert_eq!((r.sx, r.sy), (21, 18));
}

#[test]
fn crop() {
    // bug67447
    let mut image = Image::create_true_color(500, 500).unwrap();
    let red = image.color_allocate(255, 0, 0);
    image.fill(0, 0, red);
    let c = crop::crop(
        &image,
        &Rect {
            x: 0,
            y: 0,
            width: 250,
            height: 250,
        },
    )
    .unwrap();
    assert_eq!(c.get_pixel(249, 249), red);
    let mut image = Image::create(500, 500).unwrap();
    image.color_allocate(0, 0, 255);
    let red = image.color_allocate(255, 0, 0);
    image.fill(0, 0, red);
    let c = crop::crop(
        &image,
        &Rect {
            x: 0,
            y: 0,
            width: 250,
            height: 250,
        },
    )
    .unwrap();
    let p = c.get_pixel(249, 249);
    assert_eq!(
        (c.red_of(p), c.green_of(p), c.blue_of(p), c.alpha_of(p)),
        (255, 0, 0, 0)
    );
    // bug66356
    let img = Image::create_true_color(10, 10).unwrap();
    assert!(
        crop::crop(
            &img,
            &Rect {
                x: 0,
                y: 0,
                width: -1,
                height: 10
            }
        )
        .is_none()
    );
    assert_eq!(
        take_errors(),
        vec![(
            GD_WARNING,
            "One parameter to a memory allocation multiplication is negative or zero, failing operation gracefully\n"
                .to_string()
        )]
    );
    assert!(
        crop::crop(
            &img,
            &Rect {
                x: -20,
                y: -20,
                width: 10,
                height: 10
            }
        )
        .is_some()
    );
    assert!(
        crop::crop(
            &img,
            &Rect {
                x: 0x7fffff00,
                y: 0,
                width: 10,
                height: 10
            }
        )
        .is_some()
    );
    assert!(
        crop::crop(
            &img,
            &Rect {
                x: 0,
                y: 0,
                width: 65535,
                height: 65535
            }
        )
        .is_none()
    );
    assert_eq!(take_errors()[0].1.chars().next(), Some('P'));
}

#[test]
fn cropauto() {
    let sz = |i: Option<Image>| i.map(|i| (i.sx, i.sy));
    let mut im = Image::create_true_color(99, 99).unwrap();
    im.filled_rectangle(20, 20, 30, 30, 0xff);
    assert_eq!(sz(crop::crop_auto(&im, 0)), Some((99, 99)));
    assert_eq!(sz(crop::crop_auto(&im, 4)), Some((11, 11)));
    let mut im = Image::create(99, 99).unwrap();
    im.filled_rectangle(20, 20, 30, 30, 0xff);
    assert_eq!(sz(crop::crop_auto(&im, 0)), Some((99, 99)));
    assert_eq!(sz(crop::crop_auto(&im, 4)), Some((11, 11)));
    let mut im = Image::create_true_color(50, 50).unwrap();
    im.filled_rectangle(20, 20, 30, 30, 0xff);
    assert_eq!(sz(crop::crop_auto(&im, 2)), Some((11, 11)));
    let mut im = Image::create(50, 50).unwrap();
    im.color_allocate(0, 0, 0);
    im.color_allocate(0, 0, 255);
    im.filled_rectangle(20, 20, 30, 30, 0xff);
    assert_eq!(sz(crop::crop_auto(&im, 2)), Some((11, 11)));
    let im = load_png("logo_noise.png");
    assert_eq!(sz(crop::crop_threshold(&im, 0, 0.1)), Some((240, 134)));

    // bug73291
    let mut src = Image::create_true_color(255, 255).unwrap();
    let white = src.color_allocate(255, 255, 255);
    src.filled_rectangle(0, 0, 254, 254, white);
    for i in (1..=254).rev() {
        let c = src.color_allocate(i, i, i);
        src.filled_ellipse(127, 127, i, i, c);
    }
    let got: Vec<_> = [0.1f32, 0.5, 1.0, 10.0]
        .iter()
        .map(|&t| sz(crop::crop_threshold(&src, white as u32, t)))
        .collect();
    assert_eq!(
        got,
        vec![
            Some((247, 247)),
            Some((237, 237)),
            Some((229, 229)),
            Some((175, 175))
        ]
    );

    // bug77198
    for y in 0..8 {
        for x in 0..8 {
            let mut im = Image::create_true_color(8, 8).unwrap();
            im.filled_rectangle(0, 0, 7, 7, 0xffffff);
            im.set_pixel(x, y, 0);
            let c = crop::crop_threshold(&im, 0xffffff, 1.0).unwrap();
            assert_eq!((c.sx, c.sy, c.get_pixel(0, 0)), (1, 1, 0));
            let corner = (x == 0 || x == 7) && (y == 0 || y == 7);
            if !corner {
                let c = crop::crop_auto(&im, 4).unwrap();
                assert_eq!((c.sx, c.sy, c.get_pixel(0, 0)), (1, 1, 0));
            }
        }
    }

    // bug77200
    let mut orig = Image::create_true_color(8, 8).unwrap();
    orig.filled_rectangle(0, 0, 3, 3, 0x00ff00);
    orig.filled_rectangle(4, 0, 7, 3, 0xff0000);
    orig.filled_rectangle(0, 4, 3, 7, 0x00ff00);
    orig.filled_rectangle(4, 4, 7, 7, 0x0000ff);
    assert_eq!(crop::crop_auto(&orig, 4).unwrap().sx, 4);
    orig.filled_rectangle(0, 0, 3, 3, 0xff0000);
    orig.filled_rectangle(4, 0, 7, 3, 0x00ff00);
    orig.filled_rectangle(0, 4, 3, 7, 0x0000ff);
    orig.filled_rectangle(4, 4, 7, 7, 0x00ff00);
    assert_eq!(crop::crop_auto(&orig, 4).unwrap().sx, 4);
}

#[test]
fn flip() {
    let mut im = Image::create_true_color(99, 99).unwrap();
    im.set_pixel(0, 0, 0xFF);
    im.set_pixel(0, 98, 0x00FF00);
    im.set_pixel(98, 0, 0xFF0000);
    im.set_pixel(98, 98, 0x0000FF);
    transform::flip_horizontal(&mut im);
    transform::flip_vertical(&mut im);
    transform::flip_both(&mut im);
    assert_eq!(
        [
            im.get_pixel(0, 0),
            im.get_pixel(0, 98),
            im.get_pixel(98, 0),
            im.get_pixel(98, 98)
        ],
        [0xff, 0xff00, 0xff0000, 0xff]
    );
}

#[test]
fn interpolation_ids() {
    let mut im = Image::create(8, 8).unwrap();
    for m in [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21,
    ] {
        assert!(interpolation::set_interpolation_method(&mut im, m));
        assert_eq!(interpolation::get_interpolation_method(&im), m);
    }
    assert!(interpolation::set_interpolation_method(&mut im, 0));
    assert_eq!(im.interpolation_id, interp::LINEAR);
    assert!(!interpolation::set_interpolation_method(&mut im, 23));
}
