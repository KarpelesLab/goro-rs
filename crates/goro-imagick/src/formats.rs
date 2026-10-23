//! Readers for the formats beyond the common web ones: PCX, DDS, JPEG XL
//! and Radiance HDR on their OxideAV crates; MIFF, SGI, XPM and PSD
//! here, from their published descriptions (ImageMagick's MIFF page,
//! the SGI image file format, the XPM manual, Adobe's PSD specification).

use crate::codec::corrupt;
use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use crate::types::*;

fn rgba8_image(w: u32, h: u32, rgba: &[u8], alpha: bool) -> Image {
    let mut img = Image::blank(w as usize, h as usize);
    img.alpha = alpha;
    img.set_from_rgba8(rgba);
    img.depth = 8;
    img
}

fn has_alpha8(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|p| p[3] != 255)
}

pub fn decode_pcx(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_pcx::decode_rgba8(data).map_err(|e| corrupt("PCX", e))?;
    let alpha = has_alpha8(&im.data);
    Ok(vec![rgba8_image(im.width, im.height, &im.data, alpha)])
}

pub fn decode_dds(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_dds::decode_rgba8(data).map_err(|e| corrupt("DDS", e))?;
    let alpha = has_alpha8(&im.data);
    Ok(vec![rgba8_image(im.width, im.height, &im.data, alpha)])
}

pub fn decode_jxl(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_jpegxl::decode_rgba8(data).map_err(|e| corrupt("JXL", e))?;
    let alpha = has_alpha8(&im.data);
    Ok(vec![rgba8_image(im.width, im.height, &im.data, alpha)])
}

/// Radiance RGBE: linear floats, stored clamped at Q16, in the RGB
/// (linear) colourspace.
pub fn decode_hdr(data: &[u8]) -> MResult<Vec<Image>> {
    let im = oxideav_hdr::decode(data).map_err(|e| corrupt("HDR", e))?;
    let (w, h) = (im.width as usize, im.height as usize);
    let plane = &im.planes[0];
    let mut img = Image::blank(w, h);
    for y in 0..h {
        let row = &plane.data[y * plane.stride..];
        for x in 0..w {
            let f = |c: usize| {
                let o = (x * 3 + c) * 4;
                f32::from_le_bytes([row[o], row[o + 1], row[o + 2], row[o + 3]]) as f64
            };
            let q = |v: f64| crate::color::clamp_to_quantum(v * 65535.0) as u16;
            img.set_px(x, y, [q(f(0)), q(f(1)), q(f(2)), 65535]);
        }
    }
    img.depth = 32;
    img.colorspace = Colorspace::RGB;
    Ok(vec![img])
}

// ---------------------------------------------------------------------------
// MIFF

/// Inflate zlib data that may hold one stream or several back to back.
fn zlib_inflate(input: &[u8]) -> Option<Vec<u8>> {
    use goro_zlib::{Inflate, Io, Z_BUF_ERROR, Z_OK, Z_STREAM_END, Z_SYNC_FLUSH};
    let mut inf = Inflate::new(15).ok()?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    let mut pos = 0;
    loop {
        let mut io = Io::new(&input[pos..], &mut buf);
        let st = inf.inflate(&mut io, Z_SYNC_FLUSH);
        let (consumed, n) = (io.in_pos, io.out_pos);
        pos += consumed;
        out.extend_from_slice(&buf[..n]);
        match st {
            Z_STREAM_END if pos < input.len() => inf = Inflate::new(15).ok()?,
            Z_STREAM_END => return Some(out),
            Z_OK if n == 0 && consumed == 0 => return Some(out),
            Z_OK => {}
            Z_BUF_ERROR if pos >= input.len() => return Some(out),
            _ => return None,
        }
    }
}

/// The header's key=value pairs (braced values may hold spaces; a brace
/// group without a key is a comment), up to the ":" before ctrl-Z.
fn miff_header(data: &[u8], mut i: usize) -> Option<(Vec<(String, String)>, usize)> {
    let mut kv = Vec::new();
    let n = data.len();
    loop {
        while i < n && (data[i].is_ascii_whitespace() || data[i] == 0x0c) {
            i += 1;
        }
        if i >= n {
            return None;
        }
        if data[i] == b':' {
            // the terminator: ":" then ctrl-Z (or a newline)
            i += 1;
            if i < n && (data[i] == 0x1a || data[i] == b'\n') {
                i += 1;
            }
            return Some((kv, i));
        }
        if data[i] == b'{' {
            while i < n && data[i] != b'}' {
                i += 1;
            }
            i += 1;
            continue;
        }
        let ks = i;
        while i < n && data[i] != b'=' && !data[i].is_ascii_whitespace() {
            i += 1;
        }
        let key = String::from_utf8_lossy(&data[ks..i]).to_string();
        if i >= n || data[i] != b'=' {
            continue;
        }
        i += 1;
        let val = if i < n && data[i] == b'{' {
            let vs = i + 1;
            while i < n && data[i] != b'}' {
                i += 1;
            }
            let v = data[vs..i.min(n)].to_vec();
            i += 1;
            v
        } else {
            let vs = i;
            while i < n && !data[i].is_ascii_whitespace() {
                i += 1;
            }
            data[vs..i].to_vec()
        };
        kv.push((key, val.iter().map(|&b| b as char).collect()));
    }
}

fn colorspace_named(s: &str) -> Colorspace {
    (0..40)
        .map(Colorspace)
        .find(|c| c.name().eq_ignore_ascii_case(s))
        .unwrap_or(Colorspace::SRGB)
}

pub fn decode_miff(data: &[u8]) -> MResult<Vec<Image>> {
    let bad = |m: &str| corrupt("MIFF", m);
    let mut out = Vec::new();
    let mut pos = 0usize;
    loop {
        while pos < data.len() && (data[pos].is_ascii_whitespace() || data[pos] == 0) {
            pos += 1;
        }
        if pos >= data.len() || !data[pos..].starts_with(b"id=ImageMagick") {
            break;
        }
        let (kv, start) = miff_header(data, pos).ok_or_else(|| bad("improper image header"))?;
        let get = |k: &str| kv.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        let num = |k: &str| get(k).and_then(|v| v.parse::<f64>().ok());
        let (w, h) = (
            num("columns").unwrap_or(0.0) as usize,
            num("rows").unwrap_or(0.0) as usize,
        );
        if w == 0 || h == 0 {
            return Err(bad("improper image header"));
        }
        let depth = num("depth").unwrap_or(8.0) as usize;
        let pseudo = get("class").is_some_and(|c| c.eq_ignore_ascii_case("PseudoClass"));
        let colors = num("colors").unwrap_or(0.0) as usize;
        let cs = colorspace_named(get("colorspace").unwrap_or("sRGB"));
        let alpha = get("alpha-trait").is_some_and(|v| !v.eq_ignore_ascii_case("Undefined"))
            || get("matte").is_some_and(|v| v.eq_ignore_ascii_case("True"));
        let float = get("quantum:format").is_some_and(|v| v.eq_ignore_ascii_case("floating-point"));
        let lsb = get("endian").is_some_and(|v| v.eq_ignore_ascii_case("LSB"));
        let comp = get("compression").unwrap_or("None").to_ascii_lowercase();
        let mut img = Image::blank(w, h);
        img.alpha = alpha;
        img.colorspace = if cs == Colorspace::RGB && get("colorspace").is_none() {
            Colorspace::SRGB
        } else {
            cs
        };
        img.depth = depth as u32;
        let mut p = start;
        // profiles, in the order the header names them: u32 length + bytes
        for (k, v) in &kv {
            if k == "profile" || k.starts_with("profile-") {
                let name = if k == "profile" {
                    v.clone()
                } else {
                    k[8..].to_string()
                };
                if p + 4 > data.len() {
                    return Err(bad("insufficient image data in file"));
                }
                let n =
                    u32::from_be_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
                p += 4;
                let end = (p + n).min(data.len());
                img.set_profile(&name.to_ascii_lowercase(), data[p..end].to_vec());
                p = end;
            }
        }
        // the colormap: R, G, B per entry, 1 byte (2 at depth 16, 4 at 32)
        let bps = (depth / 8).max(1);
        let rd = |b: &[u8]| -> f64 {
            match (b.len(), float) {
                (4, true) => {
                    let a = [b[0], b[1], b[2], b[3]];
                    (if lsb {
                        f32::from_le_bytes(a)
                    } else {
                        f32::from_be_bytes(a)
                    }) as f64
                        * 65535.0
                }
                (8, true) => {
                    let mut a = [0u8; 8];
                    a.copy_from_slice(b);
                    (if lsb {
                        f64::from_le_bytes(a)
                    } else {
                        f64::from_be_bytes(a)
                    }) * 65535.0
                }
                _ => {
                    let mut v: u64 = 0;
                    if lsb {
                        for &x in b.iter().rev() {
                            v = (v << 8) | x as u64;
                        }
                    } else {
                        for &x in b {
                            v = (v << 8) | x as u64;
                        }
                    }
                    let max = ((1u128 << (8 * b.len())) - 1) as f64;
                    v as f64 * 65535.0 / max
                }
            }
        };
        let mut map: Vec<[u16; 3]> = Vec::new();
        if pseudo {
            let n = if colors == 0 { 256 } else { colors };
            if get("colors").is_none() {
                map = (0..256).map(|k| [(k * 257) as u16; 3]).collect();
            } else {
                for _ in 0..n {
                    if p + 3 * bps > data.len() {
                        return Err(bad("insufficient image data in file"));
                    }
                    let c =
                        |k: usize| crate::color::to_q16(rd(&data[p + k * bps..p + (k + 1) * bps]));
                    map.push([c(0), c(1), c(2)]);
                    p += 3 * bps;
                }
            }
            img.colors = Some(map.len());
        }
        // samples per pixel
        let nch = if pseudo {
            1 + alpha as usize
        } else {
            let base = match cs {
                Colorspace::Gray | Colorspace::LinearGray => 1,
                Colorspace::CMYK => 4,
                _ => 3,
            };
            num("number-channels")
                .map(|v| v as usize)
                .unwrap_or(base + alpha as usize)
        };
        let ibps = if pseudo {
            if colors > 256 || depth == 16 { 2 } else { 1 }
        } else {
            bps
        };
        let abps = if pseudo { bps } else { ibps };
        let px_bytes = if pseudo {
            ibps + alpha as usize * abps
        } else {
            nch * ibps
        };
        let row_bytes = px_bytes * w;
        // the raw rows, decompressed
        let mut raw: Vec<u8> = Vec::with_capacity(row_bytes * h);
        match comp.as_str() {
            "none" | "undefined" | "" => {
                let end = p + row_bytes * h;
                if end > data.len() {
                    return Err(bad("insufficient image data in file"));
                }
                raw.extend_from_slice(&data[p..end]);
                p = end;
            }
            "zip" => {
                // one zlib stream, sync-flushed per row, each row's bytes
                // behind a u32 length
                let mut z = Vec::new();
                for _ in 0..h {
                    if p + 4 > data.len() {
                        return Err(bad("insufficient image data in file"));
                    }
                    let n = u32::from_be_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]])
                        as usize;
                    p += 4;
                    let end = (p + n).min(data.len());
                    z.extend_from_slice(&data[p..end]);
                    p = end;
                }
                raw = zlib_inflate(&z).ok_or_else(|| bad("unable to uncompress image"))?;
            }
            "rle" | "runlengthencoded" => {
                // each run: a pixel then (count - 1)
                let total = w * h;
                let mut got = 0;
                while got < total {
                    if p + px_bytes + 1 > data.len() {
                        return Err(bad("insufficient image data in file"));
                    }
                    let px = &data[p..p + px_bytes];
                    let cnt = data[p + px_bytes] as usize + 1;
                    for _ in 0..cnt.min(total - got) {
                        raw.extend_from_slice(px);
                    }
                    got += cnt;
                    p += px_bytes + 1;
                }
            }
            other => {
                return Err(MagickError::with_desc(
                    severity::CORRUPT_IMAGE_ERROR,
                    "unable to uncompress image",
                    other,
                    "error/miff.c/ReadMIFFImage",
                ));
            }
        }
        if raw.len() < row_bytes * h {
            return Err(bad("insufficient image data in file"));
        }
        let mut black = if cs == Colorspace::CMYK {
            Some(vec![0u16; w * h])
        } else {
            None
        };
        for k in 0..w * h {
            let s = &raw[k * px_bytes..(k + 1) * px_bytes];
            let q = |b: &[u8]| crate::color::to_q16(rd(b));
            let px = if pseudo {
                let idx = if ibps == 2 {
                    ((s[0] as usize) << 8) | s[1] as usize
                } else {
                    s[0] as usize
                };
                let c = map.get(idx).copied().unwrap_or([0, 0, 0]);
                let a = if alpha {
                    q(&s[ibps..ibps + abps])
                } else {
                    65535
                };
                [c[0], c[1], c[2], a]
            } else {
                let ch = |c: usize| q(&s[c * ibps..(c + 1) * ibps]);
                match (cs, nch) {
                    (Colorspace::Gray | Colorspace::LinearGray, n) => {
                        let g = ch(0);
                        [g, g, g, if n > 1 && alpha { ch(1) } else { 65535 }]
                    }
                    (Colorspace::CMYK, n) => {
                        if let Some(b) = &mut black {
                            b[k] = ch(3);
                        }
                        [
                            ch(0),
                            ch(1),
                            ch(2),
                            if n > 4 && alpha { ch(4) } else { 65535 },
                        ]
                    }
                    (_, n) => [
                        ch(0),
                        ch(1),
                        ch(2),
                        if n > 3 && alpha { ch(3) } else { 65535 },
                    ],
                }
            };
            img.pixels[k * 4..k * 4 + 4].copy_from_slice(&px);
        }
        img.black = black;
        // the other header fields
        for (k, v) in &kv {
            match k.as_str() {
                "id"
                | "class"
                | "colors"
                | "alpha-trait"
                | "matte"
                | "number-channels"
                | "number-meta-channels"
                | "channel-mask"
                | "columns"
                | "rows"
                | "depth"
                | "colorspace"
                | "compression"
                | "type"
                | "endian"
                | "profile"
                | "version"
                | "quantum:format" => {}
                "gamma" => img.gamma = v.parse().unwrap_or(img.gamma),
                "page" => {
                    if let Some(g) = crate::geometry::Geometry::parse(v) {
                        img.page = crate::geometry::Rect::new(
                            g.width.unwrap_or(w as f64) as usize,
                            g.height.unwrap_or(h as f64) as usize,
                            g.x.unwrap_or(0.0) as i64,
                            g.y.unwrap_or(0.0) as i64,
                        );
                    }
                }
                "resolution" => {
                    if let Some(g) = crate::geometry::Geometry::parse(v) {
                        img.x_resolution = g.width.unwrap_or(0.0);
                        img.y_resolution = g.height.unwrap_or(img.x_resolution);
                    }
                }
                "units" => {
                    img.units = match v.as_str() {
                        "PixelsPerInch" => ResolutionType::PixelsPerInch,
                        "PixelsPerCentimeter" => ResolutionType::PixelsPerCentimeter,
                        _ => ResolutionType::Undefined,
                    }
                }
                "delay" => img.delay = v.parse().unwrap_or(0),
                "iterations" => img.iterations = v.parse().unwrap_or(0),
                "scene" => img.scene = v.parse().unwrap_or(0),
                "quality" => img.quality = v.parse().unwrap_or(0),
                _ if k.starts_with("profile-") => {}
                "rendering-intent" | "red-primary" | "green-primary" | "blue-primary"
                | "white-point" | "orientation" | "dispose" | "background-color"
                | "border-color" | "matte-color" | "ticks-per-second" | "gravity" => {}
                _ => img.properties.set(k, v),
            }
        }
        img.format = "MIFF".to_string();
        out.push(img);
        pos = p;
    }
    if out.is_empty() {
        return Err(corrupt("MIFF", "improper image header"));
    }
    Ok(out)
}

/// WriteMIFFImage(): the header as ImageMagick writes it, the profiles,
/// then DirectClass samples (8 or 16 bits, MSB first) in the image's
/// colourspace, uncompressed, RLE or Zip as the wand's compression asks.
pub fn encode_miff(images: &[&Image], compression: CompressionType) -> Vec<u8> {
    let mut out = Vec::new();
    for img in images {
        let depth = if img.depth > 8 { 16 } else { 8 };
        let cs = img.colorspace;
        let gray = matches!(cs, Colorspace::Gray | Colorspace::LinearGray);
        let cmyk = cs == Colorspace::CMYK;
        let base = if gray {
            1
        } else if cmyk {
            4
        } else {
            3
        };
        let nch = base + img.alpha as usize;
        let mut h = String::new();
        h.push_str("id=ImageMagick  version=1.0\n");
        h.push_str(&format!(
            "class=DirectClass  colors=0  alpha-trait={}\n",
            if img.alpha { "Blend" } else { "Undefined" }
        ));
        h.push_str(&format!(
            "number-channels={nch}  number-meta-channels=0  channel-mask=0x0000000007ffffff\n"
        ));
        if img.alpha {
            h.push_str("matte=True\n");
        }
        let comp = match compression {
            CompressionType::Zip => "Zip",
            CompressionType::RLE => "RLE",
            _ => "",
        };
        h.push_str(&format!(
            "columns={}  rows={}  depth={depth}\n",
            img.columns, img.rows
        ));
        h.push_str(&format!("colorspace={}\n", cs.name()));
        if !comp.is_empty() {
            h.push_str(&format!("compression={comp}  quality={}\n", img.quality));
        }
        if img.units != ResolutionType::Undefined {
            h.push_str(&format!("units={}\n", img.units.name()));
        }
        if img.x_resolution > 0.0 || img.y_resolution > 0.0 {
            h.push_str(&format!(
                "resolution={}x{}\n",
                crate::color::fmt_g(img.x_resolution),
                crate::color::fmt_g(img.y_resolution)
            ));
        }
        let pg = &img.page;
        h.push_str(&format!(
            "page={}x{}{:+}{:+}\n",
            if pg.width == 0 { img.columns } else { pg.width },
            if pg.height == 0 { img.rows } else { pg.height },
            pg.x,
            pg.y
        ));
        if img.gamma != 0.0 {
            h.push_str(&format!("gamma={}\n", crate::color::fmt_g(img.gamma)));
        }
        if img.delay != 0 {
            h.push_str(&format!("delay={}\n", img.delay));
        }
        if img.iterations != 0 {
            h.push_str(&format!("iterations={}\n", img.iterations));
        }
        for (name, _) in &img.profiles {
            h.push_str(&format!("profile={name}\n"));
        }
        for (k, v) in &img.properties.0 {
            let v = if v.contains(|c: char| c.is_whitespace()) || v.is_empty() {
                format!("{{{}}}", v.replace('}', "\\}"))
            } else {
                v.clone()
            };
            h.push_str(&format!("{k}={v}\n"));
        }
        h.push_str("\x0c\n:\x1a");
        out.extend(
            h.chars()
                .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }),
        );
        for (_, data) in &img.profiles {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
        }
        let put = |out: &mut Vec<u8>, v: u16| {
            if depth == 16 {
                out.extend_from_slice(&v.to_be_bytes());
            } else {
                out.push(crate::image::q_to_u8(v));
            }
        };
        let mut raw = Vec::with_capacity(img.columns * img.rows * nch * depth / 8);
        for (k, p) in img.pixels.chunks_exact(4).enumerate() {
            if gray {
                put(&mut raw, p[0]);
            } else {
                put(&mut raw, p[0]);
                put(&mut raw, p[1]);
                put(&mut raw, p[2]);
                if cmyk {
                    put(&mut raw, img.black.as_ref().map_or(0, |b| b[k]));
                }
            }
            if img.alpha {
                put(&mut raw, p[3]);
            }
        }
        let px = nch * depth / 8;
        match comp {
            "RLE" => {
                // a pixel then (run length - 1), runs of at most 256
                let mut k = 0;
                let n = raw.len() / px;
                while k < n {
                    let cur = &raw[k * px..(k + 1) * px];
                    let mut run = 1;
                    while k + run < n
                        && run < 256
                        && &raw[(k + run) * px..(k + run + 1) * px] == cur
                    {
                        run += 1;
                    }
                    out.extend_from_slice(cur);
                    out.push((run - 1) as u8);
                    k += run;
                }
            }
            "Zip" => {
                use goro_zlib::{Deflate, Io, Z_DEFLATED, Z_FINISH, Z_SYNC_FLUSH};
                let row = img.columns * px;
                if let Ok(mut d) = Deflate::new(7, Z_DEFLATED, 15, 8, 0) {
                    let mut buf = vec![0u8; row * 2 + 1024];
                    for (y, r) in raw.chunks(row.max(1)).enumerate() {
                        let flush = if y + 1 == img.rows {
                            Z_FINISH
                        } else {
                            Z_SYNC_FLUSH
                        };
                        let mut chunk = Vec::new();
                        let mut pos = 0;
                        loop {
                            let mut io = Io::new(&r[pos..], &mut buf);
                            d.deflate(&mut io, flush);
                            pos += io.in_pos;
                            let n = io.out_pos;
                            chunk.extend_from_slice(&buf[..n]);
                            if pos >= r.len() && n < buf.len() {
                                break;
                            }
                        }
                        out.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
                        out.extend_from_slice(&chunk);
                    }
                }
            }
            _ => out.extend_from_slice(&raw),
        }
    }
    out
}
