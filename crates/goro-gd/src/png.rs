//! gd_png.c on top of a small PNG codec that reproduces what libpng 1.6
//! does for libgd: the chunk layout, the adaptive filter selection, the
//! zlib parameters (incl. the reduced window of small images and the CMF
//! byte optimization) and the IDAT chunking, so that the files are
//! byte-identical to php-src's.

use crate::io::{In, Out};
use crate::*;
use goro_zlib::{Deflate, Inflate, Io};

const SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

const COLOR_GRAY: u8 = 0;
const COLOR_RGB: u8 = 2;
const COLOR_PALETTE: u8 = 3;
const COLOR_GRAY_ALPHA: u8 = 4;
const COLOR_RGB_ALPHA: u8 = 6;

pub const PNG_NO_FILTER: i32 = 0x00;
pub const PNG_FILTER_NONE: i32 = 0x08;
pub const PNG_FILTER_SUB: i32 = 0x10;
pub const PNG_FILTER_UP: i32 = 0x20;
pub const PNG_FILTER_AVG: i32 = 0x40;
pub const PNG_FILTER_PAETH: i32 = 0x80;
pub const PNG_ALL_FILTERS: i32 = 0xf8;

/// libpng's version string (gdPngGetVersionString())
pub const LIBPNG_VERSION: &str = "1.6.43";

fn dpm2dpi(dpm: u32) -> u32 {
    (dpm as f64 * 0.0254 + 0.5) as u32
}

fn dpi2dpm(dpi: u32) -> u32 {
    (dpi as f64 / 0.0254 + 0.5) as u32
}

/// Adam7 pass geometry: (x start, y start, x step, y step)
const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

fn pass_cols(w: u32, pass: usize) -> u32 {
    let (xs, _, dx, _) = ADAM7[pass];
    if w <= xs { 0 } else { (w - xs).div_ceil(dx) }
}

fn pass_rows(h: u32, pass: usize) -> u32 {
    let (_, ys, _, dy) = ADAM7[pass];
    if h <= ys { 0 } else { (h - ys).div_ceil(dy) }
}

fn row_bytes(pixel_depth: u32, width: u32) -> usize {
    if pixel_depth >= 8 {
        width as usize * (pixel_depth as usize / 8)
    } else {
        (width as usize * pixel_depth as usize).div_ceil(8)
    }
}

enum PngErr {
    /// png_error(): "gd-png:  fatal libpng error: ..."
    Fatal(String),
}

fn fatal(msg: impl Into<String>) -> PngErr {
    PngErr::Fatal(msg.into())
}

struct Info {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
    palette: Vec<[u8; 3]>,
    trans: Option<Vec<u8>>,
    phys: Option<(u32, u32, u8)>,
    idat: Vec<u8>,
}

fn read_chunks(inp: &mut In) -> Result<Info, PngErr> {
    let mut info = Info {
        width: 0,
        height: 0,
        bit_depth: 0,
        color_type: 0,
        interlace: 0,
        palette: Vec::new(),
        trans: None,
        phys: None,
        idat: Vec::new(),
    };
    let mut have_ihdr = false;
    let mut have_plte = false;
    let mut have_idat = false;
    let mut after_idat = false;
    loop {
        let mut hdr = [0u8; 8];
        if inp.get_buf(&mut hdr) != 8 {
            return Err(fatal("Read Error: truncated data"));
        }
        let len = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let name = [hdr[4], hdr[5], hdr[6], hdr[7]];
        let cname = String::from_utf8_lossy(&name).into_owned();
        if len > 0x7fffffff {
            return Err(fatal(format!(
                "{}: PNG unsigned integer out of range",
                cname
            )));
        }
        for &c in &name {
            if !c.is_ascii_alphabetic() {
                return Err(fatal(format!(
                    "{}: invalid chunk type",
                    chunk_display(&name)
                )));
            }
        }
        let mut data = vec![0u8; len as usize];
        if len > 0 && inp.get_buf(&mut data) != len as i32 {
            return Err(fatal("Read Error: truncated data"));
        }
        let mut crcb = [0u8; 4];
        if inp.get_buf(&mut crcb) != 4 {
            return Err(fatal("Read Error: truncated data"));
        }
        let crc = goro_zlib::crc32(goro_zlib::crc32(0, &name), &data);
        let crc_ok = crc == u32::from_be_bytes(crcb);
        let ancillary = name[0] & 0x20 != 0;
        if !crc_ok {
            if ancillary {
                gd_error_ex(
                    GD_WARNING,
                    format!("gd-png: libpng warning: {}: CRC error", cname),
                );
                continue;
            }
            return Err(fatal(format!("{}: CRC error", cname)));
        }
        if !have_ihdr && &name != b"IHDR" {
            return Err(fatal(format!("{}: missing IHDR", cname)));
        }
        if have_idat && &name != b"IDAT" {
            after_idat = true;
        }
        match &name {
            b"IHDR" => {
                if have_ihdr {
                    return Err(fatal("IHDR: out of place"));
                }
                if len != 13 {
                    return Err(fatal("IHDR: invalid"));
                }
                have_ihdr = true;
                info.width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                info.height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                info.bit_depth = data[8];
                info.color_type = data[9];
                let compression = data[10];
                let filter = data[11];
                info.interlace = data[12];
                check_ihdr(&info, compression, filter)?;
            }
            b"PLTE" => {
                if have_plte {
                    return Err(fatal("PLTE: duplicate"));
                }
                if have_idat {
                    return Err(fatal("PLTE: out of place"));
                }
                if info.color_type & 2 == 0 {
                    // PLTE in a grayscale image: ignored with an error
                    gd_error_ex(
                        GD_WARNING,
                        "gd-png: libpng warning: PLTE: ignored in grayscale PNG",
                    );
                    continue;
                }
                let n = len / 3;
                let max = if info.color_type == COLOR_PALETTE {
                    1u32 << info.bit_depth
                } else {
                    256
                };
                if len % 3 != 0 || n == 0 || n > 256 {
                    if info.color_type == COLOR_PALETTE {
                        return Err(fatal("PLTE: invalid"));
                    }
                    gd_error_ex(GD_WARNING, "gd-png: libpng warning: PLTE: invalid");
                    continue;
                }
                have_plte = true;
                let n = n.min(max);
                info.palette = (0..n as usize)
                    .map(|i| [data[i * 3], data[i * 3 + 1], data[i * 3 + 2]])
                    .collect();
            }
            b"tRNS" => {
                if have_idat {
                    gd_error_ex(GD_WARNING, "gd-png: libpng warning: tRNS: out of place");
                    continue;
                }
                match info.color_type {
                    COLOR_GRAY => {
                        if len != 2 {
                            gd_error_ex(GD_WARNING, "gd-png: libpng warning: tRNS: invalid");
                            continue;
                        }
                    }
                    COLOR_RGB => {
                        if len != 6 {
                            gd_error_ex(GD_WARNING, "gd-png: libpng warning: tRNS: invalid");
                            continue;
                        }
                    }
                    COLOR_PALETTE => {
                        if !have_plte {
                            gd_error_ex(GD_WARNING, "gd-png: libpng warning: tRNS: out of place");
                            continue;
                        }
                        if len as usize > info.palette.len() || len == 0 {
                            gd_error_ex(GD_WARNING, "gd-png: libpng warning: tRNS: invalid");
                            continue;
                        }
                    }
                    _ => {
                        gd_error_ex(
                            GD_WARNING,
                            "gd-png: libpng warning: tRNS: invalid with alpha channel",
                        );
                        continue;
                    }
                }
                info.trans = Some(data);
            }
            b"pHYs" => {
                if len == 9 {
                    info.phys = Some((
                        u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
                        u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
                        data[8],
                    ));
                }
            }
            b"IDAT" => {
                if info.color_type == COLOR_PALETTE && !have_plte {
                    return Err(fatal("Missing PLTE before IDAT"));
                }
                if after_idat {
                    // libpng: "Too many IDATs found" (benign)
                    gd_error_ex(GD_WARNING, "gd-png: libpng warning: Too many IDATs found");
                    continue;
                }
                have_idat = true;
                info.idat.extend_from_slice(&data);
            }
            b"IEND" => {
                if !have_idat {
                    return Err(fatal("Not enough image data"));
                }
                return Ok(info);
            }
            _ => {
                if !ancillary {
                    return Err(fatal(format!("{}: unknown critical chunk", cname)));
                }
            }
        }
    }
}

fn chunk_display(name: &[u8; 4]) -> String {
    let mut s = String::new();
    for &c in name {
        if c.is_ascii_alphabetic() {
            s.push(c as char);
        } else {
            s.push_str(&format!("[{:02X}]", c));
        }
    }
    s
}

fn check_ihdr(info: &Info, compression: u8, filter: u8) -> Result<(), PngErr> {
    let mut bad = false;
    let w = |m: &str| gd_error_ex(GD_WARNING, format!("gd-png: libpng warning: {}", m));
    if info.width == 0 {
        w("Image width is zero in IHDR");
        bad = true;
    }
    if info.width > 0x7fffffff {
        w("Invalid image width in IHDR");
        bad = true;
    }
    if info.width > 1000000 {
        w("Image width exceeds user limit in IHDR");
        bad = true;
    }
    if info.height == 0 {
        w("Image height is zero in IHDR");
        bad = true;
    }
    if info.height > 0x7fffffff {
        w("Invalid image height in IHDR");
        bad = true;
    }
    if info.height > 1000000 {
        w("Image height exceeds user limit in IHDR");
        bad = true;
    }
    let bd = info.bit_depth;
    if !matches!(bd, 1 | 2 | 4 | 8 | 16) {
        w("Invalid bit depth in IHDR");
        bad = true;
    }
    let ct = info.color_type;
    if ct == 1 || ct == 5 || ct > 6 {
        w("Invalid color type in IHDR");
        bad = true;
    }
    if (ct == COLOR_PALETTE && bd > 8)
        || ((ct == COLOR_RGB || ct == COLOR_GRAY_ALPHA || ct == COLOR_RGB_ALPHA) && bd < 8)
    {
        w("Invalid color type/bit depth combination in IHDR");
        bad = true;
    }
    if info.interlace > 1 {
        w("Unknown interlace method in IHDR");
        bad = true;
    }
    if compression != 0 {
        w("Unknown compression method in IHDR");
        bad = true;
    }
    if filter != 0 {
        w("Unknown filter method in IHDR");
        bad = true;
    }
    if bad {
        return Err(fatal("Invalid IHDR data"));
    }
    Ok(())
}

fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = b - c;
    let pc = a - c;
    let pa = p.abs();
    let pb = pc.abs();
    let pc = (p + pc).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn unfilter(ft: u8, row: &mut [u8], prev: &[u8], bpp: usize) -> Result<(), PngErr> {
    let n = row.len();
    match ft {
        0 => {}
        1 => {
            for i in bpp..n {
                row[i] = row[i].wrapping_add(row[i - bpp]);
            }
        }
        2 => {
            for i in 0..n {
                row[i] = row[i].wrapping_add(prev[i]);
            }
        }
        3 => {
            for i in 0..n {
                let a = if i >= bpp { row[i - bpp] as u32 } else { 0 };
                row[i] = row[i].wrapping_add(((a + prev[i] as u32) >> 1) as u8);
            }
        }
        4 => {
            for i in 0..n {
                let a = if i >= bpp { row[i - bpp] as i32 } else { 0 };
                let c = if i >= bpp { prev[i - bpp] as i32 } else { 0 };
                row[i] = row[i].wrapping_add(paeth(a, prev[i] as i32, c) as u8);
            }
        }
        _ => return Err(fatal("bad adaptive filter value")),
    }
    Ok(())
}

/// Expand a raw (unfiltered) row into 8-bit samples (png_set_packing and
/// png_set_strip_16)
fn expand_row(raw: &[u8], width: usize, bit_depth: u8, channels: usize, out: &mut Vec<u8>) {
    match bit_depth {
        16 => {
            for i in 0..width * channels {
                out.push(raw[i * 2]);
            }
        }
        8 => out.extend_from_slice(&raw[..width * channels]),
        bd => {
            let bd = bd as usize;
            let mask = (1u16 << bd) - 1;
            for i in 0..width {
                let bit = i * bd;
                let byte = raw[bit / 8];
                let shift = 8 - bd - (bit % 8);
                out.push(((byte as u16 >> shift) & mask) as u8);
            }
        }
    }
}

fn decode_pixels(info: &Info) -> Result<Vec<Vec<u8>>, PngErr> {
    let channels = match info.color_type {
        COLOR_GRAY | COLOR_PALETTE => 1,
        COLOR_RGB => 3,
        COLOR_GRAY_ALPHA => 2,
        _ => 4,
    };
    let pixel_depth = channels as u32 * info.bit_depth as u32;
    let bpp = (pixel_depth as usize).div_ceil(8).max(1);
    let (w, h) = (info.width, info.height);
    // total inflated size expected
    let passes: Vec<(usize, u32, u32)> = if info.interlace == 1 {
        (0..7)
            .filter_map(|p| {
                let pw = pass_cols(w, p);
                let ph = pass_rows(h, p);
                if pw == 0 || ph == 0 {
                    None
                } else {
                    Some((p, pw, ph))
                }
            })
            .collect()
    } else {
        vec![(usize::MAX, w, h)]
    };
    let total: usize = passes
        .iter()
        .map(|&(_, pw, ph)| (row_bytes(pixel_depth, pw) + 1) * ph as usize)
        .sum();
    let data = inflate_idat(&info.idat, total)?;
    let mut out_rows: Vec<Vec<u8>> = vec![vec![0u8; w as usize * channels]; h as usize];
    let mut pos = 0usize;
    for &(p, pw, ph) in &passes {
        let rb = row_bytes(pixel_depth, pw);
        let mut prev = vec![0u8; rb];
        let mut row = vec![0u8; rb];
        let mut px = Vec::with_capacity(pw as usize * channels);
        for r in 0..ph {
            let ft = data[pos];
            row.copy_from_slice(&data[pos + 1..pos + 1 + rb]);
            pos += rb + 1;
            unfilter(ft, &mut row, &prev, bpp)?;
            px.clear();
            expand_row(&row, pw as usize, info.bit_depth, channels, &mut px);
            if p == usize::MAX {
                out_rows[r as usize].copy_from_slice(&px);
            } else {
                let (xs, ys, dx, dy) = ADAM7[p];
                let y = (ys + r * dy) as usize;
                for c in 0..pw as usize {
                    let x = xs as usize + c * dx as usize;
                    out_rows[y][x * channels..(x + 1) * channels]
                        .copy_from_slice(&px[c * channels..(c + 1) * channels]);
                }
            }
            std::mem::swap(&mut prev, &mut row);
        }
    }
    Ok(out_rows)
}

fn inflate_idat(idat: &[u8], total: usize) -> Result<Vec<u8>, PngErr> {
    let mut inf = Inflate::new(15).map_err(|_| fatal("zlib failed to initialize compressor"))?;
    let mut out = vec![0u8; total];
    let mut io = Io::new(idat, &mut out);
    let r = inf.inflate(&mut io, goro_zlib::Z_NO_FLUSH);
    let produced = io.out_pos;
    if r != goro_zlib::Z_OK
        && r != goro_zlib::Z_STREAM_END
        && !(r == goro_zlib::Z_BUF_ERROR && produced == total)
    {
        let msg = inf.msg.unwrap_or(match r {
            goro_zlib::Z_DATA_ERROR => "damaged LZ stream",
            goro_zlib::Z_BUF_ERROR => "truncated",
            _ => "unexpected zlib return code",
        });
        if produced < total {
            return Err(fatal(format!("IDAT: {}", msg)));
        }
    }
    if produced < total {
        return Err(fatal("Not enough image data"));
    }
    Ok(out)
}

/// gdImageCreateFromPngCtx()
pub fn create_from_png(data: &[u8]) -> Option<Image> {
    let mut inp = In::new(data);
    let mut sig = [0u8; 8];
    if inp.get_buf(&mut sig) < 8 {
        return None;
    }
    if sig != SIG {
        return None;
    }
    let info = match read_chunks(&mut inp) {
        Ok(i) => i,
        Err(PngErr::Fatal(m)) => {
            gd_error_ex(GD_WARNING, format!("gd-png:  fatal libpng error: {}", m));
            gd_error("gd-png error: setjmp returns error condition");
            return None;
        }
    };
    let ct = info.color_type;
    let mut im = if ct == COLOR_RGB || ct == COLOR_RGB_ALPHA || ct == COLOR_GRAY_ALPHA {
        Image::create_true_color(info.width as i32, info.height as i32)
    } else {
        Image::create(info.width as i32, info.height as i32)
    };
    let Some(mut im) = im.take() else {
        gd_error("gd-png error: cannot allocate gdImage struct");
        return None;
    };
    if let Some((rx, ry, unit)) = info.phys {
        if unit == 1 {
            im.res_x = dpm2dpi(rx);
            im.res_y = dpm2dpi(ry);
        }
    }
    let mut transparent = -1;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let num_palette;
    match ct {
        COLOR_PALETTE => {
            palette = info.palette.clone();
            num_palette = palette.len() as i32;
            if let Some(trans) = &info.trans {
                let mut first_zero = true;
                for (i, &t) in trans.iter().enumerate() {
                    im.alpha[i] = ALPHA_MAX - (t as i32 >> 1);
                    if t == 0 && first_zero {
                        transparent = i as i32;
                        first_zero = false;
                    }
                }
            }
        }
        COLOR_GRAY => {
            if info.bit_depth < 8 {
                num_palette = 1 << info.bit_depth;
                for i in 0..256 {
                    let j = (255 * i) / (num_palette - 1);
                    palette.push([j as u8, j as u8, j as u8]);
                }
            } else {
                num_palette = 256;
                for i in 0..256 {
                    palette.push([i as u8, i as u8, i as u8]);
                }
            }
            if let Some(t) = &info.trans {
                let g = u16::from_be_bytes([t[0], t[1]]) as i32;
                transparent = if info.bit_depth == 16 { g >> 8 } else { g };
            }
        }
        _ => {
            num_palette = 0;
            if let Some(t) = &info.trans {
                if t.len() >= 6 {
                    let r = u16::from_be_bytes([t[0], t[1]]) as i32;
                    let g = u16::from_be_bytes([t[2], t[3]]) as i32;
                    let b = u16::from_be_bytes([t[4], t[5]]) as i32;
                    transparent = if info.bit_depth == 16 {
                        true_color(r >> 8, g >> 8, b >> 8)
                    } else {
                        true_color(r, g, b)
                    };
                }
            }
        }
    }
    let rows = match decode_pixels(&info) {
        Ok(r) => r,
        Err(PngErr::Fatal(m)) => {
            gd_error_ex(GD_WARNING, format!("gd-png:  fatal libpng error: {}", m));
            gd_error("gd-png error: setjmp returns error condition");
            return None;
        }
    };
    if !im.true_color {
        im.colors_total = num_palette;
        for i in 0..num_palette as usize {
            im.red[i] = palette[i][0] as i32;
            im.green[i] = palette[i][1] as i32;
            im.blue[i] = palette[i][2] as i32;
            im.open[i] = 1;
        }
        for i in num_palette as usize..MAX_COLORS {
            im.open[i] = 1;
        }
    }
    im.transparent = transparent;
    im.interlace = (info.interlace == 1) as i32;
    let (w, h) = (info.width as usize, info.height as usize);
    match ct {
        COLOR_RGB => {
            for y in 0..h {
                let r = &rows[y];
                for x in 0..w {
                    im.tpixels[y][x] =
                        true_color(r[x * 3] as i32, r[x * 3 + 1] as i32, r[x * 3 + 2] as i32);
                }
            }
        }
        COLOR_RGB_ALPHA => {
            for y in 0..h {
                let r = &rows[y];
                for x in 0..w {
                    let a = ALPHA_MAX - (r[x * 4 + 3] as i32 >> 1);
                    im.tpixels[y][x] = true_color_alpha(
                        r[x * 4] as i32,
                        r[x * 4 + 1] as i32,
                        r[x * 4 + 2] as i32,
                        a,
                    );
                }
            }
        }
        COLOR_GRAY_ALPHA => {
            for y in 0..h {
                let r = &rows[y];
                for x in 0..w {
                    let g = r[x * 2] as i32;
                    let a = ALPHA_MAX - (r[x * 2 + 1] as i32 >> 1);
                    im.tpixels[y][x] = true_color_alpha(g, g, g, a);
                }
            }
        }
        _ => {
            for y in 0..h {
                for x in 0..w {
                    let idx = rows[y][x];
                    im.pixels[y][x] = idx;
                    im.open[idx as usize] = 0;
                }
            }
        }
    }
    Some(im)
}

fn write_chunk(out: &mut Out, name: &[u8; 4], data: &[u8]) {
    out.put_int(data.len() as i32);
    out.put_buf(name);
    out.put_buf(data);
    let crc = goro_zlib::crc32(goro_zlib::crc32(0, name), data);
    out.put_int(crc as i32);
}

/// png_write_find_filter(): the filtered row (filter type first)
fn filter_row(row: &[u8], prev: &[u8], bpp: usize, filters: i32) -> Vec<u8> {
    let n = row.len();
    let abs_sum = |v: u8| -> usize {
        if v < 128 {
            v as usize
        } else {
            256 - v as usize
        }
    };
    let make = |ft: u8| -> Vec<u8> {
        let mut o = Vec::with_capacity(n + 1);
        o.push(ft);
        for i in 0..n {
            let a = if i >= bpp { row[i - bpp] as i32 } else { 0 };
            let b = prev[i] as i32;
            let c = if i >= bpp { prev[i - bpp] as i32 } else { 0 };
            let x = row[i] as i32;
            let v = match ft {
                0 => x,
                1 => x - a,
                2 => x - b,
                3 => x - ((a + b) >> 1),
                _ => x - paeth(a, b, c),
            };
            o.push((v & 0xff) as u8);
        }
        o
    };
    let single = match filters {
        PNG_FILTER_NONE => Some(0),
        PNG_FILTER_SUB => Some(1),
        PNG_FILTER_UP => Some(2),
        PNG_FILTER_AVG => Some(3),
        PNG_FILTER_PAETH => Some(4),
        _ => None,
    };
    if let Some(ft) = single {
        return make(ft);
    }
    let mut best: Option<Vec<u8>> = None;
    let mut mins = usize::MAX - 256;
    if filters & PNG_FILTER_NONE != 0 {
        let sum: usize = row.iter().map(|&v| abs_sum(v)).sum();
        mins = sum;
        best = Some(make(0));
    }
    for (bit, ft) in [
        (PNG_FILTER_SUB, 1u8),
        (PNG_FILTER_UP, 2),
        (PNG_FILTER_AVG, 3),
        (PNG_FILTER_PAETH, 4),
    ] {
        if filters & bit == 0 {
            continue;
        }
        let cand = make(ft);
        // libpng stops summing once the sum exceeds the best so far
        // (after the first bpp bytes), which never changes the choice.
        let sum: usize = cand[1..].iter().map(|&v| abs_sum(v)).sum();
        if sum < mins {
            mins = sum;
            best = Some(cand);
        }
    }
    best.unwrap_or_else(|| make(0))
}

/// optimize_cmf() of pngwutil.c
fn optimize_cmf(data: &mut [u8], data_size: usize) {
    if data_size <= 16384 && data.len() >= 2 {
        let mut z_cmf = data[0] as u32;
        if (z_cmf & 0x0f) == 8 && (z_cmf & 0xf0) <= 0x70 {
            let mut z_cinfo = z_cmf >> 4;
            let mut half = 1u32 << (z_cinfo + 7);
            if data_size as u32 <= half {
                loop {
                    half >>= 1;
                    z_cinfo -= 1;
                    if !(z_cinfo > 0 && data_size as u32 <= half) {
                        break;
                    }
                }
                z_cmf = (z_cmf & 0x0f) | (z_cinfo << 4);
                data[0] = z_cmf as u8;
                let mut tmp = data[1] as u32 & 0xe0;
                tmp += 0x1f - ((z_cmf << 8) + tmp) % 0x1f;
                data[1] = tmp as u8;
            }
        }
    }
}

/// gdImagePngCtxEx()
pub fn image_png(im: &Image, level: i32, basefilter: i32) -> Option<Vec<u8>> {
    let width = im.sx;
    let height = im.sy;
    let mut transparent = im.transparent;
    let mut remap = false;
    let mut mapping = [-1i32; MAX_COLORS];
    let mut colors = im.colors_total;
    let mut bit_depth = 8u8;
    if level != -1 && !(0..=9).contains(&level) {
        gd_error("gd-png error: compression level must be 0 through 9");
        return None;
    }
    if !im.true_color {
        if transparent >= im.colors_total
            || (transparent >= 0 && im.open[transparent as usize] != 0)
        {
            transparent = -1;
        }
        let _ = transparent;
        colors = 0;
        for i in 0..im.colors_total as usize {
            if im.open[i] == 0 {
                mapping[i] = colors;
                colors += 1;
            }
        }
        if colors == 0 {
            gd_error("gd-png error: no colors in palette");
            return None;
        }
        if colors < im.colors_total {
            remap = true;
        }
        bit_depth = if colors <= 2 {
            1
        } else if colors <= 4 {
            2
        } else if colors <= 16 {
            4
        } else {
            8
        };
    }
    let interlaced = im.interlace != 0;
    let color_type = if im.true_color {
        if im.save_alpha_flag != 0 {
            COLOR_RGB_ALPHA
        } else {
            COLOR_RGB
        }
    } else {
        COLOR_PALETTE
    };
    let mut out = Out::new();
    out.put_buf(&SIG);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.push(bit_depth);
    ihdr.push(color_type);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(interlaced as u8);
    write_chunk(&mut out, b"IHDR", &ihdr);

    let mut trns: Option<Vec<u8>> = None;
    if im.true_color && im.save_alpha_flag == 0 && transparent >= 0 {
        let t = im.transparent;
        let mut v = Vec::new();
        for c in [
            true_color_get_red(t),
            true_color_get_green(t),
            true_color_get_blue(t),
        ] {
            v.extend_from_slice(&(c as u16).to_be_bytes());
        }
        trns = Some(v);
    }
    let mut plte: Option<Vec<u8>> = None;
    if !im.true_color {
        let mut tc = 0;
        for i in 0..im.colors_total as usize {
            if im.open[i] == 0 && im.alpha[i] != ALPHA_OPAQUE {
                tc += 1;
            }
        }
        if tc > 0 {
            remap = true;
            let mut j = 0;
            let mut k = colors - 1;
            let mut tv = vec![0u8; tc];
            for i in 0..im.colors_total as usize {
                if im.open[i] == 0 {
                    if im.alpha[i] != ALPHA_OPAQUE {
                        tv[j as usize] = (255 - ((im.alpha[i] << 1) + (im.alpha[i] >> 6))) as u8;
                        mapping[i] = j;
                        j += 1;
                    } else {
                        mapping[i] = k;
                        k -= 1;
                    }
                }
            }
            trns = Some(tv);
        }
        let mut pal = vec![0u8; colors as usize * 3];
        if remap {
            for i in 0..im.colors_total as usize {
                if mapping[i] < 0 {
                    continue;
                }
                let m = mapping[i] as usize;
                pal[m * 3] = im.red[i] as u8;
                pal[m * 3 + 1] = im.green[i] as u8;
                pal[m * 3 + 2] = im.blue[i] as u8;
            }
        } else {
            for i in 0..colors as usize {
                pal[i * 3] = im.red[i] as u8;
                pal[i * 3 + 1] = im.green[i] as u8;
                pal[i * 3 + 2] = im.blue[i] as u8;
            }
        }
        plte = Some(pal);
    }
    if let Some(p) = &plte {
        write_chunk(&mut out, b"PLTE", p);
    }
    if let Some(t) = &trns {
        write_chunk(&mut out, b"tRNS", t);
    }
    let mut phys = Vec::with_capacity(9);
    phys.extend_from_slice(&dpi2dpm(im.res_x).to_be_bytes());
    phys.extend_from_slice(&dpi2dpm(im.res_y).to_be_bytes());
    phys.push(1);
    write_chunk(&mut out, b"pHYs", &phys);

    // The rows as libpng sees them (packed for bit depths < 8)
    let channels: usize = match color_type {
        COLOR_RGB_ALPHA => 4,
        COLOR_RGB => 3,
        _ => 1,
    };
    let pixel_depth = channels as u32 * bit_depth as u32;
    let bpp = (pixel_depth as usize).div_ceil(8);
    let row_of = |y: usize| -> Vec<u8> {
        let mut r = Vec::with_capacity(width as usize * channels);
        if im.true_color {
            for &p in &im.tpixels[y] {
                r.push(true_color_get_red(p) as u8);
                r.push(true_color_get_green(p) as u8);
                r.push(true_color_get_blue(p) as u8);
                if channels == 4 {
                    let a = true_color_get_alpha(p);
                    r.push((255 - ((a << 1) + (a >> 6))) as u8);
                }
            }
        } else {
            for &p in &im.pixels[y] {
                r.push(if remap { mapping[p as usize] as u8 } else { p });
            }
        }
        r
    };
    let pack = |px: &[u8]| -> Vec<u8> {
        if bit_depth == 8 {
            return px.to_vec();
        }
        let bd = bit_depth as usize;
        let mut o = vec![0u8; (px.len() * bd).div_ceil(8)];
        for (i, &v) in px.iter().enumerate() {
            let bit = i * bd;
            let shift = 8 - bd - (bit % 8);
            o[bit / 8] |= ((v as usize & ((1 << bd) - 1)) << shift) as u8;
        }
        o
    };
    // do_filter: libpng's default unless a filter set was given
    let mut filters = if basefilter >= 0 {
        basefilter & 0xff
    } else {
        0
    };
    if filters == 0 {
        filters = if color_type == COLOR_PALETTE || bit_depth < 8 {
            PNG_FILTER_NONE
        } else {
            PNG_ALL_FILTERS
        };
    }
    let mut filtered: Vec<Vec<u8>> = Vec::new();
    let w = width as u32;
    let h = height as u32;
    if interlaced {
        let full: Vec<Vec<u8>> = (0..h as usize).map(row_of).collect();
        for p in 0..7 {
            let pw = pass_cols(w, p);
            let ph = pass_rows(h, p);
            if pw == 0 || ph == 0 {
                continue;
            }
            let (xs, ys, dx, dy) = ADAM7[p];
            let rb = row_bytes(pixel_depth, pw);
            let mut prev = vec![0u8; rb];
            for r in 0..ph {
                let y = (ys + r * dy) as usize;
                let mut px = Vec::with_capacity(pw as usize * channels);
                for c in 0..pw {
                    let x = (xs + c * dx) as usize;
                    px.extend_from_slice(&full[y][x * channels..(x + 1) * channels]);
                }
                let row = pack(&px);
                filtered.push(filter_row(&row, &prev, bpp.max(1), filters));
                prev = row;
            }
        }
    } else {
        let rb = row_bytes(pixel_depth, w);
        let mut prev = vec![0u8; rb];
        for y in 0..h as usize {
            let row = pack(&row_of(y));
            filtered.push(filter_row(&row, &prev, bpp.max(1), filters));
            prev = row;
        }
    }
    // png_deflate_claim()
    let data_size: usize = filtered.iter().map(|r| r.len()).sum();
    let mut window_bits = 15;
    if data_size <= 16384 {
        let mut half = 1usize << (window_bits - 1);
        while data_size + 262 <= half {
            half >>= 1;
            window_bits -= 1;
        }
    }
    if window_bits == 8 {
        window_bits = 9;
    }
    let strategy = if filters != PNG_FILTER_NONE {
        goro_zlib::Z_FILTERED
    } else {
        goro_zlib::Z_DEFAULT_STRATEGY
    };
    let mut z = Deflate::new(level, goro_zlib::Z_DEFLATED, window_bits, 8, strategy).ok()?;
    // png_compress_IDAT(): one deflate(Z_NO_FLUSH) per row into an
    // 8192-byte buffer written out as an IDAT whenever it fills up, then
    // deflate(Z_FINISH).
    let mut zbuf = vec![0u8; 8192];
    let mut zpos = 0usize;
    let mut first = true;
    let emit = |out: &mut Out, data: &mut [u8], first: &mut bool| {
        if *first {
            optimize_cmf(data, data_size);
            *first = false;
        }
        write_chunk(out, b"IDAT", data);
    };
    let empty: Vec<u8> = Vec::new();
    for (ri, row) in filtered.iter().chain(std::iter::once(&empty)).enumerate() {
        let flush = if ri == filtered.len() {
            goro_zlib::Z_FINISH
        } else {
            goro_zlib::Z_NO_FLUSH
        };
        let mut in_pos = 0usize;
        loop {
            let (r, consumed, produced) = {
                let mut io = Io::new(&row[in_pos..], &mut zbuf[zpos..]);
                let r = z.deflate(&mut io, flush);
                (r, io.in_pos, io.out_pos)
            };
            in_pos += consumed;
            zpos += produced;
            if zpos == zbuf.len() {
                emit(&mut out, &mut zbuf, &mut first);
                zpos = 0;
                continue;
            }
            if r == goro_zlib::Z_OK {
                if in_pos == row.len() && flush != goro_zlib::Z_FINISH {
                    break;
                }
                if flush == goro_zlib::Z_FINISH {
                    return None;
                }
            } else if r == goro_zlib::Z_STREAM_END && flush == goro_zlib::Z_FINISH {
                let mut last = zbuf[..zpos].to_vec();
                emit(&mut out, &mut last, &mut first);
                break;
            } else if r == goro_zlib::Z_BUF_ERROR
                && in_pos == row.len()
                && flush != goro_zlib::Z_FINISH
            {
                break;
            } else {
                return None;
            }
        }
    }
    write_chunk(&mut out, b"IEND", &[]);
    Some(out.into_vec())
}
