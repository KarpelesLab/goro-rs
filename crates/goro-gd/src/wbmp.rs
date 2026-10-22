//! gd_wbmp.c and wbmp.c: WBMP (type 0) reading and writing.

use crate::io::{In, Out};
use crate::*;

const WBMP_WHITE: i32 = 1;
const WBMP_BLACK: i32 = 0;

/// getmbi()
fn getmbi(inp: &mut In) -> i32 {
    let mut mbi: u32 = 0;
    loop {
        let i = inp.get_c();
        if i < 0 {
            return -1;
        }
        mbi = (mbi << 7) | (i as u32 & 0x7f);
        if i & 0x80 == 0 {
            break;
        }
    }
    mbi as i32
}

/// putmbi()
fn putmbi(i: i32, out: &mut Out) {
    let mut cnt: i32 = 0;
    let mut accu: i32 = 0;
    while accu != i {
        accu = accu.wrapping_add(i & 0x7fi32.wrapping_shl((7 * cnt) as u32));
        cnt += 1;
        if cnt > 64 {
            break;
        }
    }
    let mut l = cnt - 1;
    while l > 0 {
        let sh = (7 * l) as u32;
        out.put_c(0x80 | ((i & 0x7fi32.wrapping_shl(sh)) >> (sh & 31)));
        l -= 1;
    }
    out.put_c(i & 0x7f);
}

/// skipheader()
fn skipheader(inp: &mut In) -> bool {
    loop {
        let i = inp.get_c();
        if i < 0 {
            return false;
        }
        if i & 0x80 == 0 {
            return true;
        }
    }
}

/// readwbmp(): (width, height, bitmap)
fn readwbmp(inp: &mut In) -> Option<(i32, i32, Vec<i32>)> {
    let ty = inp.get_c();
    if ty != 0 {
        return None;
    }
    if !skipheader(inp) {
        return None;
    }
    let width = getmbi(inp);
    if width == -1 {
        return None;
    }
    let height = getmbi(inp);
    if height == -1 {
        return None;
    }
    if overflow2(4, width) || overflow2(4 * width, height) {
        return None;
    }
    let mut bitmap = vec![0i32; width as usize * height as usize];
    let mut pos = 0usize;
    for _row in 0..height {
        let mut col = 0;
        while col < width {
            let byte = inp.get_c();
            for pel in (0..8).rev() {
                let c = col;
                col += 1;
                if c < width {
                    bitmap[pos] = if byte & (1 << pel) != 0 {
                        WBMP_WHITE
                    } else {
                        WBMP_BLACK
                    };
                    pos += 1;
                }
            }
        }
    }
    Some((width, height, bitmap))
}

/// gdImageCreateFromWBMPCtx()
pub fn create_from_wbmp(data: &[u8]) -> Option<Image> {
    let mut inp = In::new(data);
    let (width, height, bitmap) = readwbmp(&mut inp)?;
    let mut im = Image::create(width, height)?;
    let white = im.color_allocate(255, 255, 255);
    let black = im.color_allocate(0, 0, 0);
    let mut pos = 0usize;
    for row in 0..height {
        for col in 0..width {
            let c = if bitmap[pos] == WBMP_WHITE {
                white
            } else {
                black
            };
            pos += 1;
            im.set_pixel(col, row, c);
        }
    }
    Some(im)
}

/// gdImageWBMPPtr() / _gdImageWBMPCtx(): None on failure
pub fn image_wbmp(im: &Image, fg: i32) -> Option<Vec<u8>> {
    let (w, h) = (im.sx, im.sy);
    // createwbmp()
    if overflow2(4, w) || overflow2(4 * w, h) {
        gd_error("Could not create WBMP");
        return None;
    }
    let mut bitmap = vec![WBMP_WHITE; w as usize * h as usize];
    let mut pos = 0usize;
    for y in 0..h {
        for x in 0..w {
            if im.get_pixel(x, y) == fg {
                bitmap[pos] = WBMP_BLACK;
            }
            pos += 1;
        }
    }
    // writewbmp()
    let mut out = Out::new();
    out.put_c(0);
    out.put_c(0);
    putmbi(w, &mut out);
    putmbi(h, &mut out);
    for row in 0..h as usize {
        let mut bitpos = 8;
        let mut octet = 0;
        for col in 0..w as usize {
            bitpos -= 1;
            let v = if bitmap[row * w as usize + col] == 1 {
                WBMP_WHITE
            } else {
                WBMP_BLACK
            };
            octet |= v << bitpos;
            if bitpos == 0 {
                bitpos = 8;
                out.put_c(octet);
                octet = 0;
            }
        }
        if bitpos != 8 {
            out.put_c(octet);
        }
    }
    Some(out.into_vec())
}
