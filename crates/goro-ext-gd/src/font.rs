//! Bitmap fonts: GD's five built-in fonts and fonts loaded from `.gdf`
//! files (imageloadfont), and drawing characters and strings with them
//! (imagechar, imagecharup, imagestring, imagestringup).
//!
//! The .gdf layout follows the PHP manual (imageloadfont): number of
//! characters, first character, character width and height as 4-byte
//! integers, then one byte per pixel.

use crate::font_data as fd;
use crate::image::Image;

#[derive(Clone, Debug)]
pub struct Font {
    pub nchars: i32,
    pub offset: i32,
    pub w: i32,
    pub h: i32,
    /// One byte per pixel, `nchars * w * h` bytes.
    pub data: Vec<u8>,
}

fn builtin(w: usize, h: usize, rows: &[u16]) -> Font {
    let mut data = vec![0u8; 256 * w * h];
    for ch in 0..256 {
        for r in 0..h {
            let bits = rows[ch * h + r];
            for c in 0..w {
                if bits >> (w - 1 - c) & 1 != 0 {
                    data[(ch * h + r) * w + c] = 1;
                }
            }
        }
    }
    Font {
        nchars: 256,
        offset: 0,
        w: w as i32,
        h: h as i32,
        data,
    }
}

/// Built-in font 1..=5 (PHP clamps other numbers into this range).
pub fn builtin_font(n: i64) -> &'static Font {
    use std::sync::OnceLock;
    static F: OnceLock<[Font; 5]> = OnceLock::new();
    let fonts = F.get_or_init(|| {
        [
            builtin(fd::TINY_W, fd::TINY_H, &fd::TINY),
            builtin(fd::SMALL_W, fd::SMALL_H, &fd::SMALL),
            builtin(fd::MEDIUM_BOLD_W, fd::MEDIUM_BOLD_H, &fd::MEDIUM_BOLD),
            builtin(fd::LARGE_W, fd::LARGE_H, &fd::LARGE),
            builtin(fd::GIANT_W, fd::GIANT_H, &fd::GIANT),
        ]
    });
    let i = n.clamp(1, 5) as usize - 1;
    &fonts[i]
}

/// Errors of imageloadfont (warning texts PHP prints).
#[derive(Debug, PartialEq)]
pub enum FontError {
    HeaderEof,
    Overflow,
    BadHeader,
    DataEof,
}

/// Parses a .gdf file. The header integers are read in the machine's
/// byte order, or big-endian when that gives a plausible font (as PHP
/// does for portability: PHP manual, imageloadfont).
pub fn load_gdf(b: &[u8]) -> Result<Font, FontError> {
    if b.len() < 16 {
        return Err(FontError::HeaderEof);
    }
    // Little-endian header; when its body is not all there, the header
    // is read again big-endian (tests imageloadfont_invalid, bug81739,
    // gh13082).
    let le = |i: usize| i32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let be = |i: usize| i32::from_be_bytes(b[i..i + 4].try_into().unwrap());
    let ok = |n: i32, w: i32, h: i32| n > 0 && w > 0 && h > 0;
    let check = |n: i32, w: i32, h: i32| -> Result<usize, FontError> {
        if overflow3(n, w, h) {
            return Err(FontError::Overflow);
        }
        if !ok(n, w, h) {
            return Err(FontError::BadHeader);
        }
        Ok(n as usize * w as usize * h as usize)
    };
    let (mut nchars, mut offset, mut w, mut h) = (le(0), le(4), le(8), le(12));
    let mut size = check(nchars, w, h)?;
    if b.len() - 16 < size {
        let (bn, bo, bw, bh) = (be(0), be(4), be(8), be(12));
        size = check(bn, bw, bh)?;
        if b.len() - 16 < size {
            return Err(FontError::DataEof);
        }
        nchars = bn;
        offset = bo;
        w = bw;
        h = bh;
    }
    Ok(Font {
        nchars,
        offset,
        w,
        h,
        data: b[16..16 + size].to_vec(),
    })
}

fn overflow3(a: i32, b: i32, c: i32) -> bool {
    let p = a as i64 * b as i64;
    p > i32::MAX as i64 || p < i32::MIN as i64 || p * c as i64 > i32::MAX as i64
}

impl Font {
    #[inline]
    fn pixel(&self, ch: i32, x: i32, y: i32) -> bool {
        let fline = (ch - self.offset) as i64 * self.h as i64 * self.w as i64;
        let i = fline + y as i64 * self.w as i64 + x as i64;
        i >= 0 && (i as usize) < self.data.len() && self.data[i as usize] != 0
    }

    fn has(&self, ch: i32) -> bool {
        ch >= self.offset && ch < self.offset + self.nchars
    }
}

impl Image {
    /// imagechar: one character, left to right.
    pub fn draw_char(&mut self, f: &Font, x: i32, y: i32, ch: i32, color: i32) {
        if !f.has(ch) {
            return;
        }
        for py in 0..f.h {
            for px in 0..f.w {
                if f.pixel(ch, px, py) {
                    self.set_pixel(x.wrapping_add(px), y.wrapping_add(py), color);
                }
            }
        }
    }

    /// imagecharup: one character rotated 90° counter-clockwise.
    pub fn draw_char_up(&mut self, f: &Font, x: i32, y: i32, ch: i32, color: i32) {
        if !f.has(ch) {
            return;
        }
        for py in 0..f.h {
            for px in 0..f.w {
                if f.pixel(ch, px, py) {
                    self.set_pixel(x.wrapping_add(py), y.wrapping_sub(px), color);
                }
            }
        }
    }

    /// imagestring.
    pub fn draw_string(&mut self, f: &Font, x: i32, y: i32, s: &[u8], color: i32) {
        let mut x = x as i64;
        for &c in s {
            if x > i32::MAX as i64 || x < i32::MIN as i64 - f.w as i64 {
                break;
            }
            if x + f.w as i64 >= 0 && x < self.sx as i64 {
                self.draw_char(f, x as i32, y, c as i32, color);
            }
            x += f.w as i64;
        }
    }

    /// imagestringup.
    pub fn draw_string_up(&mut self, f: &Font, x: i32, y: i32, s: &[u8], color: i32) {
        let mut y = y as i64;
        for &c in s {
            if y < i32::MIN as i64 || y > i32::MAX as i64 + f.w as i64 {
                break;
            }
            if y - (f.w as i64) < self.sy as i64 && y >= 0 {
                self.draw_char_up(f, x, y as i32, c as i32, color);
            }
            y -= f.w as i64;
        }
    }
}
