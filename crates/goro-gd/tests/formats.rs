//! GIF/WBMP/XBM/XPM/BMP/TGA/GD/GD2 codecs, following the expectations of
//! php-src's ext/gd/tests (the tests that need the php-src test images are
//! skipped when they are not available).

use goro_gd::*;

const T: &str = "/home/magicaltux/php-src/ext/gd/tests";

fn file(name: &str) -> Option<Vec<u8>> {
    std::fs::read(format!("{}/{}", T, name)).ok()
}

fn errors() -> Vec<String> {
    take_errors().into_iter().map(|(_, m)| m).collect()
}

fn same_pixels(a: &Image, b: &Image) -> usize {
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

#[test]
fn gd2_sizes_and_header() {
    // bug73155
    let mut im = Image::create(64, 64).unwrap();
    im.color_allocate(0, 0, 0);
    let d = gd2::image_gd2(&im, 64, GD2_FMT_RAW).unwrap();
    assert_eq!(d.len(), 5145);
    assert_eq!(&d[10..16], &[0, 64, 0, 1, 0, 1]);
    // bug73157
    let mut im = Image::create(8, 8).unwrap();
    im.color_allocate(0, 0, 0);
    let d = gd2::image_gd2(&im, 256, GD2_FMT_RAW).unwrap();
    assert_eq!(u16::from_be_bytes([d[10], d[11]]), 256);
    // bug73159
    let im = Image::create_true_color(10, 10).unwrap();
    let d = gd2::image_gd2(&im, 128, 0).unwrap();
    assert_eq!(u16::from_be_bytes([d[12], d[13]]), 4);
}

#[test]
fn gd_truecolor_size() {
    // imagegd_truecolor
    let mut im = Image::create_true_color(10, 10).unwrap();
    let white = im.color_allocate(255, 255, 255);
    im.filled_rectangle(0, 0, 9, 9, white);
    let blue = im.color_allocate(0, 0, 255);
    im.filled_rectangle(3, 3, 6, 6, blue);
    let d = gd_gd::image_gd(&im);
    assert_eq!(u16::from_be_bytes([d[0], d[1]]), 65534);
    assert_eq!(d[6], 1);
    assert_eq!(d.len(), 411);
    let back = gd_gd::create_from_gd(&d).unwrap();
    assert_eq!(same_pixels(&im, &back), 0);
}

fn sample(tc: bool) -> Image {
    let mut im = if tc {
        Image::create_true_color(150, 133).unwrap()
    } else {
        Image::create(150, 133).unwrap()
    };
    for i in 0..40 {
        im.color_allocate_alpha(i * 6, 255 - i * 6, (i * 37) & 255, i % 3);
    }
    for y in 0..im.sy {
        for x in 0..im.sx {
            let c = if tc {
                true_color_alpha(x & 255, y & 255, (x * y) & 255, (x + y) % 128)
            } else {
                (x / 4 + y / 7) % 40
            };
            if tc {
                im.tpixels[y as usize][x as usize] = c;
            } else {
                im.pixels[y as usize][x as usize] = c as u8;
            }
        }
    }
    im.transparent = 3;
    im
}

#[test]
fn gd_gd2_round_trips() {
    for tc in [false, true] {
        let im = sample(tc);
        let back = gd_gd::create_from_gd(&gd_gd::image_gd(&im)).unwrap();
        assert_eq!(same_pixels(&im, &back), 0);
        assert_eq!(back.transparent, 3);
        for (cs, fmt) in [
            (64, GD2_FMT_RAW),
            (64, GD2_FMT_COMPRESSED),
            (100, GD2_FMT_COMPRESSED),
            (0, 7),
        ] {
            let d = gd2::image_gd2(&im, cs, fmt).unwrap();
            let back = gd2::create_from_gd2(&d).unwrap();
            assert_eq!(back.true_color, tc);
            assert_eq!(
                same_pixels(&im, &back),
                0,
                "tc {} cs {} fmt {}",
                tc,
                cs,
                fmt
            );
            let part = gd2::create_from_gd2_part(&d, 70, 60, 50, 90).unwrap();
            for y in 0..90 {
                for x in 0..50 {
                    let exp = if 70 + x < 150 && 60 + y < 133 {
                        im.get_pixel(70 + x, 60 + y)
                    } else {
                        0
                    };
                    assert_eq!(part.get_pixel(x, y), exp);
                }
            }
        }
    }
    assert!(errors().is_empty());
}

#[test]
fn gd2_test_files() {
    let Some(src) = file("src.gd2") else {
        return;
    };
    let im = gd2::create_from_gd2(&src).unwrap();
    assert_eq!(im.get_true_color_pixel(4, 4) & 0xffffff, 0xffffff);
    assert!(gd2::create_from_gd2_part(&src, 4, 4, 2, 2).is_some());
    for f in [
        "invalid_neg_size.gd2",
        "bug73161.gd2",
        "bug73869a.gd2",
        "bug73869b.gd2",
    ] {
        assert!(gd2::create_from_gd2(&file(f).unwrap()).is_none(), "{}", f);
    }
    let _ = errors();
    assert!(gd2::create_from_gd2(&file("bug73868.gd2").unwrap()).is_none());
    assert_eq!(errors(), vec!["gd2: EOF while reading\n".to_string()]);
    // crafted.gd2 read as .gd
    assert!(gd_gd::create_from_gd(&file("crafted.gd2").unwrap()).is_none());
    assert!(gd_gd::create_from_gd(&file("libgd00101.gd").unwrap()).is_none());
    assert_eq!(
        errors(),
        vec!["Product of memory allocation multiplication would exceed INT_MAX, failing operation gracefully\n".to_string()]
    );
    // bug72339
    let mut d = b"gd2\0".to_vec();
    for v in [2u16, 1, 1, 0x40, 2, 0x5AA0, 0x5B00] {
        d.extend_from_slice(&v.to_be_bytes());
    }
    d.extend_from_slice(&[0x41; 4096]);
    assert!(gd2::create_from_gd2(&d).is_none());
    assert_eq!(
        errors(),
        vec!["Product of memory allocation multiplication would exceed INT_MAX, failing operation gracefully\n".to_string()]
    );
}

#[test]
fn gif_round_trip_and_files() {
    let mut im = Image::create(3, 3).unwrap();
    for x in 0..3 {
        for y in 0..3 {
            let c = im.color_allocate(x, y, 0);
            im.set_pixel(x, y, c);
        }
    }
    // bug79615
    let d = gif_out::image_gif(&im).unwrap();
    assert_eq!(d[0xA], 0b10110011);
    let back = gif_in::create_from_gif(&d).unwrap();
    assert_eq!(same_pixels(&im, &back), 0);
    // interlace flag survives
    let mut im = sample(false);
    im.interlace = 1;
    let back = gif_in::create_from_gif(&gif_out::image_gif(&im).unwrap()).unwrap();
    assert_eq!(back.interlace, 1);
    assert_eq!(back.transparent, 3);
    for y in 0..im.sy {
        for x in 0..im.sx {
            assert_eq!(back.get_pixel(x, y), im.get_pixel(x, y));
        }
    }
    let Some(_) = file("php.gif") else {
        return;
    };
    for f in [
        "bug38112.gif",
        "bug37360.gif",
        "bug37346.gif",
        "bug75571.gif",
    ] {
        assert!(
            gif_in::create_from_gif(&file(f).unwrap()).is_none(),
            "{}",
            f
        );
    }
    for f in [
        "bug74435.gif",
        "php.gif",
        "conv_test.gif",
        "bug43121.gif",
        "gif_nocolormaps.gif",
    ] {
        assert!(
            gif_in::create_from_gif(&file(f).unwrap()).is_some(),
            "{}",
            f
        );
    }
}

#[test]
fn xbm_writer_and_reader() {
    // bug53640 / bug66339
    let mut im = Image::create(9, 9).unwrap();
    im.color_allocate(0, 0, 0);
    let white = im.color_allocate(255, 255, 255);
    im.filled_rectangle(2, 2, 6, 6, white);
    let fg = 0; // imagexbm() with a null foreground: the first black-ish color
    let s = String::from_utf8(xbm::image_xbm(&im, b"", fg)).unwrap();
    assert_eq!(
        s,
        "#define image_width 9\n#define image_height 9\nstatic unsigned char image_bits[] = {\n  0xFF, 0x01, 0xFF, 0x01, 0x83, 0x01, 0x83, 0x01, 0x83, 0x01, 0x83, 0x01, \n  0x83, 0x01, 0xFF, 0x01, 0xFF, 0x01};\n"
    );
    let mut im = Image::create(8, 8).unwrap();
    im.color_allocate(0, 0, 0);
    let white = im.color_allocate(255, 255, 255);
    im.filled_rectangle(2, 2, 6, 6, white);
    let s = String::from_utf8(xbm::image_xbm(&im, b"./bug66339.xbm", 0)).unwrap();
    assert_eq!(
        s,
        "#define bug66339_width 8\n#define bug66339_height 8\nstatic unsigned char bug66339_bits[] = {\n  0xFF, 0xFF, 0x83, 0x83, 0x83, 0x83, 0x83, 0xFF};\n"
    );
    let back = xbm::create_from_xbm(s.as_bytes()).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                back.get_pixel(x, y),
                if im.get_pixel(x, y) == 0 { 1 } else { 0 }
            );
        }
    }
    // bug77973
    let _ = errors();
    let d = b"#define width 2\n#define height 2\nstatic char bits[] ={\nzzxzz";
    assert!(xbm::create_from_xbm(d).is_none());
    assert_eq!(errors(), vec!["Invalid XBM".to_string()]);
    if let Some(d) = file("bug73968.xbm") {
        assert!(xbm::create_from_xbm(&d).is_some());
    }
    if let Some(d) = file("libgd00094.xbm") {
        assert!(xbm::create_from_xbm(&d).is_none());
        assert!(errors().is_empty());
    }
}

#[test]
fn wbmp_round_trip() {
    let im = sample(false);
    let d = wbmp::image_wbmp(&im, 5).unwrap();
    let back = wbmp::create_from_wbmp(&d).unwrap();
    for y in 0..im.sy {
        for x in 0..im.sx {
            assert_eq!(
                back.get_pixel(x, y),
                if im.get_pixel(x, y) == 5 { 1 } else { 0 }
            );
        }
    }
    // bug77479
    let _ = errors();
    let big = Image {
        sx: 40000,
        sy: 20000,
        ..Image::create(1, 1).unwrap()
    };
    assert!(wbmp::image_wbmp(&big, 0).is_none());
    assert_eq!(
        errors(),
        vec![
            "Product of memory allocation multiplication would exceed INT_MAX, failing operation gracefully\n".to_string(),
            "Could not create WBMP".to_string()
        ]
    );
    if let Some(d) = file("src.wbmp") {
        assert!(wbmp::create_from_wbmp(&d).is_some());
    }
}

#[test]
fn bmp_round_trips() {
    // gd276
    let mut orig = Image::create(10, 10).unwrap();
    orig.color_allocate(0, 0, 0);
    let white = orig.color_allocate(255, 255, 255);
    orig.line(0, 0, 99, 99, white);
    let saved = bmp::create_from_bmp(&bmp::image_bmp(&orig, 1).unwrap()).unwrap();
    assert_eq!(same_pixels(&orig, &saved), 0);
    for tc in [false, true] {
        let im = sample(tc);
        for comp in [0, 1] {
            let back = bmp::create_from_bmp(&bmp::image_bmp(&im, comp).unwrap()).unwrap();
            for y in 0..im.sy {
                for x in 0..im.sx {
                    let a = im.get_true_color_pixel(x, y) & 0xffffff;
                    assert_eq!(back.get_true_color_pixel(x, y) & 0xffffff, a);
                }
            }
        }
    }
    // bug75111
    let mut s = b"BM6\x03\0\0\0\0\0\0".to_vec();
    s.extend_from_slice(&(-0x120000i32).to_le_bytes());
    s.extend_from_slice(&40i32.to_le_bytes());
    s.extend_from_slice(&256i32.to_le_bytes());
    s.extend_from_slice(&256i32.to_le_bytes());
    s.extend_from_slice(&[1, 0, 0x18, 0]);
    s.extend_from_slice(&[0; 24]);
    assert!(bmp::create_from_bmp(&s).is_none());
}

#[test]
fn files_match_their_png() {
    let pairs = [
        (
            "imagecreatefrombmp_basic.bmp",
            "imagecreatefrombmp_basic.png",
        ),
        ("imagecreatefromstring.bmp", "imagecreatefromstring_bmp.png"),
        ("imagecreatefromtga_basic.tga", "imagecreatefromtga.png"),
        ("imagecreatefromtga_variation.tga", "imagecreatefromtga.png"),
    ];
    for (a, b) in pairs {
        let (Some(da), Some(db)) = (file(a), file(b)) else {
            return;
        };
        let mut ia = if a.ends_with(".bmp") {
            bmp::create_from_bmp(&da).unwrap()
        } else {
            tga::create_from_tga(&da).unwrap()
        };
        ia.palette_to_true_color();
        let mut ib = png::create_from_png(&db).unwrap();
        ib.palette_to_true_color();
        assert_eq!(same_pixels(&ia, &ib), 0, "{}", a);
    }
    assert!(bmp::create_from_bmp(&file("bug77391.bmp").unwrap()).is_some());
}

#[test]
fn xpm_test_file() {
    let Some(d) = file("conv_test.xpm") else {
        return;
    };
    let im = xpm::create_from_xpm(&d).unwrap();
    assert_eq!((im.sx, im.sy, im.true_color), (50, 50, false));
    assert_eq!(im.colors_total, 256);
    let xpm = b"/* XPM */\nstatic char *x[] = {\n\"2 2 2 1\",\n\"a c #FF0000\",\n\"b c None\",\n\"ab\",\n\"ba\"};\n";
    let im = xpm::create_from_xpm(xpm).unwrap();
    assert_eq!(im.get_true_color_pixel(0, 0), 0xff0000);
    // "None" goes through strtol() of "o", "n", "e"
    assert_eq!(im.get_true_color_pixel(1, 0), 0x00000e);
    assert!(
        xpm::create_from_xpm(b"/* XPM */\n\"2 2 1 1\",\n\"a c #FF0000\",\n\"ab\",\n\"aa\"")
            .is_none()
    );
}
