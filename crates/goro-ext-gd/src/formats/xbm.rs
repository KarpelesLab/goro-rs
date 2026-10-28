//! XBM (X11 bitmap C source): `#define NAME_width`, `#define
//! NAME_height`, then `static [unsigned] char NAME_bits[] = { 0x.., }`
//! (or X10 `short` words), least significant bit first. Read as a
//! two-color palette image (white, black). imagexbm writes the format
//! as test bug66339 shows.

use super::{DecodeError, DecodeResult, OVERFLOW_MSG};
use crate::image::Image;

pub fn decode(d: &[u8]) -> DecodeResult {
    let text = String::from_utf8_lossy(d);
    let mut width: Option<i64> = None;
    let mut height: Option<i64> = None;
    let mut rest: Option<(usize, bool)> = None;
    let mut off = 0usize;
    for line in text.split_inclusive('\n') {
        let l = line.trim();
        if let Some(def) = l.strip_prefix("#define") {
            let mut it = def.split_whitespace();
            if let (Some(name), Some(v)) = (it.next(), it.next())
                && let Ok(v) = v.parse::<i64>()
            {
                if name.ends_with("width") {
                    width = Some(v);
                } else if name.ends_with("height") {
                    height = Some(v);
                }
            }
        } else if l.starts_with("static") && l.contains("[]") {
            if width.is_none() || height.is_none() {
                return Err(DecodeError::silent());
            }
            let short = l.contains("short");
            let brace = line.find('{');
            let start = match brace {
                Some(b) => off + b + 1,
                None => off + line.len(),
            };
            rest = Some((start, short));
            break;
        }
        off += line.len();
    }
    let (Some(w), Some(h), Some((start, short))) = (width, height, rest) else {
        return Err(DecodeError::silent());
    };
    if w <= 0 || h <= 0 || w > i32::MAX as i64 || h > i32::MAX as i64 {
        return Err(DecodeError::silent());
    }
    if w * h > i32::MAX as i64 {
        return Err(DecodeError::with(OVERFLOW_MSG));
    }
    let body = &text[start.min(text.len())..];
    let mut vals = Vec::new();
    let unit = if short { 16 } else { 8 };
    let needed = ((w as usize).div_ceil(unit)) * h as usize;
    let mut chars = body.char_indices().peekable();
    while vals.len() < needed {
        // Skip separators.
        while let Some(&(_, c)) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(i, c)) = chars.peek() else {
            break;
        };
        if c == '}' {
            break;
        }
        let tok: String = body[i..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let hex = tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X"));
        match hex.and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(v) => vals.push(v),
            None => return Err(DecodeError::with("Invalid XBM")),
        }
        for _ in 0..tok.chars().count().max(1) {
            chars.next();
        }
    }
    if vals.len() < needed {
        return Err(DecodeError::with("Invalid XBM"));
    }
    let mut im = Image::new_palette(w as i32, h as i32).ok_or_else(|| DecodeError::with(OVERFLOW_MSG))?;
    im.color_allocate(255, 255, 255, 0);
    im.color_allocate(0, 0, 0, 0);
    let per_row = (w as usize).div_ceil(unit);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let v = vals[y * per_row + x / unit];
            let bit = (v >> (x % unit)) & 1;
            im.pixels[y * w as usize + x] = bit as u8;
        }
    }
    Ok(im)
}

/// imagexbm: pixels of the foreground color are set bits; `name` is
/// the C identifier prefix.
pub fn encode(im: &Image, name: &str, fg: i32) -> Vec<u8> {
    let mut s = format!(
        "#define {name}_width {}\n#define {name}_height {}\nstatic unsigned char {name}_bits[] = {{\n  ",
        im.sx, im.sy
    );
    let per_row = (im.sx as usize).div_ceil(8);
    let total = per_row * im.sy as usize;
    let mut n = 0;
    for y in 0..im.sy {
        for bx in 0..per_row {
            let mut b = 0u32;
            for bit in 0..8 {
                let x = (bx * 8 + bit) as i32;
                if x < im.sx && im.get_pixel(x, y) == fg {
                    b |= 1 << bit;
                }
            }
            n += 1;
            s.push_str(&format!("0x{b:02X}"));
            if n < total {
                s.push_str(", ");
                if n % 12 == 0 {
                    s.push_str("\n  ");
                }
            }
        }
    }
    s.push_str("};\n");
    s.into_bytes()
}
