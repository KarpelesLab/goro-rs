//! Windows BMP (Microsoft "Bitmap Storage" documentation: BITMAPFILEHEADER,
//! BITMAPCOREHEADER / BITMAPINFOHEADER and its V4/V5 extensions; BI_RGB,
//! BI_RLE8, BI_RLE4, BI_BITFIELDS). Palette depths give palette images,
//! 16/24/32 bits truecolor images. imagebmp writes 8-bit (RLE8 when
//! compressed) palette or 24-bit truecolor files.

use super::{DecodeError, DecodeResult, OVERFLOW_MSG};
use crate::image::{Image, tc};

fn u16le(d: &[u8], i: usize) -> Option<u32> {
    Some(u16::from_le_bytes([*d.get(i)?, *d.get(i + 1)?]) as u32)
}
fn u32le(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

pub fn decode(d: &[u8]) -> DecodeResult {
    decode_inner(d).ok_or_else(DecodeError::silent)?
}

fn decode_inner(d: &[u8]) -> Option<DecodeResult> {
    if d.len() < 26 || &d[..2] != b"BM" {
        return None;
    }
    let off = u32le(d, 10)? as usize;
    let hsize = u32le(d, 14)? as usize;
    let (w, h, bpp, comp, ncolors, pal_entry) = if hsize == 12 {
        (
            u16le(d, 18)? as i64,
            u16le(d, 20)? as i16 as i64,
            u16le(d, 24)?,
            0,
            0,
            3,
        )
    } else if hsize >= 40 {
        (
            u32le(d, 18)? as i32 as i64,
            u32le(d, 22)? as i32 as i64,
            u16le(d, 28)?,
            u32le(d, 30)?,
            u32le(d, 46)?,
            4,
        )
    } else {
        return None;
    };
    if w <= 0 || h == 0 {
        return None;
    }
    let top_down = h < 0;
    let h = h.abs();
    if w * h > i32::MAX as i64 {
        return Some(Err(DecodeError::with(OVERFLOW_MSG)));
    }
    let (w, h) = (w as i32, h as i32);
    let pal_start = 14 + hsize;
    // Bitfield masks.
    let (mut rm, mut gm, mut bm, mut am) = (0u32, 0u32, 0u32, 0u32);
    let mut pal_off = pal_start;
    if comp == 3 {
        if hsize >= 52 {
            rm = u32le(d, 54)?;
            gm = u32le(d, 58)?;
            bm = u32le(d, 62)?;
            if hsize >= 56 {
                am = u32le(d, 66)?;
            }
        } else {
            rm = u32le(d, pal_start)?;
            gm = u32le(d, pal_start + 4)?;
            bm = u32le(d, pal_start + 8)?;
            pal_off += 12;
        }
    } else if bpp == 16 {
        rm = 0x7c00;
        gm = 0x03e0;
        bm = 0x001f;
    } else if bpp == 32 {
        rm = 0xff0000;
        gm = 0xff00;
        bm = 0xff;
    }
    let palette = bpp <= 8;
    let mut im = if palette {
        Image::new_palette(w, h)
    } else {
        Image::new_truecolor(w, h)
    }?;
    if palette {
        let n = if ncolors == 0 || ncolors > 256 {
            1usize << bpp
        } else {
            ncolors as usize
        };
        for i in 0..n {
            let p = pal_off + i * pal_entry;
            let Some(e) = d.get(p..p + 3) else {
                break;
            };
            im.color_allocate(e[2] as i32, e[1] as i32, e[0] as i32, 0);
        }
    }
    let data = d.get(off..)?;
    let row_y = |r: i32| if top_down { r } else { h - 1 - r };
    match (bpp, comp) {
        (1 | 4 | 8, 0) => {
            let stride = ((w as usize * bpp as usize).div_ceil(32)) * 4;
            for r in 0..h {
                let row = data.get(r as usize * stride..r as usize * stride + stride)?;
                for x in 0..w as usize {
                    let v = match bpp {
                        8 => row[x],
                        4 => (row[x / 2] >> (if x % 2 == 0 { 4 } else { 0 })) & 0xf,
                        _ => (row[x / 8] >> (7 - x % 8)) & 1,
                    };
                    im.put_raw(x as i32, row_y(r), v as i32);
                }
            }
        }
        (8, 1) | (4, 2) => {
            // Run-length encoding.
            let (mut x, mut r) = (0i32, 0i32);
            let mut i = 0;
            let four = bpp == 4;
            while i + 1 < data.len() && r < h {
                let n = data[i];
                let c = data[i + 1];
                i += 2;
                if n > 0 {
                    for k in 0..n {
                        let v = if four {
                            if k % 2 == 0 { c >> 4 } else { c & 0xf }
                        } else {
                            c
                        };
                        if x < w {
                            im.put_raw(x, row_y(r), v as i32);
                        }
                        x += 1;
                    }
                } else {
                    match c {
                        0 => {
                            x = 0;
                            r += 1;
                        }
                        1 => break,
                        2 => {
                            x += *data.get(i)? as i32;
                            r += *data.get(i + 1)? as i32;
                            i += 2;
                        }
                        n => {
                            let n = n as usize;
                            let bytes = if four { n.div_ceil(2) } else { n };
                            let run = data.get(i..i + bytes)?;
                            for k in 0..n {
                                let v = if four {
                                    let b = run[k / 2];
                                    if k % 2 == 0 { b >> 4 } else { b & 0xf }
                                } else {
                                    run[k]
                                };
                                if x < w {
                                    im.put_raw(x, row_y(r), v as i32);
                                }
                                x += 1;
                            }
                            i += bytes + (bytes & 1);
                        }
                    }
                }
            }
        }
        (16 | 32, 0 | 3) | (24, 0) => {
            let bytespp = bpp as usize / 8;
            let stride = (w as usize * bytespp).div_ceil(4) * 4;
            let chan = |v: u32, m: u32| -> i32 {
                if m == 0 {
                    return 0;
                }
                let shift = m.trailing_zeros();
                let bits = (m >> shift).count_ones();
                let x = (v & m) >> shift;
                if bits >= 8 {
                    (x >> (bits - 8)) as i32
                } else {
                    ((x * 255) / ((1 << bits) - 1)) as i32
                }
            };
            for r in 0..h {
                let row = data.get(r as usize * stride..r as usize * stride + w as usize * bytespp)?;
                for x in 0..w as usize {
                    let p = &row[x * bytespp..(x + 1) * bytespp];
                    let c = if bpp == 24 {
                        tc(p[2] as i32, p[1] as i32, p[0] as i32, 0)
                    } else {
                        let v = if bpp == 16 {
                            u16::from_le_bytes([p[0], p[1]]) as u32
                        } else {
                            u32::from_le_bytes([p[0], p[1], p[2], p[3]])
                        };
                        let a = if am != 0 {
                            127 - (chan(v, am) >> 1)
                        } else {
                            0
                        };
                        tc(chan(v, rm), chan(v, gm), chan(v, bm), a)
                    };
                    im.put_raw(x as i32, row_y(r), c);
                }
            }
        }
        _ => return None,
    }
    Some(Ok(im))
}

/// imagebmp.
pub fn encode(im: &Image, compressed: bool) -> Vec<u8> {
    let (w, h) = (im.sx as usize, im.sy as usize);
    let palette = !im.truecolor;
    let bpp: u16 = if palette { 8 } else { 24 };
    let ncolors = if palette { im.colors_total.max(0) as usize } else { 0 };
    let mut pixels = Vec::new();
    let rle = palette && compressed;
    if palette {
        let stride = w.div_ceil(4) * 4;
        for y in (0..h).rev() {
            let row = &im.pixels[y * w..(y + 1) * w];
            if rle {
                let mut x = 0;
                while x < w {
                    let mut n = 1;
                    while x + n < w && n < 255 && row[x + n] == row[x] {
                        n += 1;
                    }
                    pixels.push(n as u8);
                    pixels.push(row[x]);
                    x += n;
                }
                pixels.extend_from_slice(&[0, 0]);
            } else {
                pixels.extend_from_slice(row);
                pixels.resize(pixels.len() + (stride - w), 0);
            }
        }
        if rle {
            pixels.extend_from_slice(&[0, 1]);
        }
    } else {
        let stride = (w * 3).div_ceil(4) * 4;
        for y in (0..h).rev() {
            for x in 0..w {
                let c = im.tpixels[y * w + x];
                pixels.extend_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8]);
            }
            pixels.resize(pixels.len() + (stride - w * 3), 0);
        }
    }
    let off = 14 + 40 + ncolors * 4;
    let total = off + pixels.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(off as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&bpp.to_le_bytes());
    out.extend_from_slice(&(if rle { 1u32 } else { 0 }).to_le_bytes());
    out.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    let ppm = |dpi: u32| ((dpi as f64 / 0.0254) + 0.5) as u32;
    out.extend_from_slice(&ppm(im.res_x).to_le_bytes());
    out.extend_from_slice(&ppm(im.res_y).to_le_bytes());
    out.extend_from_slice(&(ncolors as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for i in 0..ncolors {
        out.extend_from_slice(&[im.blue[i] as u8, im.green[i] as u8, im.red[i] as u8, 0]);
    }
    out.extend_from_slice(&pixels);
    out
}
