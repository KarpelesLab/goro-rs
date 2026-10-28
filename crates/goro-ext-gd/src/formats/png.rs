//! PNG reading and writing (PNG specification, W3C 2nd edition / ISO
//! 15948), mapped to GD images as PHP's imagecreatefrompng()/imagepng()
//! do: palette PNGs give palette images, RGB(A) and grayscale-with-alpha
//! give truecolor images, alpha is reduced to GD's 7 bits.

use crate::image::{ALPHA_TRANSPARENT, Image, tc};
use goro_ext_zlib::{InflateFlush, InflateStatus, InflateWrap, Inflater, Wrap};

pub const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// A decoding failure, with the message PHP prints (after "gd-png: ").
#[derive(Debug)]
pub struct PngError(pub String);

fn err<T>(m: &str) -> Result<T, PngError> {
    Err(PngError(m.to_string()))
}

fn crc_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb88320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    })
}

pub fn crc32(parts: &[&[u8]]) -> u32 {
    let t = crc_table();
    let mut c = 0xffff_ffffu32;
    for p in parts {
        for &b in *p {
            c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xffff_ffff
}

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    ctype: u8,
    interlace: u8,
}

/// Samples per pixel of a color type.
fn channels(ctype: u8) -> usize {
    match ctype {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => 0,
    }
}

/// Reads a PNG file into a GD image, with PHP's diagnostics: a file
/// without the PNG signature is refused silently; libpng-level errors
/// print "gd-png:  fatal libpng error: ..." and "gd-png error: setjmp
/// returns error condition" (tests bug39780, libgd00086, 001).
pub fn decode_gd(data: &[u8]) -> super::DecodeResult {
    if data.len() < 8 || &data[..8] != SIGNATURE {
        return Err(super::DecodeError::silent());
    }
    decode(data).map_err(|e| super::DecodeError {
        warnings: vec![
            format!("gd-png:  fatal libpng error: {}", e.0),
            "gd-png error: setjmp returns error condition".to_string(),
        ],
    })
}

/// Reads a PNG file into a GD image.
pub fn decode(data: &[u8]) -> Result<Image, PngError> {
    if data.len() < 8 || &data[..8] != SIGNATURE {
        return err("Not a PNG file");
    }
    let mut pos = 8;
    let mut hdr: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Option<Vec<u8>> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut phys: Option<(u32, u32, u8)> = None;
    let mut seen_iend = false;
    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &data[pos + 4..pos + 8];
        let start = pos + 8;
        if len > 0x7fff_ffff {
            return err("PNG unsigned integer out of range");
        }
        if start + len > data.len() {
            if kind == b"IDAT" {
                idat.extend_from_slice(&data[start..]);
            }
            break;
        }
        let body = &data[start..start + len];
        pos = start + len + 4;
        match kind {
            b"IHDR" => {
                if len != 13 {
                    return err("IHDR: invalid");
                }
                let width = u32::from_be_bytes(body[0..4].try_into().unwrap());
                let height = u32::from_be_bytes(body[4..8].try_into().unwrap());
                if width == 0 || height == 0 {
                    return err("Image width or height is zero in IHDR");
                }
                let h = Header {
                    width,
                    height,
                    depth: body[8],
                    ctype: body[9],
                    interlace: body[12],
                };
                let ok = match h.ctype {
                    0 => matches!(h.depth, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(h.depth, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(h.depth, 8 | 16),
                    _ => false,
                };
                if !ok {
                    return err("Invalid IHDR data");
                }
                hdr = Some(h);
            }
            b"PLTE" => {
                palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => trns = Some(body.to_vec()),
            b"IDAT" => idat.extend_from_slice(body),
            b"pHYs" if len == 9 => {
                let x = u32::from_be_bytes(body[0..4].try_into().unwrap());
                let y = u32::from_be_bytes(body[4..8].try_into().unwrap());
                phys = Some((x, y, body[8]));
            }
            b"IEND" => {
                seen_iend = true;
                break;
            }
            _ => {}
        }
    }
    let _ = seen_iend;
    let Some(h) = hdr else {
        return err("Missing IHDR before IDAT");
    };
    if h.width > i32::MAX as u32 || h.height > i32::MAX as u32 {
        return err("Image width exceeds user limit in IHDR");
    }
    if h.ctype == 3 && palette.is_empty() {
        return err("Missing PLTE before IDAT");
    }
    let ch = channels(h.ctype);
    let bpp_bits = ch * h.depth as usize;
    let row_bytes = |w: usize| (w * bpp_bits).div_ceil(8);
    // Decompress.
    let mut raw = Vec::new();
    let mut inf = Inflater::new(InflateWrap::Zlib, 15);
    let expected_len: usize = if h.interlace == 1 {
        adam7_passes(h.width as usize, h.height as usize)
            .iter()
            .map(|&(w, hh)| if w == 0 { 0 } else { (row_bytes(w) + 1) * hh })
            .sum()
    } else {
        (row_bytes(h.width as usize) + 1) * h.height as usize
    };
    let (_, st) = inf.inflate(&idat, &mut raw, expected_len, InflateFlush::Finish);
    if let InflateStatus::DataError(_) = st {
        return err("IDAT: invalid distance too far back");
    }
    if raw.len() < expected_len {
        return err("Read Error: truncated data");
    }
    let w = h.width as usize;
    let hh = h.height as usize;
    let truecolor = !(h.ctype == 3 || (h.ctype == 0 && trns.is_none()));
    let mut im = if truecolor {
        Image::new_truecolor(w as i32, hh as i32)
    } else {
        Image::new_palette(w as i32, hh as i32)
    }
    .ok_or_else(|| PngError("image too large".into()))?;
    if h.interlace == 1 {
        im.interlace = true;
    }
    if let Some((x, y, unit)) = phys
        && unit == 1
    {
        im.res_x = (x as f64 * 0.0254 + 0.5) as u32;
        im.res_y = (y as f64 * 0.0254 + 0.5) as u32;
    }
    // Palette setup.
    let mut trns_color: Option<(u16, u16, u16)> = None;
    match h.ctype {
        3 => {
            for (i, p) in palette.iter().enumerate().take(256) {
                im.red[i] = p[0] as i32;
                im.green[i] = p[1] as i32;
                im.blue[i] = p[2] as i32;
                im.alpha[i] = 0;
                im.open[i] = false;
            }
            im.colors_total = palette.len().min(256) as i32;
            if let Some(t) = &trns {
                // A single fully transparent entry (the rest opaque)
                // becomes the transparent color; other alpha values map to
                // GD's 7-bit alpha.
                let mut first_transparent = None;
                let mut only_simple = true;
                for (i, &a) in t.iter().enumerate().take(im.colors_total as usize) {
                    if a == 0 {
                        if first_transparent.is_none() {
                            first_transparent = Some(i);
                        } else {
                            only_simple = false;
                        }
                    } else if a != 255 {
                        only_simple = false;
                    }
                }
                if only_simple {
                    if let Some(i) = first_transparent {
                        im.transparent = i as i32;
                    }
                } else {
                    for (i, &a) in t.iter().enumerate().take(im.colors_total as usize) {
                        im.alpha[i] = 127 - (a as i32 >> 1);
                    }
                }
            }
        }
        0 if !truecolor => {
            let n = 1usize << h.depth.min(8);
            for i in 0..n {
                let v = (i * 255 / (n - 1)) as i32;
                im.red[i] = v;
                im.green[i] = v;
                im.blue[i] = v;
                im.open[i] = false;
            }
            im.colors_total = n as i32;
        }
        0 | 2 => {
            if let Some(t) = &trns {
                if h.ctype == 0 && t.len() >= 2 {
                    let g = u16::from_be_bytes([t[0], t[1]]);
                    trns_color = Some((g, g, g));
                } else if t.len() >= 6 {
                    trns_color = Some((
                        u16::from_be_bytes([t[0], t[1]]),
                        u16::from_be_bytes([t[2], t[3]]),
                        u16::from_be_bytes([t[4], t[5]]),
                    ));
                }
            }
        }
        _ => {}
    }
    if let Some((r, g, b)) = trns_color {
        let s = |v: u16| -> i32 {
            if h.depth == 16 {
                (v >> 8) as i32
            } else if h.depth < 8 {
                let max = (1u32 << h.depth) - 1;
                (v as u32 * 255 / max) as i32
            } else {
                v as i32 & 0xff
            }
        };
        im.transparent = tc(s(r), s(g), s(b), 0);
    }
    // Unfilter and store.
    let mut store = |im: &mut Image, x: usize, y: usize, samples: &[u16; 4]| {
        let scale = |v: u16| -> i32 {
            match h.depth {
                16 => (v >> 8) as i32,
                8 => v as i32,
                d => {
                    let max = (1u32 << d) - 1;
                    (v as u32 * 255 / max) as i32
                }
            }
        };
        let a7 = |a: i32| 127 - (a >> 1);
        let c = match h.ctype {
            3 => samples[0] as i32,
            0 if !truecolor => {
                if h.depth == 16 {
                    (samples[0] >> 8) as i32
                } else {
                    samples[0] as i32
                }
            }
            0 => {
                let g = scale(samples[0]);
                tc(g, g, g, 0)
            }
            2 => tc(scale(samples[0]), scale(samples[1]), scale(samples[2]), 0),
            4 => {
                let g = scale(samples[0]);
                tc(g, g, g, a7(scale(samples[1])))
            }
            _ => tc(
                scale(samples[0]),
                scale(samples[1]),
                scale(samples[2]),
                a7(scale(samples[3])),
            ),
        };
        im.put_raw(x as i32, y as i32, c);
    };
    let bpp = bpp_bits.div_ceil(8).max(1);
    let mut off = 0usize;
    let passes: Vec<(usize, usize, usize, usize, usize, usize)> = if h.interlace == 1 {
        // (x0, y0, dx, dy) per Adam7 pass.
        let p = [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ];
        p.iter()
            .map(|&(x0, y0, dx, dy)| {
                let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
                let ph = if hh > y0 { (hh - y0).div_ceil(dy) } else { 0 };
                (x0, y0, dx, dy, pw, ph)
            })
            .collect()
    } else {
        vec![(0, 0, 1, 1, w, hh)]
    };
    for (x0, y0, dx, dy, pw, ph) in passes {
        if pw == 0 || ph == 0 {
            continue;
        }
        let rb = row_bytes(pw);
        let mut prev = vec![0u8; rb];
        let mut cur = vec![0u8; rb];
        for py in 0..ph {
            let ft = raw[off];
            cur.copy_from_slice(&raw[off + 1..off + 1 + rb]);
            off += rb + 1;
            unfilter(ft, &mut cur, &prev, bpp).map_err(PngError)?;
            let y = y0 + py * dy;
            for px in 0..pw {
                let x = x0 + px * dx;
                let mut s = [0u16; 4];
                for (c, sv) in s.iter_mut().enumerate().take(ch) {
                    *sv = sample(&cur, px * ch + c, h.depth);
                }
                store(&mut im, x, y, &s);
            }
            std::mem::swap(&mut prev, &mut cur);
        }
    }
    Ok(im)
}

fn adam7_passes(w: usize, h: usize) -> Vec<(usize, usize)> {
    let p = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    p.iter()
        .map(|&(x0, y0, dx, dy)| {
            let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
            let ph = if h > y0 { (h - y0).div_ceil(dy) } else { 0 };
            (pw, if pw == 0 { 0 } else { ph })
        })
        .collect()
}

#[inline]
fn sample(row: &[u8], i: usize, depth: u8) -> u16 {
    match depth {
        8 => row[i] as u16,
        16 => u16::from_be_bytes([row[2 * i], row[2 * i + 1]]),
        d => {
            let d = d as usize;
            let bit = i * d;
            let byte = row[bit / 8];
            let shift = 8 - d - (bit % 8);
            ((byte >> shift) & ((1u8 << d) - 1)) as u16
        }
    }
}

fn unfilter(ft: u8, cur: &mut [u8], prev: &[u8], bpp: usize) -> Result<(), String> {
    match ft {
        0 => {}
        1 => {
            for i in bpp..cur.len() {
                cur[i] = cur[i].wrapping_add(cur[i - bpp]);
            }
        }
        2 => {
            for i in 0..cur.len() {
                cur[i] = cur[i].wrapping_add(prev[i]);
            }
        }
        3 => {
            for i in 0..cur.len() {
                let a = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                cur[i] = cur[i].wrapping_add(((a + prev[i] as u16) / 2) as u8);
            }
        }
        4 => {
            for i in 0..cur.len() {
                let a = if i >= bpp { cur[i - bpp] as i16 } else { 0 };
                let b = prev[i] as i16;
                let c = if i >= bpp { prev[i - bpp] as i16 } else { 0 };
                let p = a + b - c;
                let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                let pr = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                cur[i] = cur[i].wrapping_add(pr as u8);
            }
        }
        _ => return Err("bad adaptive filter value".into()),
    }
    Ok(())
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&crc32(&[kind, body]).to_be_bytes());
}

/// PNG filter selection (PNG_ALL_FILTERS etc. bit flags as in PHP's
/// PNG_FILTER_* constants); 0 means the default choice.
pub const PNG_NO_FILTER: i32 = 0x00;
pub const PNG_FILTER_NONE: i32 = 0x08;
pub const PNG_FILTER_SUB: i32 = 0x10;
pub const PNG_FILTER_UP: i32 = 0x20;
pub const PNG_FILTER_AVG: i32 = 0x40;
pub const PNG_FILTER_PAETH: i32 = 0x80;
pub const PNG_ALL_FILTERS: i32 = 0xf8;

/// Writes a GD image as PNG. `level` is the zlib level (-1 default),
/// `filters` the PNG_FILTER_* mask (-1: default).
pub fn encode(im: &Image, level: i32, filters: i32) -> Vec<u8> {
    let w = im.sx as usize;
    let h = im.sy as usize;
    let mut out = Vec::with_capacity(w * h / 2 + 128);
    out.extend_from_slice(SIGNATURE);
    let (ctype, depth): (u8, u8);
    let mut plte = Vec::new();
    let mut trns = Vec::new();
    // Palette remap: indices as stored.
    if im.truecolor {
        ctype = if im.save_alpha { 6 } else { 2 };
        depth = 8;
        if !im.save_alpha && im.transparent >= 0 {
            let t = im.transparent;
            for v in [(t >> 16) & 0xff, (t >> 8) & 0xff, t & 0xff] {
                trns.extend_from_slice(&(v as u16).to_be_bytes());
            }
        }
    } else {
        ctype = 3;
        let n = im.colors_total.max(1) as usize;
        depth = if n <= 2 {
            1
        } else if n <= 4 {
            2
        } else if n <= 16 {
            4
        } else {
            8
        };
        let mut last_alpha = None;
        for i in 0..n {
            plte.extend_from_slice(&[im.red[i] as u8, im.green[i] as u8, im.blue[i] as u8]);
            let a = if im.transparent == i as i32 {
                0
            } else {
                alpha8(im.alpha[i])
            };
            if a != 255 {
                last_alpha = Some(i);
            }
        }
        if let Some(last) = last_alpha {
            for i in 0..=last {
                trns.push(if im.transparent == i as i32 {
                    0
                } else {
                    alpha8(im.alpha[i])
                });
            }
        }
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[depth, ctype, 0, 0, im.interlace as u8]);
    chunk(&mut out, b"IHDR", &ihdr);
    // pHYs: the resolution in pixels per meter.
    let ppm = |dpi: u32| ((dpi as f64 / 0.0254) + 0.5) as u32;
    let mut phys = Vec::with_capacity(9);
    phys.extend_from_slice(&ppm(im.res_x).to_be_bytes());
    phys.extend_from_slice(&ppm(im.res_y).to_be_bytes());
    phys.push(1);
    chunk(&mut out, b"pHYs", &phys);
    if !plte.is_empty() {
        chunk(&mut out, b"PLTE", &plte);
    }
    if !trns.is_empty() {
        chunk(&mut out, b"tRNS", &trns);
    }
    let ch = channels(ctype);
    let bpp = (ch * depth as usize).div_ceil(8).max(1);
    let row_bytes = |pw: usize| (pw * ch * depth as usize).div_ceil(8);
    let mut raw = Vec::with_capacity((row_bytes(w) + 1) * h);
    let pack_row = |y: usize, xs: &mut dyn Iterator<Item = usize>, buf: &mut Vec<u8>| {
        buf.clear();
        if im.truecolor {
            for x in xs {
                let c = im.tpixels[y * w + x];
                buf.push(((c >> 16) & 0xff) as u8);
                buf.push(((c >> 8) & 0xff) as u8);
                buf.push((c & 0xff) as u8);
                if ctype == 6 {
                    buf.push(alpha8((c >> 24) & 0x7f));
                }
            }
        } else {
            let d = depth as usize;
            let mut acc = 0u8;
            let mut nb = 0;
            for x in xs {
                let v = im.pixels[y * w + x];
                if d == 8 {
                    buf.push(v);
                    continue;
                }
                acc |= (v & ((1u8 << d) - 1)) << (8 - d - nb);
                nb += d;
                if nb == 8 {
                    buf.push(acc);
                    acc = 0;
                    nb = 0;
                }
            }
            if nb > 0 {
                buf.push(acc);
            }
        }
    };
    let mut cur = Vec::new();
    let passes: Vec<(usize, usize, usize, usize)> = if im.interlace {
        vec![
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    } else {
        vec![(0, 0, 1, 1)]
    };
    let filter_mask = if filters < 0 || filters == 0 {
        // Default: no filtering for palette images and depths < 8,
        // adaptive filtering otherwise (PNG spec 12.8 recommendation).
        if ctype == 3 || depth < 8 {
            PNG_FILTER_NONE
        } else {
            PNG_ALL_FILTERS
        }
    } else {
        filters
    };
    for (x0, y0, dx, dy) in passes {
        if x0 >= w || y0 >= h {
            continue;
        }
        let pw = (w - x0).div_ceil(dx);
        let mut prev = vec![0u8; row_bytes(pw)];
        let mut y = y0;
        while y < h {
            let mut xs = (x0..w).step_by(dx);
            pack_row(y, &mut xs, &mut cur);
            filter_row(&cur, &prev, bpp, filter_mask, &mut raw);
            std::mem::swap(&mut prev, &mut cur);
            y += dy;
        }
    }
    let lvl = if !(0..=9).contains(&level) { 6 } else { level };
    let z = goro_ext_zlib::compress(&raw, lvl, Wrap::Zlib);
    // Split IDAT at 8 KiB like common encoders.
    for part in z.chunks(8192) {
        chunk(&mut out, b"IDAT", part);
    }
    if z.is_empty() {
        chunk(&mut out, b"IDAT", &[]);
    }
    chunk(&mut out, b"IEND", &[]);
    out
}

/// GD's 7-bit alpha (0 opaque) to PNG's 8-bit alpha (255 opaque).
#[inline]
pub fn alpha8(a: i32) -> u8 {
    if a == 127 {
        0
    } else {
        (255 - ((a << 1) + (a >> 6))) as u8
    }
}

fn filter_row(cur: &[u8], prev: &[u8], bpp: usize, mask: i32, out: &mut Vec<u8>) {
    let n = cur.len();
    let candidates: Vec<u8> = [
        (PNG_FILTER_NONE, 0u8),
        (PNG_FILTER_SUB, 1),
        (PNG_FILTER_UP, 2),
        (PNG_FILTER_AVG, 3),
        (PNG_FILTER_PAETH, 4),
    ]
    .iter()
    .filter(|(m, _)| mask & m != 0)
    .map(|&(_, t)| t)
    .collect();
    let candidates = if candidates.is_empty() {
        vec![0]
    } else {
        candidates
    };
    let apply = |t: u8, buf: &mut Vec<u8>| {
        for i in 0..n {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let p = match t {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                _ => {
                    let (ia, ib, ic) = (a as i16, b as i16, c as i16);
                    let p = ia + ib - ic;
                    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
            };
            buf.push(cur[i].wrapping_sub(p));
        }
    };
    if candidates.len() == 1 {
        out.push(candidates[0]);
        apply(candidates[0], out);
        return;
    }
    // Minimum sum of absolute differences heuristic.
    let mut best: Option<(u64, u8, Vec<u8>)> = None;
    for &t in &candidates {
        let mut buf = Vec::with_capacity(n);
        apply(t, &mut buf);
        let s: u64 = buf.iter().map(|&v| (v as i8).unsigned_abs() as u64).sum();
        if best.as_ref().is_none_or(|b| s < b.0) {
            best = Some((s, t, buf));
        }
    }
    let (_, t, buf) = best.unwrap();
    out.push(t);
    out.extend_from_slice(&buf);
}
