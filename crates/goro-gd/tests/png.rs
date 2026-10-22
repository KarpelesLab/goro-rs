use goro_gd::*;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// Bytes produced by php 8.3 (libgd 2.3.3 + libpng 1.6) for the same image
#[test]
fn palette_png_matches_libpng() {
    let mut im = Image::create(10, 10).unwrap();
    im.color_allocate(255, 0, 0);
    let b = im.color_allocate_alpha(0, 0, 255, 60);
    im.line(0, 0, 9, 9, b);
    let d = png::image_png(&im, -1, -1).unwrap();
    assert_eq!(
        hex(&d),
        "89504e470d0a1a0a0000000d494844520000000a0000000a0103000000b7fc5dfe00000006504c54450000ffff0000c5fa8bd30000000174524e5387333acee5000000097048597300000ec400000ec401952b0e1b00000029494441540899011e00e1ff007fc000bfc000dfc000efc000f7c000fbc000fdc000fec000ff4000ff80eb050fb8cde5873d0000000049454e44ae426082"
    );
    let back = png::create_from_png(&d).unwrap();
    assert_eq!(back.get_pixel(3, 3), 0);
    assert_eq!(back.get_pixel(3, 4), 1);
}

#[test]
fn truecolor_png_matches_libpng() {
    let mut im = Image::create_true_color(100, 100).unwrap();
    im.filled_ellipse(50, 50, 80, 60, 0xff8800);
    let d = png::image_png(&im, -1, -1).unwrap();
    assert_eq!(
        hex(&d[..120]),
        "89504e470d0a1a0a0000000d4948445200000064000000640802000000ff800203000000097048597300000ec400000ec401952b0e1b0000018a49444154789ceddc416ec320104661d253f708bd75bbf82b972a52ccc00c30e47deb48414fe3856d7029000000000000000020c263f5024a29e5fbabe967"
    );
    assert_eq!(d.len(), 472);
    let back = png::create_from_png(&d).unwrap();
    for y in 0..100 {
        for x in 0..100 {
            assert_eq!(back.get_pixel(x, y), im.get_pixel(x, y));
        }
    }
}
