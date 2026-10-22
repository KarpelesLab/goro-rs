//! Filters, quantizer and color matching against php-src's test images and
//! references produced by php 8.3 (libgd 2.3.3, whose filter and gd_topal
//! code matches the bundled one). Skipped when the php-src checkout or the
//! reference directory is not available.

use goro_gd::*;
use std::path::Path;

const TESTS: &str = "/home/magicaltux/php-src/ext/gd/tests";
const REF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ref/filters");

fn load(path: &str) -> Option<Image> {
    let d = std::fs::read(path).ok()?;
    png::create_from_png(&d)
}

fn diff(a: &Image, b: &Image) -> usize {
    assert_eq!((a.sx, a.sy), (b.sx, b.sy));
    let mut n = 0;
    for y in 0..a.sy {
        for x in 0..a.sx {
            if a.get_true_color_pixel(x, y) != b.get_true_color_pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

fn available() -> bool {
    Path::new(TESTS).exists()
}

#[test]
fn bug47946_convolution_keeps_background() {
    if !available() {
        return;
    }
    let mut im = Image::create_true_color(40, 40).unwrap();
    im.alpha_blending_flag = 0;
    im.filled_rectangle(0, 0, 39, 39, 0x7fffffff);
    im.filled_ellipse(19, 19, 20, 20, 0x00ff00);
    im.save_alpha_flag = 1;
    let m = [[1.0, 0.0, 1.0], [0.0, 5.0, 0.0], [1.0, 0.0, 1.0]];
    assert!(filter::convolution(&mut im, &m, 9.0, 1.0));
    let exp = load(&format!("{}/bug47946_exp.png", TESTS)).unwrap();
    assert_eq!(diff(&exp, &im), 0);
}

#[test]
fn imageconvolution_basic() {
    if !available() {
        return;
    }
    let mut im = Image::create_true_color(180, 30).unwrap();
    im.string(Font::builtin(5), 10, 8, b"Gaussian Blur Text", 0x00ff00);
    let m = [[1.0, 2.0, 1.0], [2.0, 4.0, 2.0], [1.0, 2.0, 1.0]];
    assert!(filter::convolution(&mut im, &m, 16.0, 0.0));
    let exp = load(&format!("{}/imageconvolution_basic.png", TESTS)).unwrap();
    assert_eq!(diff(&exp, &im), 0);
}

#[test]
fn imagetruecolortopalette_basic() {
    if !available() {
        return;
    }
    let mut im = Image::create_true_color(150, 150).unwrap();
    let a = im.color_allocate(255, 0, 255);
    let b = im.color_allocate(0, 255, 255);
    im.filled_arc(75, 75, 70, 70, 0, 180, a, PIE);
    im.filled_arc(75, 55, 80, 70, 0, -180, b, PIE);
    assert!(topal::true_color_to_palette(&mut im, true, 2));
    im.palette_to_true_color();
    let exp = load(&format!("{}/imagetruecolortopalette_basic.png", TESTS)).unwrap();
    assert_eq!(diff(&exp, &im), 0);
}

#[test]
fn bug75124_grayscale() {
    if !available() {
        return;
    }
    let mut im = load(&format!("{}/bug75124.png", TESTS)).unwrap();
    assert!(im.true_color);
    filter::grayscale(&mut im);
    for y in 0..im.sy {
        for x in 0..im.sx {
            let c = im.get_pixel(x, y);
            let (r, g, b) = (
                true_color_get_red(c),
                true_color_get_green(c),
                true_color_get_blue(c),
            );
            assert!(r == g && g == b);
        }
    }
}

#[test]
fn bug72512_transparent_out_of_range() {
    let mut im = Image::create_true_color(100, 100).unwrap();
    im.color_transparent(-1000000);
    assert!(topal::true_color_to_palette(&mut im, true, 3));
    im.color_transparent(9);
}

#[test]
fn colormatch_errors() {
    let im = Image::create_true_color(5, 5).unwrap();
    let mut im2 = Image::create(5, 5).unwrap();
    assert_eq!(color_match::color_match(&im, &mut im2), -4);
    let mut im3 = Image::create(6, 5).unwrap();
    im3.color_allocate(0, 0, 0);
    assert_eq!(color_match::color_match(&im, &mut im3), -3);
    let mut tc = Image::create_true_color(5, 5).unwrap();
    assert_eq!(color_match::color_match(&im, &mut tc), -2);
    assert_eq!(color_match::color_match(&im2.clone(), &mut im2), -1);
}

fn ref_available() -> bool {
    available() && Path::new(REF).exists()
}

fn src() -> Image {
    load(&format!("{}/test.png", TESTS)).unwrap()
}

#[test]
fn filters_match_reference() {
    if !ref_available() {
        return;
    }
    type F = fn(&mut Image) -> bool;
    let cases: Vec<(&str, F)> = vec![
        ("negate", filter::negate),
        ("grayscale", filter::grayscale),
        ("edge", filter::edge_detect_quick),
        ("gauss", filter::gaussian_blur),
        ("selective", filter::selective_blur),
        ("emboss", filter::emboss),
        ("mean", filter::mean_removal),
        ("smooth", |im| filter::smooth(im, -1924.124)),
        ("smooth2", |im| filter::smooth(im, 5.0)),
        ("colorize", |im| filter::color(im, -127, -127, 127, 0)),
        ("colorize2", |im| filter::color(im, 20, -30, 40, 50)),
        ("contrast", |im| filter::contrast(im, -90.0)),
        ("contrast2", |im| filter::contrast(im, 40.0)),
        ("brightness", |im| filter::brightness(im, 98)),
        ("brightness2", |im| filter::brightness(im, -50)),
        ("pixelate", |im| {
            filter::pixelate(im, 5, filter::PIXELATE_AVERAGE)
        }),
        ("pixelate2", |im| {
            filter::pixelate(im, 7, filter::PIXELATE_UPPERLEFT)
        }),
    ];
    let mut bad = Vec::new();
    for (name, f) in cases {
        for mode in ["tc", "pal"] {
            let mut im = src();
            if mode == "pal" {
                topal::true_color_to_palette(&mut im, false, 64);
            }
            assert!(f(&mut im));
            im.palette_to_true_color();
            let exp = load(&format!("{}/{}-{}.png", REF, name, mode)).unwrap();
            let d = diff(&exp, &im);
            if d != 0 {
                bad.push(format!("{}-{}: {} pixels", name, mode, d));
            }
        }
    }
    assert!(bad.is_empty(), "{:?}", bad);
}

#[test]
fn topal_matches_reference() {
    if !ref_available() {
        return;
    }
    let mut bad = Vec::new();
    for (d, n, total) in [
        (false, 256, 156),
        (true, 256, 156),
        (false, 16, 16),
        (true, 16, 16),
        (true, 3, 3),
        (false, 2, 2),
    ] {
        let mut im = src();
        assert!(topal::true_color_to_palette(&mut im, d, n));
        assert_eq!(im.colors_total, total);
        im.palette_to_true_color();
        let exp = load(&format!("{}/topal-{}-{}.png", REF, d as i32, n)).unwrap();
        let c = diff(&exp, &im);
        if c != 0 {
            bad.push(format!("{} {}: {} pixels", d, n, c));
        }
    }
    let mut im = src();
    let t = im.get_pixel(10, 10);
    im.color_transparent(t);
    let pal = topal::create_palette_from_true_color(&im, true, 50).unwrap();
    assert_eq!(pal.transparent, 50);
    assert_eq!(pal.colors_total, 51);
    assert!(topal::true_color_to_palette(&mut im, true, 50));
    assert_eq!(Image::compare(&pal, &im) & GD_CMP_IMAGE, 0);
    assert_eq!(im.transparent, 50);
    assert_eq!(im.colors_total, 51);
    im.palette_to_true_color();
    let exp = load(&format!("{}/topal-trans.png", REF)).unwrap();
    let c = diff(&exp, &im);
    if c != 0 {
        bad.push(format!("trans: {} pixels", c));
    }
    assert!(bad.is_empty(), "{:?}", bad);
}

#[test]
fn colormatch_reference() {
    if !ref_available() {
        return;
    }
    let im = src();
    let mut im2 = src();
    topal::true_color_to_palette(&mut im2, false, 8);
    assert_eq!(color_match::color_match(&im, &mut im2), 0);
    let got: Vec<String> = (0..im2.colors_total as usize)
        .map(|i| {
            format!(
                "{},{},{},{}",
                im2.red[i], im2.green[i], im2.blue[i], im2.alpha[i]
            )
        })
        .collect();
    let exp: Vec<&str> = vec![
        "255,42,0,0",
        "186,170,0,0",
        "129,212,0,0",
        "254,154,0,0",
        "165,192,0,0",
        "254,98,0,0",
        "67,254,0,0",
        "255,203,0,0",
    ];
    assert_eq!(got, exp);
}

#[test]
fn scatter_deterministic() {
    let mut a = src_or_blank();
    let mut b = a.clone();
    let s = filter::Scatter {
        sub: 3,
        plus: 5,
        colors: vec![],
        seed: 1234,
    };
    assert!(filter::scatter_ex(&mut a, &s));
    assert!(filter::scatter_ex(&mut b, &s));
    assert_eq!(diff(&a, &b), 0);
    assert!(!filter::scatter_ex(
        &mut a,
        &filter::Scatter {
            sub: 5,
            plus: 3,
            ..Default::default()
        }
    ));
    assert!(filter::scatter(&mut a, 0, 0));
}

fn src_or_blank() -> Image {
    if available() {
        src()
    } else {
        Image::create_true_color(20, 20).unwrap()
    }
}
