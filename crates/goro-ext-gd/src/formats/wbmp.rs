//! WBMP type 0 (WAP-237 Wireless Application Environment, section 6
//! "Wireless Bitmap"): multi-byte width and height, 1 bit per pixel,
//! 1 = white. GD reads it as a two-color palette image (0 white,
//! 1 black: test createfromwbmp).

use super::{DecodeError, DecodeResult, OVERFLOW_MSG};
use crate::image::Image;

fn mbi(d: &[u8], pos: &mut usize) -> Option<u32> {
    let mut v: u32 = 0;
    for _ in 0..5 {
        let b = *d.get(*pos)?;
        *pos += 1;
        v = (v << 7) | (b & 0x7f) as u32;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

pub fn looks_like(d: &[u8]) -> bool {
    if d.len() < 4 || d[0] != 0 || d[1] != 0 {
        return false;
    }
    let mut p = 2;
    matches!((mbi(d, &mut p), mbi(d, &mut p)), (Some(w), Some(h)) if w > 0 && h > 0)
}

pub fn decode(d: &[u8]) -> DecodeResult {
    let mut p = 0;
    let typ = mbi(d, &mut p).ok_or_else(DecodeError::silent)?;
    if typ != 0 {
        return Err(DecodeError::silent());
    }
    // Fix header field (extension headers are not used by type 0).
    let fh = *d.get(p).ok_or_else(DecodeError::silent)?;
    p += 1;
    if fh & 0x80 != 0 {
        return Err(DecodeError::silent());
    }
    let w = mbi(d, &mut p).ok_or_else(DecodeError::silent)?;
    let h = mbi(d, &mut p).ok_or_else(DecodeError::silent)?;
    if w == 0 || h == 0 {
        return Err(DecodeError::silent());
    }
    let n = w as u64 * h as u64;
    if w > i32::MAX as u32 || h > i32::MAX as u32 || n > i32::MAX as u64 || n * 4 > i32::MAX as u64
    {
        return Err(DecodeError::with(OVERFLOW_MSG));
    }
    let row = (w as usize).div_ceil(8);
    if d.len() - p < row * h as usize {
        return Err(DecodeError::silent());
    }
    let mut im =
        Image::new_palette(w as i32, h as i32).ok_or_else(|| DecodeError::with(OVERFLOW_MSG))?;
    im.color_allocate(255, 255, 255, 0);
    im.color_allocate(0, 0, 0, 0);
    for y in 0..h as usize {
        let r = &d[p + y * row..p + (y + 1) * row];
        for x in 0..w as usize {
            let bit = (r[x / 8] >> (7 - (x % 8))) & 1;
            im.pixels[y * w as usize + x] = if bit == 1 { 0 } else { 1 };
        }
    }
    Ok(im)
}

/// Errors of imagewbmp.
pub fn encode(im: &Image, fg: i32) -> Result<Vec<u8>, String> {
    let (w, h) = (im.sx as u64, im.sy as u64);
    if w * h > i32::MAX as u64 || w * h * 4 > i32::MAX as u64 {
        return Err(OVERFLOW_MSG.to_string());
    }
    let mut out = vec![0u8, 0u8];
    let put = |out: &mut Vec<u8>, v: u32| {
        let mut bytes = vec![(v & 0x7f) as u8];
        let mut v = v >> 7;
        while v > 0 {
            bytes.push(0x80 | (v & 0x7f) as u8);
            v >>= 7;
        }
        bytes.reverse();
        out.extend_from_slice(&bytes);
    };
    put(&mut out, w as u32);
    put(&mut out, h as u32);
    let row = (w as usize).div_ceil(8);
    for y in 0..h as i32 {
        let mut r = vec![0u8; row];
        for x in 0..w as i32 {
            if im.get_pixel(x, y) != fg {
                r[x as usize / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&r);
    }
    Ok(out)
}
