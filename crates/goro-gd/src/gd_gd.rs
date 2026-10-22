//! gd_gd.c: the GD (gd 1.x / 2.x) image format.

use crate::io::{EOF, In, Out};
use crate::*;

/// _gdGetColors(): shared by the GD and GD2 readers
pub(crate) fn get_colors(inp: &mut In, im: &mut Image, gd2x: bool) -> bool {
    if gd2x {
        let Some(tc) = inp.get_byte() else {
            return false;
        };
        if tc != im.true_color as i32 {
            return false;
        }
        if !im.true_color {
            let Some(n) = inp.get_word() else {
                return false;
            };
            im.colors_total = n;
            if im.colors_total > MAX_COLORS as i32 {
                return false;
            }
        }
        let Some(t) = inp.get_int() else {
            return false;
        };
        im.transparent = t;
    } else {
        let Some(n) = inp.get_byte() else {
            return false;
        };
        im.colors_total = n;
        let Some(t) = inp.get_word() else {
            return false;
        };
        im.transparent = t;
        if im.transparent == 257 {
            im.transparent = -1;
        }
    }
    if im.true_color {
        return true;
    }
    for i in 0..MAX_COLORS {
        let Some(r) = inp.get_byte() else {
            return false;
        };
        im.red[i] = r;
        let Some(g) = inp.get_byte() else {
            return false;
        };
        im.green[i] = g;
        let Some(b) = inp.get_byte() else {
            return false;
        };
        im.blue[i] = b;
        if gd2x {
            let Some(a) = inp.get_byte() else {
                return false;
            };
            im.alpha[i] = a;
        }
    }
    for i in 0..im.colors_total.max(0) as usize {
        im.open[i] = 0;
    }
    true
}

/// _gdPutColors()
pub(crate) fn put_colors(im: &Image, out: &mut Out) {
    out.put_c(im.true_color as i32);
    if !im.true_color {
        out.put_word(im.colors_total);
    }
    out.put_int(im.transparent);
    if !im.true_color {
        for i in 0..MAX_COLORS {
            out.put_c(im.red[i]);
            out.put_c(im.green[i]);
            out.put_c(im.blue[i]);
            out.put_c(im.alpha[i]);
        }
    }
}

/// _gdCreateFromFile()
fn create_from_file(inp: &mut In) -> Option<(Image, i32, i32)> {
    let mut gd2x = false;
    let mut true_color = false;
    let mut sx = inp.get_word()?;
    if sx == 65535 || sx == 65534 {
        gd2x = true;
        if sx == 65534 {
            true_color = true;
        }
        sx = inp.get_word()?;
    }
    let sy = inp.get_word()?;
    let mut im = if true_color {
        Image::create_true_color(sx, sy)?
    } else {
        Image::create(sx, sy)?
    };
    if !get_colors(inp, &mut im, gd2x) {
        return None;
    }
    Some((im, sx, sy))
}

/// gdImageCreateFromGdCtx()
pub fn create_from_gd(data: &[u8]) -> Option<Image> {
    let mut inp = In::new(data);
    let (mut im, sx, sy) = create_from_file(&mut inp)?;
    if im.true_color {
        for y in 0..sy as usize {
            for x in 0..sx as usize {
                im.tpixels[y][x] = inp.get_int()?;
            }
        }
    } else {
        for y in 0..sy as usize {
            for x in 0..sx as usize {
                let ch = inp.get_c();
                if ch == EOF {
                    return None;
                }
                im.pixels[y][x] = ch as u8;
            }
        }
    }
    Some(im)
}

/// gdImageGdPtr()
pub fn image_gd(im: &Image) -> Vec<u8> {
    let mut out = Out::new();
    out.put_word(if im.true_color { 65534 } else { 65535 });
    out.put_word(im.sx);
    out.put_word(im.sy);
    put_colors(im, &mut out);
    for y in 0..im.sy as usize {
        for x in 0..im.sx as usize {
            if im.true_color {
                out.put_int(im.tpixels[y][x]);
            } else {
                out.put_c(im.pixels[y][x] as i32);
            }
        }
    }
    out.into_vec()
}
