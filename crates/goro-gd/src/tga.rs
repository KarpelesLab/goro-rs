//! gd_tga.c: TGA reading (24-bit RGB and 32-bit RGBA, raw or RLE).

use crate::io::In;
use crate::*;

const TGA_TYPE_RGB: u8 = 2;
const TGA_TYPE_RGB_RLE: u8 = 10;
const TGA_BPP_24: u8 = 24;
const TGA_BPP_32: u8 = 32;
const TGA_RLE_FLAG: u8 = 128;

struct Tga {
    identsize: u8,
    imagetype: u8,
    width: i32,
    height: i32,
    bits: u8,
    alphabits: u8,
    fliph: bool,
    flipv: bool,
    bitmap: Vec<u8>,
}

/// gdImageCreateFromTgaCtx()
pub fn create_from_tga(data: &[u8]) -> Option<Image> {
    let mut inp = In::new(data);
    let mut tga = read_header(&mut inp)?;
    read_image(&mut inp, &mut tga)?;
    let mut image = Image::create_true_color(tga.width, tga.height)?;
    if tga.alphabits != 0 {
        image.set_alpha_blending(0);
        image.set_save_alpha(1);
    }
    let bm = &tga.bitmap;
    let mut c = 0usize;
    for y in 0..tga.height as usize {
        for x in 0..tga.width as usize {
            if tga.bits == TGA_BPP_24 {
                image.tpixels[y][x] = true_color(bm[c + 2] as i32, bm[c + 1] as i32, bm[c] as i32);
                c += 3;
            } else if tga.bits == TGA_BPP_32 && tga.alphabits != 0 {
                let a = bm[c + 3] as i32;
                image.tpixels[y][x] = true_color_alpha(
                    bm[c + 2] as i32,
                    bm[c + 1] as i32,
                    bm[c] as i32,
                    ALPHA_MAX - (a >> 1),
                );
                c += 4;
            }
        }
    }
    if tga.flipv {
        image.tpixels.reverse();
    }
    if tga.fliph {
        for row in image.tpixels.iter_mut() {
            row.reverse();
        }
    }
    Some(image)
}

/// read_header_tga()
fn read_header(inp: &mut In) -> Option<Tga> {
    let mut header = [0u8; 18];
    if inp.get_buf(&mut header) < 18 {
        gd_error("Fail to read header");
        return None;
    }
    let tga = Tga {
        identsize: header[0],
        imagetype: header[2],
        width: header[12] as i32 + ((header[13] as i32) << 8),
        height: header[14] as i32 + ((header[15] as i32) << 8),
        bits: header[16],
        alphabits: header[17] & 0x0f,
        fliph: header[17] & 0x10 != 0,
        flipv: header[17] & 0x20 == 0,
        bitmap: Vec::new(),
    };
    if !((tga.bits == TGA_BPP_24 && tga.alphabits == 0)
        || (tga.bits == TGA_BPP_32 && tga.alphabits == 8))
    {
        gd_error_ex(
            GD_WARNING,
            format!(
                "gd-tga: {} bits per pixel with {} alpha bits not supported\n",
                tga.bits, tga.alphabits
            ),
        );
        return None;
    }
    if tga.identsize > 0 {
        let mut ident = vec![0u8; tga.identsize as usize];
        if inp.get_buf(&mut ident) != tga.identsize as i32 {
            gd_error("fail to read header ident");
            return None;
        }
    }
    Some(tga)
}

/// read_image_tga()
fn read_image(inp: &mut In, tga: &mut Tga) -> Option<()> {
    let pbs = (tga.bits / 8) as i32;
    if overflow2(tga.width, tga.height) {
        return None;
    }
    if overflow2(tga.width * tga.height, pbs) {
        return None;
    }
    let ibs = tga.width * tga.height * pbs;
    if overflow2(ibs, 4) {
        return None;
    }
    if tga.imagetype != TGA_TYPE_RGB && tga.imagetype != TGA_TYPE_RGB_RLE {
        return None;
    }
    let ibs = ibs as usize;
    let pbs = pbs as usize;
    tga.bitmap = vec![0u8; ibs];
    if tga.imagetype == TGA_TYPE_RGB {
        let mut buf = vec![0u8; ibs];
        if inp.get_buf(&mut buf) != ibs as i32 {
            gd_error("gd-tga: premature end of image data\n");
            return None;
        }
        tga.bitmap = buf;
    } else {
        let mut buf = vec![0u8; ibs];
        let rle_size = inp.get_buf(&mut buf);
        if rle_size <= 0 {
            return None;
        }
        let rle_size = rle_size as usize;
        let mut bc = 0usize; // buffer_caret
        let mut mc = 0usize; // bitmap_caret
        while mc < ibs {
            if bc + pbs > rle_size {
                return None;
            }
            if buf[bc] & TGA_RLE_FLAG == TGA_RLE_FLAG {
                let n = (buf[bc] & !TGA_RLE_FLAG) as usize + 1;
                bc += 1;
                if mc + n * pbs > ibs || bc + pbs > rle_size {
                    return None;
                }
                for _ in 0..n {
                    tga.bitmap[mc..mc + pbs].copy_from_slice(&buf[bc..bc + pbs]);
                    mc += pbs;
                }
                bc += pbs;
            } else {
                let n = buf[bc] as usize + 1;
                bc += 1;
                if mc + n * pbs > ibs || bc + n * pbs > rle_size {
                    return None;
                }
                tga.bitmap[mc..mc + n * pbs].copy_from_slice(&buf[bc..bc + n * pbs]);
                mc += n * pbs;
                bc += n * pbs;
            }
        }
    }
    Some(())
}
