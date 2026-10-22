//! The php-src ext/gd AVIF tests (avif_decode_encode,
//! imagecreatefromstring_avif) on goro-gd.

use goro_gd::*;

const TESTS: &str = "/home/magicaltux/php-src/ext/gd/tests";

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

#[test]
fn avif_decode_encode() {
    let Ok(data) = std::fs::read(format!("{}/girl.avif", TESTS)) else {
        return;
    };
    let img = avif::create_from_avif(&data).unwrap();
    assert_eq!((img.sx, img.sy), (102, 121));
    for (q, s) in [(-1, 6), (70, 6), (70, 5), (-1, 6)] {
        let out = avif::image_avif(&img, q, s).unwrap();
        let back = avif::create_from_avif(&out).unwrap();
        assert!(mse(&img, &back) < 50.0, "quality {}", q);
    }
    let out = avif::image_avif(&img, 100, 0).unwrap();
    if let Ok(dir) = std::env::var("GD_AVIF_OUT") {
        std::fs::write(format!("{}/lossless.avif", dir), &out).unwrap();
        std::fs::write(
            format!("{}/q70.avif", dir),
            avif::image_avif(&img, 70, 6).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("{}/q95.avif", dir),
            avif::image_avif(&img, 95, 6).unwrap(),
        )
        .unwrap();
    }
    let back = avif::create_from_avif(&out).unwrap();
    assert_eq!(mse(&img, &back), 0.0);
    // the alpha channel survives the lossless encode too
    for y in 0..img.sy {
        for x in 0..img.sx {
            assert_eq!(img.get_pixel(x, y), back.get_pixel(x, y));
        }
    }
    assert!(take_errors().is_empty());
}

#[test]
fn imagecreatefromstring_avif() {
    for (f, w, h) in [
        ("imagecreatefromstring_major_brand.avif", 250, 375),
        ("imagecreatefromstring_compatible_brand.avif", 480, 270),
    ] {
        let Ok(data) = std::fs::read(format!("{}/{}", TESTS, f)) else {
            return;
        };
        let im = avif::create_from_avif(&data).unwrap();
        assert_eq!((im.sx, im.sy), (w, h));
    }
}

#[test]
fn avif_errors() {
    take_errors();
    assert!(avif::create_from_avif(b"not an avif file at all").is_none());
    assert_eq!(take_errors().len(), 1);
    let im = Image::create(2, 2).unwrap();
    assert!(avif::image_avif(&im, -1, -1).is_none());
    assert_eq!(
        take_errors()[0].1,
        "avif error - avif doesn't support palette images"
    );
}
