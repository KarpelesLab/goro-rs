//! Truevision TGA (Truevision TGA File Format Specification 2.0):
//! uncompressed (type 2) and run-length (type 10) truecolor images and
//! grayscale (types 3/11), 16/24/32 bits, any origin corner.

use super::{DecodeError, DecodeResult, OVERFLOW_MSG};
use crate::image::{Image, tc};

pub fn decode(d: &[u8]) -> DecodeResult {
    let bad = DecodeError::silent;
    if d.len() < 18 {
        return Err(bad());
    }
    let idlen = d[0] as usize;
    let cmap_type = d[1];
    let typ = d[2];
    let cmap_len = u16::from_le_bytes([d[5], d[6]]) as usize;
    let cmap_bits = d[7] as usize;
    let w = u16::from_le_bytes([d[12], d[13]]) as i32;
    let h = u16::from_le_bytes([d[14], d[15]]) as i32;
    let bpp = d[16] as usize;
    let desc = d[17];
    if w == 0 || h == 0 {
        return Err(bad());
    }
    let gray = typ == 3 || typ == 11;
    let rle = typ >= 9;
    if !matches!(typ, 2 | 3 | 10 | 11) {
        return Err(bad());
    }
    if !(bpp == 16 || bpp == 24 || bpp == 32 || (gray && bpp == 8)) {
        return Err(bad());
    }
    let mut pos = 18 + idlen;
    if cmap_type == 1 {
        pos += cmap_len * cmap_bits.div_ceil(8);
    }
    let bytes = bpp / 8;
    let n = w as usize * h as usize;
    let mut px: Vec<u32> = Vec::with_capacity(n);
    let read_px = |p: &[u8]| -> u32 {
        match bytes {
            1 => {
                let g = p[0] as u32;
                0xff00_0000 | (g << 16) | (g << 8) | g
            }
            2 => {
                let v = u16::from_le_bytes([p[0], p[1]]) as u32;
                let c = |s: u32| ((v >> s) & 0x1f) * 255 / 31;
                let a = if v & 0x8000 != 0 || desc & 0x0f == 0 { 0xff } else { 0 };
                (a << 24) | (c(10) << 16) | (c(5) << 8) | c(0)
            }
            3 => 0xff00_0000 | ((p[2] as u32) << 16) | ((p[1] as u32) << 8) | p[0] as u32,
            _ => ((p[3] as u32) << 24) | ((p[2] as u32) << 16) | ((p[1] as u32) << 8) | p[0] as u32,
        }
    };
    if rle {
        while px.len() < n {
            let Some(&hd) = d.get(pos) else {
                return Err(bad());
            };
            pos += 1;
            let count = (hd & 0x7f) as usize + 1;
            if hd & 0x80 != 0 {
                let p = d.get(pos..pos + bytes).ok_or_else(bad)?;
                pos += bytes;
                let v = read_px(p);
                for _ in 0..count.min(n - px.len()) {
                    px.push(v);
                }
            } else {
                for _ in 0..count {
                    let p = d.get(pos..pos + bytes).ok_or_else(bad)?;
                    pos += bytes;
                    if px.len() < n {
                        px.push(read_px(p));
                    }
                }
            }
        }
    } else {
        let p = d.get(pos..pos + n * bytes).ok_or_else(bad)?;
        px.extend(p.chunks_exact(bytes).map(read_px));
    }
    let mut im = Image::new_truecolor(w, h).ok_or_else(|| DecodeError::with(OVERFLOW_MSG))?;
    let right_to_left = desc & 0x10 != 0;
    let top_down = desc & 0x20 != 0;
    let has_alpha = bpp == 32 && desc & 0x0f != 0;
    for (i, &v) in px.iter().enumerate() {
        let (x, y) = ((i % w as usize) as i32, (i / w as usize) as i32);
        let x = if right_to_left { w - 1 - x } else { x };
        let y = if top_down { y } else { h - 1 - y };
        let a8 = if has_alpha || bpp == 16 { (v >> 24) as i32 } else { 255 };
        let c = tc(
            ((v >> 16) & 0xff) as i32,
            ((v >> 8) & 0xff) as i32,
            (v & 0xff) as i32,
            127 - (a8 >> 1),
        );
        im.put_raw(x, y, c);
    }
    Ok(im)
}
