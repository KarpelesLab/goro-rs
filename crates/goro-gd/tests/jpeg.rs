//! Self-contained JPEG checks: crc32 of the files and of the decoded
//! pixels produced by php 8.3 (system libgd + libjpeg-turbo 3.1) for a
//! fixed pattern. php 8.3's libgd uses 4:4:4 sampling for quality >= 90,
//! which the bundled libgd does not; those cases use image_jpeg_ex().

use goro_gd::*;

fn pattern() -> Image {
    let (w, h) = (37, 23);
    let mut im = Image::create_true_color(w, h).unwrap();
    for y in 0..h {
        for x in 0..w {
            let c = (((x * 7 + y * 13) & 255) << 16)
                | (((x * x + y) & 255) << 8)
                | (((x ^ y) * 5) & 255);
            im.set_pixel(x, y, c);
        }
    }
    im
}

#[test]
fn jpeg_matches_libjpeg_turbo() {
    // (quality, interlace, length, crc32 of the file, crc32 of the pixels)
    let cases: [(i32, i32, usize, u32, u32); 7] = [
        (-1, 0, 1094, 0xb1dae88d, 0xd830275d),
        (50, 0, 966, 0x06e0fe03, 0x51d25aaa),
        (50, 1, 875, 0xdd3e8f47, 0x51d25aaa),
        (95, 0, 1890, 0xcaf78f1e, 0xeed71cc1),
        (95, 1, 1702, 0xad563267, 0xeed71cc1),
        (0, 0, 740, 0xa41196d5, 0x190cc4dc),
        (100, 1, 2516, 0x99f79d87, 0x5bf51273),
    ];
    let mut im = pattern();
    for (q, il, len, crc, pxcrc) in cases {
        im.interlace = il;
        let d = jpeg::image_jpeg_ex(&im, q, q >= 90).unwrap();
        assert_eq!(
            (d.len(), goro_zlib::crc32(0, &d)),
            (len, crc),
            "q={} il={}",
            q,
            il
        );
        let dec = jpeg::create_from_jpeg(&d, true).unwrap();
        let mut px = Vec::new();
        for y in 0..dec.sy {
            for x in 0..dec.sx {
                px.extend_from_slice(&(dec.get_pixel(x, y) as u32).to_be_bytes());
            }
        }
        assert_eq!(goro_zlib::crc32(0, &px), pxcrc, "decode q={} il={}", q, il);
    }
}

#[test]
fn jpeg_header_and_comment() {
    let mut im = Image::create(10, 7).unwrap();
    im.color_allocate(255, 0, 0);
    im.res_x = 71;
    im.res_y = 299;
    let d = jpeg::image_jpeg(&im, -1).unwrap();
    // JFIF APP0 with the density, then the gd COM marker
    assert_eq!(&d[..6], &[0xff, 0xd8, 0xff, 0xe0, 0, 16]);
    assert_eq!(&d[13..18], &[1, 0, 71, 1, 43]);
    let com = b"CREATOR: gd-jpeg v1.0 (using IJG JPEG v62), default quality\n";
    assert_eq!(&d[20..22], &[0xff, 0xfe]);
    assert_eq!(&d[24..24 + com.len()], com);
    let back = jpeg::create_from_jpeg(&d, true).unwrap();
    assert_eq!((back.sx, back.sy, back.res_x, back.res_y), (10, 7, 71, 299));
    assert!(back.true_color);
    let q = jpeg::image_jpeg(&im, 80).unwrap();
    let com = b"CREATOR: gd-jpeg v1.0 (using IJG JPEG v62), quality = 80\n";
    assert_eq!(&q[24..24 + com.len()], com);
}

#[test]
fn jpeg_errors() {
    let _ = take_errors();
    assert!(jpeg::create_from_jpeg(b"", true).is_none());
    assert!(jpeg::create_from_jpeg(b"GIF89a....", true).is_none());
    assert!(jpeg::create_from_jpeg(&[0xff, 0xd8, 0xff, 0xd9], true).is_none());
    let e: Vec<String> = take_errors().into_iter().map(|(_, m)| m).collect();
    assert_eq!(
        e,
        [
            "gd-jpeg: JPEG library reports unrecoverable error: Empty input file",
            "gd-jpeg: JPEG library reports unrecoverable error: Not a JPEG file: starts with 0x47 0x49",
            "gd-jpeg: JPEG library reports unrecoverable error: JPEG datastream contains no image",
        ]
    );
    // a truncated file decodes, with a warning unless ignored
    let d = jpeg::image_jpeg(&pattern(), 75).unwrap();
    let t = &d[..d.len() - 40];
    assert!(jpeg::create_from_jpeg(t, true).is_some());
    assert!(take_errors().is_empty());
    assert!(jpeg::create_from_jpeg(t, false).is_none());
    assert_eq!(
        take_errors()[0].1,
        "gd-jpeg, libjpeg: recoverable error: Premature end of JPEG file\n"
    );
}

/// php-src's ext/gd/tests/bug77195.jpg, when available
#[test]
fn jpeg_bug77195() {
    let Ok(d) = std::fs::read("/home/magicaltux/php-src/ext/gd/tests/bug77195.jpg") else {
        return;
    };
    let _ = take_errors();
    assert!(jpeg::create_from_jpeg(&d, true).is_none());
    assert_eq!(
        take_errors()[0].1,
        "gd-jpeg: JPEG library reports unrecoverable error: JPEG datastream contains no image"
    );
}

/// Corrupted and truncated input must never panic
#[test]
fn jpeg_fuzz_no_panic() {
    let mut im = pattern();
    let mut seed: u64 = 0x1234_5678;
    let mut rnd = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    for il in [0, 1] {
        im.interlace = il;
        let d = jpeg::image_jpeg(&im, 75).unwrap();
        for _ in 0..1500 {
            let mut e = d.clone();
            for _ in 0..1 + rnd() % 8 {
                let p = rnd() % e.len();
                e[p] = rnd() as u8;
            }
            if rnd() % 4 == 0 {
                e.truncate(rnd() % e.len());
            }
            let _ = jpeg::create_from_jpeg(&e, rnd() % 2 == 0);
        }
    }
    let _ = take_errors();
}
