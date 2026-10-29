//! GD's own formats, GD (imagegd) and GD2 (imagegd2), as libgd's
//! documentation describes them and the ext/gd tests check them
//! (imagegd_truecolor: a 10x10 truecolor GD file is 411 bytes;
//! bug73155: a 64x64 raw palette GD2 file is 5145 bytes; bug73159:
//! format 4 for a compressed truecolor GD2; createfromgd2 reads
//! src.gd2):
//!
//! - GD: signature 0xFFFE (truecolor) or 0xFFFF (palette), width,
//!   height (16-bit big-endian), a truecolor flag byte, then for palette
//!   images the color count (16 bits), the transparent index (32 bits)
//!   and 256 RGBA entries, for truecolor images the transparent color
//!   (32 bits); then the pixels (one byte, or a 32-bit ARGB value).
//! - GD2: "gd2\0", version (2), width, height, chunk size, format (1
//!   raw palette, 2 compressed palette, 3 raw truecolor, 4 compressed
//!   truecolor), chunk counts across and down; compressed files then
//!   list each chunk's offset and size; then the color header as in GD;
//!   then the chunks, row by row, each a zlib stream when compressed.

use super::{DecodeError, DecodeResult, OVERFLOW_MSG};
use crate::image::{Image, MAX_COLORS};

struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.d.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let v = u16::from_be_bytes(self.d.get(self.p..self.p + 2)?.try_into().ok()?);
        self.p += 2;
        Some(v)
    }
    fn i32(&mut self) -> Option<i32> {
        let v = i32::from_be_bytes(self.d.get(self.p..self.p + 4)?.try_into().ok()?);
        self.p += 4;
        Some(v)
    }
}

fn overflow(w: i32, h: i32, tc: bool) -> bool {
    crate::image::overflow(w, h, if tc { 4 } else { 1 })
}

/// The color header shared by both formats. `Err(true)`: invalid.
fn read_colors(r: &mut Rd, im: &mut Image, truecolor_flag: bool) -> Option<()> {
    if truecolor_flag {
        im.transparent = r.i32()?;
        return Some(());
    }
    let total = r.u16()? as usize;
    if total > MAX_COLORS {
        return None;
    }
    let t = r.i32()?;
    for i in 0..MAX_COLORS {
        let red = r.u8()? as i32;
        let green = r.u8()? as i32;
        let blue = r.u8()? as i32;
        let alpha = r.u8()? as i32;
        im.red[i] = red;
        im.green[i] = green;
        im.blue[i] = blue;
        im.alpha[i] = alpha.min(127);
        im.open[i] = i >= total;
    }
    im.colors_total = total as i32;
    im.transparent = if t >= 0 && (t as usize) < MAX_COLORS {
        t
    } else {
        -1
    };
    Some(())
}

/// imagecreatefromgd.
pub fn decode_gd(d: &[u8]) -> DecodeResult {
    let bad = DecodeError::silent;
    let mut r = Rd { d, p: 0 };
    let sig = r.u16().ok_or_else(bad)?;
    let (w, h, legacy) = if sig == 0xfffe || sig == 0xffff {
        (
            r.u16().ok_or_else(bad)? as i32,
            r.u16().ok_or_else(bad)? as i32,
            false,
        )
    } else {
        (sig as i32, r.u16().ok_or_else(bad)? as i32, true)
    };
    let tcolor = sig == 0xfffe;
    if overflow(w, h, tcolor) {
        return Err(DecodeError::with(OVERFLOW_MSG));
    }
    if w == 0 || h == 0 {
        return Err(bad());
    }
    let mut im = if tcolor {
        Image::new_truecolor(w, h)
    } else {
        Image::new_palette(w, h)
    }
    .ok_or_else(bad)?;
    if legacy {
        let total = r.u8().ok_or_else(bad)? as i32;
        let t = r.u16().ok_or_else(bad)? as i32;
        for i in 0..MAX_COLORS {
            im.red[i] = r.u8().ok_or_else(bad)? as i32;
            im.green[i] = r.u8().ok_or_else(bad)? as i32;
            im.blue[i] = r.u8().ok_or_else(bad)? as i32;
            im.open[i] = i as i32 >= total;
        }
        im.colors_total = if total == 0 { 256 } else { total };
        im.transparent = if t == 257 { -1 } else { t };
    } else {
        let flag = r.u8().ok_or_else(bad)?;
        if (flag != 0) != tcolor {
            return Err(bad());
        }
        read_colors(&mut r, &mut im, tcolor).ok_or_else(bad)?;
    }
    if tcolor {
        for p in im.tpixels.iter_mut() {
            *p = r.i32().ok_or_else(bad)?;
        }
    } else {
        let n = im.pixels.len();
        let s = d.get(r.p..r.p + n).ok_or_else(bad)?;
        im.pixels.copy_from_slice(s);
    }
    Ok(im)
}

pub fn encode_gd(im: &Image) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(&(if im.truecolor { 0xfffeu16 } else { 0xffff }).to_be_bytes());
    o.extend_from_slice(&(im.sx as u16).to_be_bytes());
    o.extend_from_slice(&(im.sy as u16).to_be_bytes());
    write_colors(&mut o, im);
    write_pixels(&mut o, im, 0, 0, im.sx, im.sy);
    o
}

fn write_colors(o: &mut Vec<u8>, im: &Image) {
    o.push(im.truecolor as u8);
    if im.truecolor {
        o.extend_from_slice(&im.transparent.to_be_bytes());
        return;
    }
    o.extend_from_slice(&(im.colors_total as u16).to_be_bytes());
    o.extend_from_slice(&im.transparent.to_be_bytes());
    for i in 0..MAX_COLORS {
        o.extend_from_slice(&[
            im.red[i] as u8,
            im.green[i] as u8,
            im.blue[i] as u8,
            im.alpha[i] as u8,
        ]);
    }
}

fn write_pixels(o: &mut Vec<u8>, im: &Image, x0: i32, y0: i32, x1: i32, y1: i32) {
    for y in y0..y1 {
        for x in x0..x1 {
            if im.truecolor {
                o.extend_from_slice(&im.get_pixel(x, y).to_be_bytes());
            } else {
                o.push(im.get_pixel(x, y) as u8);
            }
        }
    }
}

pub const GD2_RAW: i64 = 1;
pub const GD2_COMPRESSED: i64 = 2;

/// imagegd2.
pub fn encode_gd2(im: &Image, chunk: i64, mode: i64) -> Vec<u8> {
    let cs = if chunk == 0 {
        128
    } else {
        chunk.clamp(64, 4096)
    } as i32;
    let compressed = mode != GD2_RAW;
    let fmt: u16 = match (im.truecolor, compressed) {
        (false, false) => 1,
        (false, true) => 2,
        (true, false) => 3,
        (true, true) => 4,
    };
    let ncx = (im.sx + cs - 1) / cs;
    let ncy = (im.sy + cs - 1) / cs;
    let mut o = Vec::new();
    o.extend_from_slice(b"gd2\0");
    o.extend_from_slice(&2u16.to_be_bytes());
    o.extend_from_slice(&(im.sx as u16).to_be_bytes());
    o.extend_from_slice(&(im.sy as u16).to_be_bytes());
    o.extend_from_slice(&(cs as u16).to_be_bytes());
    o.extend_from_slice(&fmt.to_be_bytes());
    o.extend_from_slice(&(ncx as u16).to_be_bytes());
    o.extend_from_slice(&(ncy as u16).to_be_bytes());
    let index_at = o.len();
    if compressed {
        o.resize(o.len() + (ncx * ncy) as usize * 8, 0);
    }
    write_colors(&mut o, im);
    let mut index = Vec::new();
    for cy in 0..ncy {
        for cx in 0..ncx {
            let (x0, y0) = (cx * cs, cy * cs);
            let (x1, y1) = ((x0 + cs).min(im.sx), (y0 + cs).min(im.sy));
            if compressed {
                let mut raw = Vec::new();
                write_pixels(&mut raw, im, x0, y0, x1, y1);
                let z = goro_ext_zlib::compress(&raw, 6, goro_ext_zlib::Wrap::Zlib);
                index.push((o.len() as u32, z.len() as u32));
                o.extend_from_slice(&z);
            } else {
                write_pixels(&mut o, im, x0, y0, x1, y1);
            }
        }
    }
    for (i, (off, size)) in index.iter().enumerate() {
        let p = index_at + i * 8;
        o[p..p + 4].copy_from_slice(&off.to_be_bytes());
        o[p + 4..p + 8].copy_from_slice(&size.to_be_bytes());
    }
    o
}

/// imagecreatefromgd2 / imagecreatefromgd2part (`part`: x, y, w, h).
pub fn decode_gd2(d: &[u8], part: Option<(i32, i32, i32, i32)>) -> DecodeResult {
    let bad = DecodeError::silent;
    let mut r = Rd { d, p: 0 };
    if d.len() < 4 || &d[..4] != b"gd2\0" {
        return Err(bad());
    }
    r.p = 4;
    let version = r.u16().ok_or_else(bad)?;
    if version != 1 && version != 2 {
        return Err(bad());
    }
    let sx = r.u16().ok_or_else(bad)? as i32;
    let sy = r.u16().ok_or_else(bad)? as i32;
    let cs = r.u16().ok_or_else(bad)? as i32;
    let fmt = r.u16().ok_or_else(bad)?;
    let ncx = r.u16().ok_or_else(bad)? as i32;
    let ncy = r.u16().ok_or_else(bad)? as i32;
    if !(64..=4096).contains(&cs) || !(1..=4).contains(&fmt) {
        return Err(bad());
    }
    if fmt >= 3 && version < 2 {
        return Err(bad());
    }
    let tcolor = fmt >= 3;
    let compressed = fmt == 2 || fmt == 4;
    let mut index = Vec::new();
    if compressed {
        // The chunk index allocation (tests bug72339, bug73869).
        let n = ncx as i64 * ncy as i64;
        if n == 0 {
            return Err(DecodeError::with(super::NEGATIVE_MSG));
        }
        if n * 8 > i32::MAX as i64 {
            return Err(DecodeError::with(OVERFLOW_MSG));
        }
        for _ in 0..n {
            let off = r.i32().ok_or_else(bad)?;
            let size = r.i32().ok_or_else(bad)?;
            if off < 0 || size <= 0 || off as usize + size as usize > d.len() {
                return Err(bad());
            }
            index.push((off as usize, size as usize));
        }
    }
    let (px, py, pw, ph) = part.unwrap_or((0, 0, sx, sy));
    if overflow(pw, ph, tcolor) {
        return Err(DecodeError::with(OVERFLOW_MSG));
    }
    if pw <= 0 || ph <= 0 {
        return Err(bad());
    }
    let mut im = if tcolor {
        Image::new_truecolor(pw, ph)
    } else {
        Image::new_palette(pw, ph)
    }
    .ok_or_else(bad)?;
    let flag = r.u8().ok_or_else(bad)?;
    let _ = flag;
    read_colors(&mut r, &mut im, tcolor).ok_or_else(bad)?;
    let bpp = if tcolor { 4 } else { 1 };
    let mut ci = 0;
    for cy in 0..ncy {
        for cx in 0..ncx {
            let (x0, y0) = (cx * cs, cy * cs);
            let (x1, y1) = ((x0 + cs).min(sx), (y0 + cs).min(sy));
            let cw = (x1 - x0).max(0) as usize;
            let chh = (y1 - y0).max(0) as usize;
            let need = cw * chh * bpp;
            let data: std::borrow::Cow<[u8]> = if compressed {
                let (off, size) = index[ci];
                ci += 1;
                let mut out = Vec::with_capacity(need);
                let mut inf = goro_ext_zlib::Inflater::new(goro_ext_zlib::InflateWrap::Zlib, 15);
                let (_, st) = inf.inflate(
                    &d[off..off + size],
                    &mut out,
                    need,
                    goro_ext_zlib::InflateFlush::Finish,
                );
                if matches!(st, goro_ext_zlib::InflateStatus::DataError(_)) || out.len() < need {
                    return Err(bad());
                }
                std::borrow::Cow::Owned(out)
            } else {
                if need == 0 {
                    continue;
                }
                match d.get(r.p..r.p + need) {
                    Some(s) => {
                        r.p += need;
                        std::borrow::Cow::Borrowed(s)
                    }
                    None => {
                        return Err(DecodeError::with("gd2: EOF while reading\n"));
                    }
                }
            };
            let mut k = 0;
            for y in y0..y1 {
                for x in x0..x1 {
                    let v = if tcolor {
                        i32::from_be_bytes(data[k..k + 4].try_into().unwrap())
                    } else {
                        data[k] as i32
                    };
                    k += bpp;
                    let (tx, ty) = (x - px, y - py);
                    if tx >= 0 && ty >= 0 && tx < pw && ty < ph {
                        im.put_raw(tx, ty, v);
                    }
                }
            }
        }
    }
    Ok(im)
}
