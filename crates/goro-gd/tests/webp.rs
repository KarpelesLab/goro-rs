//! The php-src ext/gd WebP tests (webp_basic, bug66590, bug66590_1,
//! bug70102, gh13774, imagecreatefromstring_webp) on goro-gd.

use goro_gd::*;

const TESTS: &str = "/home/magicaltux/php-src/ext/gd/tests";

/// func.inc's mse()
fn mse(a: &Image, b: &Image) -> f64 {
    assert_eq!((a.sx, a.sy), (b.sx, b.sy));
    let (mut e, mut n) = (0f64, 0f64);
    for y in 0..a.sy {
        for x in 0..a.sx {
            let c1 = a.get_pixel(x, y);
            let c2 = b.get_pixel(x, y);
            for s in [16, 8, 0] {
                let d = ((c1 >> s) & 255) - ((c2 >> s) & 255);
                e += (d * d) as f64;
                n += 1.0;
            }
        }
    }
    e / n
}

fn webp_basic_image() -> Image {
    let mut im = Image::create_true_color(75, 75).unwrap();
    let white = im.color_allocate(255, 255, 255);
    let red = im.color_allocate(255, 0, 0);
    let green = im.color_allocate(0, 255, 0);
    let blue = im.color_allocate(0, 0, 255);
    im.filled_rectangle(0, 0, 74, 74, white);
    im.line(3, 3, 71, 71, red);
    im.ellipse(18, 54, 36, 36, green);
    im.rectangle(41, 3, 71, 33, blue);
    im
}

#[test]
fn webp_basic() {
    let im1 = webp_basic_image();
    let data = webp::image_webp(&im1, -1).unwrap();
    let im2 = webp::create_from_webp(&data).unwrap();
    assert!(im2.true_color && im2.save_alpha_flag == 1);
    // imagewebp($im2) must work too
    let again = webp::image_webp(&im2, -1).unwrap();
    assert!(webp::create_from_webp(&again).is_some());
    let e = mse(&im1, &im2);
    assert!(e < 500.0, "lossy mse {}", e);
    let lossless = webp::image_webp(&im1, webp::WEBP_LOSSLESS).unwrap();
    let im3 = webp::create_from_webp(&lossless).unwrap();
    assert_eq!(mse(&im1, &im3), 0.0);
    take_errors();
}

#[test]
fn riff_size_is_padded() {
    // bug66590 / bug66590_1
    for (w, h, q) in [(75, 75, -1), (6, 6, -1), (5, 3, 101), (7, 1, 50)] {
        let mut im = Image::create_true_color(w, h).unwrap();
        let red = im.color_allocate(255, 0, 0);
        im.filled_rectangle(0, 0, w - 1, h - 1, red);
        let d = webp::image_webp(&im, q).unwrap();
        let len = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize + 8;
        assert_eq!(len, d.len());
        assert_eq!(d.len() % 2, 0);
    }
}

#[test]
fn white_stays_white() {
    // bug70102
    let mut im = Image::create_true_color(8, 8).unwrap();
    let white = im.color_allocate(255, 255, 255);
    im.filled_rectangle(0, 0, 7, 7, white);
    let d = webp::image_webp(&im, -1).unwrap();
    let back = webp::create_from_webp(&d).unwrap();
    assert_eq!(back.get_pixel(4, 4) & 0xffffff, 0xffffff);
}

#[test]
fn alpha_round_trips() {
    let mut im = Image::create_true_color(16, 16).unwrap();
    im.alpha_blending_flag = 0;
    for y in 0..16 {
        for x in 0..16 {
            im.set_pixel(
                x,
                y,
                true_color_alpha(x * 16, y * 16, 128, (x * 8).min(127)),
            );
        }
    }
    let d = webp::image_webp(&im, 101).unwrap();
    let back = webp::create_from_webp(&d).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let a = true_color_get_alpha(im.get_pixel(x, y));
            let b = true_color_get_alpha(back.get_pixel(x, y));
            assert_eq!(a, b, "alpha at {},{}", x, y);
        }
    }
    let d = webp::image_webp(&im, 80).unwrap();
    let back = webp::create_from_webp(&d).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let a = true_color_get_alpha(im.get_pixel(x, y));
            let b = true_color_get_alpha(back.get_pixel(x, y));
            assert_eq!(a, b, "lossy alpha at {},{}", x, y);
        }
    }
}

#[test]
fn bad_input_and_palette() {
    // gh13774: an empty file
    take_errors();
    assert!(webp::create_from_webp(b"").is_none());
    let e = take_errors();
    assert_eq!(e[0].1, "gd-webp cannot get webp info");
    let im = Image::create(1, 1).unwrap();
    assert!(webp::image_webp(&im, -1).is_none());
    assert_eq!(take_errors()[0].1, "Palette image not supported by webp");
}

#[test]
fn imagecreatefromstring_webp() {
    let Ok(data) = std::fs::read(format!("{}/imagecreatefromstring.webp", TESTS)) else {
        return;
    };
    let im = webp::create_from_webp(&data).unwrap();
    assert_eq!((im.sx, im.sy), (10, 10));
}
