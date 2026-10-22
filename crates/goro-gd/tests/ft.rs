//! The FreeType text phpt cases of php-src's ext/gd/tests, at libgd level.
use goro_gd::ft::*;
use goro_gd::*;

const DIR: &str = "/home/magicaltux/php-src/ext/gd/tests";

fn font(name: &str) -> Option<String> {
    let p = format!("{}/{}", DIR, name);
    std::path::Path::new(&p).exists().then_some(p)
}

fn rad(a: f64) -> f64 {
    a * (std::f64::consts::PI / 180.0)
}

fn bbox(size: f64, angle: f64, font: &str, text: &str) -> [i32; 8] {
    let mut b = [0; 8];
    image_string_ft_ex(
        None,
        &mut b,
        0,
        font,
        size,
        rad(angle),
        0,
        0,
        text.as_bytes(),
        None,
    )
    .unwrap();
    b
}

#[test]
fn bug53504_bboxes() {
    let Some(f) = font("Tuffy.ttf") else { return };
    let tests: &[(f64, f64, &str, [i32; 8])] = &[
        (50.0, 0.0, "AV Teg", [2, 15, 208, 15, 208, -48, 2, -48]),
        (50.0, 90.0, "AV Teg", [15, -1, 15, -208, -48, -208, -48, -2]),
        (
            50.0,
            40.0,
            "AV Teg",
            [11, 11, 169, -122, 129, -171, -30, -39],
        ),
        (100.0, 0.0, "H-Shift", [8, 2, 386, 2, 386, -97, 8, -97]),
        (100.0, 0.0, "-", [7, -37, 51, -37, 51, -46, 7, -46]),
        (100.0, 0.0, ",", [7, 15, 21, 15, 21, -13, 7, -13]),
        (100.0, 0.0, ".", [7, 1, 21, 1, 21, -13, 7, -13]),
        (100.0, 0.0, "|", [8, 0, 17, 0, 17, -95, 8, -95]),
        (100.0, 0.0, "g", [5, 29, 60, 29, 60, -72, 5, -72]),
        (
            30.0,
            0.0,
            "Multi\nLine\nTest",
            [2, 107, 80, 107, 80, -29, 2, -29],
        ),
        (
            30.0,
            40.0,
            "Multi\nLine\nTest",
            [70, 81, 131, 31, 43, -74, -18, -24],
        ),
        (
            30.0,
            90.0,
            "Multi\nLine\nTest",
            [107, -1, 107, -80, -29, -80, -29, -2],
        ),
        (
            50.0,
            90.0,
            "iiiiiiiiiiii",
            [0, -4, 0, -165, -47, -165, -47, -4],
        ),
        (
            50.0,
            90.0,
            "~~~~~~~",
            [-19, -2, -18, -167, -29, -167, -29, -2],
        ),
        (
            50.0,
            50.0,
            "iiiiiiiiiiii",
            [3, -3, 107, -127, 70, -157, -34, -33],
        ),
        (
            50.0,
            50.0,
            "~~~~~~~",
            [-13, -13, 93, -141, 85, -147, -21, -20],
        ),
        (50.0, 0.0, "iiiiiiiiiiii", [4, 0, 165, 0, 165, -47, 4, -47]),
        (50.0, 0.0, "~~~~~~~", [2, -19, 167, -19, 167, -29, 2, -29]),
        (200.0, 0.0, "Big", [16, 59, 329, 59, 329, -190, 16, -190]),
    ];
    for (n, (s, a, t, exp)) in tests.iter().enumerate() {
        let b = bbox(*s, *a, &f, t);
        for i in 0..8 {
            assert!(
                (b[i] - exp[i]).abs() <= 2,
                "test {}: {:?} vs {:?}",
                n,
                b,
                exp
            );
        }
        // imagefttext() at an offset gives the same box, shifted
        let mut im = Image::create(800, 800).unwrap();
        im.color_allocate(255, 255, 255);
        let black = im.color_allocate(0, 0, 0);
        let mut d = [0; 8];
        image_string_ft_ex(
            Some(&mut im),
            &mut d,
            black,
            &f,
            *s,
            rad(*a),
            20,
            70,
            t.as_bytes(),
            None,
        )
        .unwrap();
        for i in (0..8).step_by(2) {
            assert_eq!(b[i] + 20, d[i]);
            assert_eq!(b[i + 1] + 70, d[i + 1]);
        }
    }
}

#[test]
fn bug43073_rotations() {
    let Some(f) = font("Tuffy.ttf") else { return };
    let exp = [
        [501, 400, 611, 400, 611, 376, 501, 376],
        [492, 361, 595, 319, 586, 296, 483, 338],
        [470, 329, 549, 251, 531, 233, 453, 312],
        [439, 307, 481, 204, 458, 195, 416, 297],
        [400, 299, 400, 189, 376, 189, 376, 299],
        [361, 307, 319, 204, 296, 213, 338, 316],
        [329, 329, 251, 250, 233, 267, 311, 346],
        [307, 360, 204, 318, 195, 341, 297, 383],
        [299, 400, 189, 400, 189, 424, 299, 424],
        [307, 438, 204, 480, 213, 503, 316, 461],
        [329, 470, 250, 548, 267, 566, 346, 488],
        [360, 492, 318, 595, 341, 604, 383, 502],
        [400, 501, 400, 611, 424, 611, 424, 501],
        [438, 492, 480, 595, 503, 586, 461, 483],
        [470, 470, 548, 549, 566, 532, 488, 453],
        [492, 439, 595, 481, 604, 458, 502, 416],
    ];
    let mut g = Image::create(800, 800).unwrap();
    g.color_allocate(255, 255, 255);
    let black = g.color_allocate(0, 0, 0);
    let delta: f64 = 360.0 / 16.0;
    let (cos_t, sin_t) = (delta.to_radians().cos(), delta.to_radians().sin());
    let (mut x, mut y) = (100.0f64, 0.0f64);
    for (i, e) in exp.iter().enumerate() {
        let angle = (i as f64 * delta) as i32 as f64;
        let mut b = [0; 8];
        let (px, py) = ((400.0 + x) as i32, (400.0 + y) as i32);
        image_string_ft_ex(
            Some(&mut g),
            &mut b,
            black,
            &f,
            24.0,
            rad(angle),
            px,
            py,
            b"ABCDEF",
            None,
        )
        .unwrap();
        for j in 0..8 {
            assert!((b[j] - e[j]).abs() <= 1, "{}: {:?} vs {:?}", i, b, e);
        }
        let t = cos_t * x + sin_t * y;
        y = cos_t * y - sin_t * x;
        x = t;
    }
}

#[test]
fn bug48801_bug48732_bug48555() {
    let Some(f) = font("Tuffy.ttf") else { return };
    let b = bbox(50.0, 0.0, &f, "image");
    assert_eq!((b[0], b[1]), (4, 15));
    assert!((160..=161).contains(&b[2]) && b[3] == 15);
    assert!((160..=161).contains(&b[4]) && (-48..=-47).contains(&b[5]));
    assert!(b[6] == 4 && (-48..=-47).contains(&b[7]));

    let mut g = Image::create(100, 50).unwrap();
    g.color_allocate(255, 255, 255);
    let black = g.color_allocate(0, 0, 0);
    let mut b = [0; 8];
    image_string_ft_ex(
        Some(&mut g),
        &mut b,
        black,
        &f,
        12.0,
        0.0,
        0,
        20,
        b"ABCEDFGHIJKLMN\nopqrstu\n",
        None,
    )
    .unwrap();
    assert_eq!((b[0], b[1]), (0, 46));

    let a = bbox(14.0, 0.0, &f, "Text without line-break");
    let c = bbox(14.0, 0.0, &f, "Text with\nline-break\none more");
    assert!(a[7] < 0);
    assert_eq!(a[7], c[7]);
}

#[test]
fn bug49600_text_vs_bbox() {
    let Some(f) = font("Tuffy.ttf") else { return };
    let mut im = Image::create_true_color(50, 50).unwrap();
    let color = im.color_allocate(255, 255, 255);
    for c in ["E", "I", "P", "g", "i", "q"] {
        let mut x = [0; 8];
        image_string_ft_ex(
            Some(&mut im),
            &mut x,
            color,
            &f,
            32.0,
            0.0,
            0,
            0,
            c.as_bytes(),
            None,
        )
        .unwrap();
        let y = bbox(32.0, 0.0, &f, c);
        for i in [0, 2, 4, 6] {
            assert!((x[i] - y[i]).abs() <= 1);
        }
    }
}

#[test]
fn bug51263_rotated_baseline() {
    let Some(f) = font("Tuffy.ttf") else { return };
    let mut img = Image::create_true_color(23, 70).unwrap();
    let blk = img.color_allocate(0, 0, 0);
    img.filled_rectangle(0, 0, 22, 69, blk);
    let col = img.color_allocate(255, 255, 255);
    let mut b = [0; 8];
    image_string_ft_ex(
        Some(&mut img),
        &mut b,
        col,
        &f,
        8.0,
        rad(90.0),
        10,
        60,
        b"foo bar qux",
        None,
    )
    .unwrap();
    for j in 0..30 {
        for i in [0, 1, 2, 3, 13] {
            assert_eq!(img.get_pixel(i, j), 0, "({}, {})", i, j);
        }
    }
}

#[test]
fn gh17373_palette_clip() {
    let Some(f) = font("Tuffy.ttf") else { return };
    let mut im = Image::create(64, 32).unwrap();
    let bg = im.color_allocate(0, 0, 0);
    let fg = im.color_allocate(255, 255, 255);
    im.filled_rectangle(0, 0, 63, 31, bg);
    im.set_clip(32, 0, 63, 31);
    let mut b = [0; 8];
    image_string_ft_ex(
        Some(&mut im),
        &mut b,
        fg,
        &f,
        16.0,
        0.0,
        10,
        23,
        b"hello",
        None,
    )
    .unwrap();
    im.set_clip(0, 0, 63, 31);
    for j in 0..31 {
        for i in 0..31 {
            assert_eq!(im.get_pixel(i, j), bg);
        }
    }
}

#[test]
fn imagefttext_draws_something() {
    let Some(f) = font("test8859.ttf") else {
        return;
    };
    for tc in [false, true] {
        let mut im = if tc {
            Image::create_true_color(256, 256).unwrap()
        } else {
            Image::create(256, 256).unwrap()
        };
        let w = im.color_allocate(255, 255, 255);
        let b = im.color_allocate(0, 0, 0);
        im.filled_rectangle(0, 0, 255, 255, b);
        let mut r = [0; 8];
        image_string_ft_ex(
            Some(&mut im),
            &mut r,
            w,
            &f,
            256.0 * 0.75,
            0.0,
            0,
            255,
            b"A",
            None,
        )
        .unwrap();
        let mut cnt = 0;
        for y in 0..256 {
            for x in 0..256 {
                if im.get_pixel(x, y) == b {
                    cnt += 1;
                }
            }
        }
        assert!(cnt < 256 * 256);
    }
}

#[test]
fn charmap_order_otf() {
    let Some(f) = font("Rochester-Regular.otf") else {
        return;
    };
    let mut im = Image::create_true_color(100, 80).unwrap();
    let white = im.color_allocate(255, 255, 255);
    let black = im.color_allocate(0, 0, 0);
    im.filled_rectangle(0, 0, 100, 80, white);
    let mut b = [0; 8];
    image_string_ft_ex(
        Some(&mut im),
        &mut b,
        black,
        &f,
        45.0,
        0.0,
        15,
        60,
        b"\xC3\x9E\xC3\xB6",
        None,
    )
    .unwrap();
    let mut n = 0;
    for y in 0..80 {
        for x in 0..100 {
            if im.get_pixel(x, y) == 0 {
                n += 1;
            }
        }
    }
    assert!(n >= 10, "{} black pixels", n);
}

#[test]
fn bug50194_similar() {
    let (Some(f), Ok(png)) = (
        font("Tuffy.ttf"),
        std::fs::read(format!("{}/bug50194.png", DIR)),
    ) else {
        return;
    };
    let mut im = Image::create_true_color(240, 55).unwrap();
    let background = im.color_allocate_alpha(60, 60, 60, 0);
    let black = im.color_allocate(0, 0, 0);
    im.set_alpha_blending(0);
    im.filled_rectangle(0, 0, 239, 54, background);
    let mut b = [0; 8];
    image_string_ft_ex(
        Some(&mut im),
        &mut b,
        black,
        &f,
        40.0,
        0.0,
        10,
        40,
        b"Testing ... ",
        None,
    )
    .unwrap();
    let exp = png::create_from_png(&png).unwrap();
    let mut diff = 0;
    for y in 0..55 {
        for x in 0..240 {
            if exp.get_pixel(x, y) != im.get_pixel(x, y) {
                diff += 1;
            }
        }
    }
    assert!(diff <= 2000, "{} pixels differ", diff);
}

#[test]
fn errors_and_entities() {
    let mut b = [0; 8];
    assert_eq!(
        image_string_ft_ex(
            None,
            &mut b,
            0,
            "/nonexistent/font.ttf",
            10.0,
            0.0,
            0,
            0,
            b"x",
            None
        ),
        Err("Could not find/open font".to_string())
    );
    let Some(f) = font("Tuffy.ttf") else { return };
    // gh19955: size 0 is clamped to 1pt by FT_Set_Char_Size()
    let mut im = Image::create(8, 8).unwrap();
    image_string_ft_ex(
        Some(&mut im),
        &mut b,
        255,
        &f,
        0.0,
        0.0,
        0,
        0,
        "Ж".as_bytes(),
        None,
    )
    .unwrap();
    // &#65;&#x42; are "AB"
    assert_eq!(
        bbox(20.0, 0.0, &f, "&#65;&#x42;C"),
        bbox(20.0, 0.0, &f, "ABC")
    );
}
