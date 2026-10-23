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

/// 8-bit, 3- or 4-plane PCX (RGB / RGBA), which oxideav-pcx refuses:
/// RLE scanlines of planes × bytes-per-line bytes.
fn decode_pcx_planes(data: &[u8]) -> Option<Image> {
    if data.len() < 128 || data[0] != 0x0a || data[3] != 8 {
        return None;
    }
    let le16 = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]) as usize;
    let (x0, y0, x1, y1) = (le16(4), le16(6), le16(8), le16(10));
    let (w, h) = (x1.checked_sub(x0)? + 1, y1.checked_sub(y0)? + 1);
    let np = data[65] as usize;
    let bpl = le16(66);
    if !(3..=4).contains(&np) || bpl < w {
        return None;
    }
    let rle = data[2] == 1;
    let mut img = Image::blank(w, h);
    let mut o = 128;
    let mut line = vec![0u8; np * bpl];
    for y in 0..h {
        let mut k = 0;
        while k < line.len() && o < data.len() {
            let b = data[o];
            o += 1;
            if rle && b >= 0xc0 {
                let n = (b & 0x3f) as usize;
                let v = *data.get(o)?;
                o += 1;
                for _ in 0..n {
                    if k < line.len() {
                        line[k] = v;
                    }
                    k += 1;
                }
            } else {
                line[k] = b;
                k += 1;
            }
        }
        for x in 0..w {
            let c = |p: usize| line[p * bpl + x] as u16 * 257;
            img.set_px(x, y, [c(0), c(1), c(2), if np == 4 { c(3) } else { 65535 }]);
        }
    }
    img.alpha = np == 4;
    img.depth = 8;
    Some(img)
}

pub fn decode_pcx(data: &[u8]) -> MResult<Vec<Image>> {
    let im = match oxideav_pcx::decode_rgba8(data) {
        Ok(im) => im,
        Err(e) => {
            return decode_pcx_planes(data)
                .map(|i| vec![i])
                .ok_or_else(|| corrupt("PCX", e));
        }
    };
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
    img.depth = 16;
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

// ---------------------------------------------------------------------------
// SGI (Iris RGB), from the SGI image file format specification

pub fn decode_sgi(data: &[u8]) -> MResult<Vec<Image>> {
    let bad = || corrupt("SGI", "improper image header");
    if data.len() < 512 || data[0] != 0x01 || data[1] != 0xda {
        return Err(bad());
    }
    let be16 = |o: usize| u16::from_be_bytes([data[o], data[o + 1]]) as usize;
    let be32 =
        |o: usize| u32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize;
    let rle = data[2] == 1;
    let bpc = data[3] as usize;
    let dim = be16(4);
    let w = be16(6);
    let h = if dim >= 2 { be16(8) } else { 1 };
    let z = if dim >= 3 { be16(10) } else { 1 };
    if !(1..=2).contains(&bpc) || w == 0 || h == 0 || z == 0 {
        return Err(bad());
    }
    let maxv = if bpc == 2 { 65535.0 } else { 255.0 };
    // channel planes of rows, bottom row first
    let mut planes = vec![vec![0u16; w * h]; z];
    let read_val = |o: usize| -> u16 {
        if bpc == 2 {
            u16::from_be_bytes([data[o], data[o + 1]])
        } else {
            data[o] as u16
        }
    };
    for (c, plane) in planes.iter_mut().enumerate() {
        for row in 0..h {
            let dst = &mut plane[(h - 1 - row) * w..(h - row) * w];
            if !rle {
                let o = 512 + ((c * h + row) * w) * bpc;
                if o + w * bpc > data.len() {
                    return Err(corrupt("SGI", "unexpected end-of-file"));
                }
                for (x, d) in dst.iter_mut().enumerate() {
                    *d = read_val(o + x * bpc);
                }
            } else {
                let t = 512 + (c * h + row) * 4;
                if t + 4 > data.len() {
                    return Err(corrupt("SGI", "unexpected end-of-file"));
                }
                let mut o = be32(t);
                let mut x = 0;
                while x < w && o + bpc <= data.len() {
                    let cnt = read_val(o) as usize;
                    o += bpc;
                    let n = cnt & 0x7f;
                    if n == 0 {
                        break;
                    }
                    if cnt & 0x80 != 0 {
                        for _ in 0..n {
                            if x < w && o + bpc <= data.len() {
                                dst[x] = read_val(o);
                            }
                            x += 1;
                            o += bpc;
                        }
                    } else {
                        let v = if o + bpc <= data.len() {
                            read_val(o)
                        } else {
                            0
                        };
                        o += bpc;
                        for _ in 0..n {
                            if x < w {
                                dst[x] = v;
                            }
                            x += 1;
                        }
                    }
                }
            }
        }
    }
    let mut img = Image::blank(w, h);
    let q = |v: u16| crate::color::to_q16(v as f64 * 65535.0 / maxv);
    for k in 0..w * h {
        let p = match z {
            1 | 2 => {
                let g = q(planes[0][k]);
                [g, g, g, if z == 2 { q(planes[1][k]) } else { 65535 }]
            }
            _ => [
                q(planes[0][k]),
                q(planes[1][k]),
                q(planes[2][k]),
                if z >= 4 { q(planes[3][k]) } else { 65535 },
            ],
        };
        img.pixels[k * 4..k * 4 + 4].copy_from_slice(&p);
    }
    img.alpha = z == 2 || z >= 4;
    if z <= 2 {
        img.colorspace = Colorspace::Gray;
    }
    img.depth = (bpc * 8) as u32;
    if rle {
        img.compression = CompressionType::RLE;
    }
    Ok(vec![img])
}

// ---------------------------------------------------------------------------
// XPM, from the XPM manual: the C strings hold "w h ncolors cpp", the
// colour table (chars, then key/colour pairs; the "c" key wins) and the rows

pub fn decode_xpm(data: &[u8]) -> MResult<Vec<Image>> {
    let bad = || corrupt("XPM", "improper image header");
    // the quoted strings, outside comments
    let mut strs: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    let n = data.len();
    while i < n {
        if data[i..].starts_with(b"/*") {
            i += 2;
            while i + 1 < n && !(data[i] == b'*' && data[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else if data[i] == b'"' {
            i += 1;
            let s = i;
            while i < n && data[i] != b'"' {
                i += 1;
            }
            strs.push(data[s..i.min(n)].to_vec());
            i += 1;
        } else {
            i += 1;
        }
    }
    let head = String::from_utf8_lossy(strs.first().ok_or_else(bad)?).to_string();
    let v: Vec<usize> = head
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    if v.len() < 4 {
        return Err(bad());
    }
    let (w, h, nc, cpp) = (v[0], v[1], v[2], v[3]);
    if w == 0 || h == 0 || cpp == 0 || strs.len() < 1 + nc + h {
        return Err(bad());
    }
    let mut table: std::collections::HashMap<Vec<u8>, [u16; 4]> = std::collections::HashMap::new();
    let mut alpha = false;
    for line in &strs[1..1 + nc] {
        if line.len() < cpp {
            return Err(bad());
        }
        let key = line[..cpp].to_vec();
        let rest = String::from_utf8_lossy(&line[cpp..]).to_string();
        let toks: Vec<&str> = rest.split_whitespace().collect();
        // key/value pairs; a colour may be several words ("light blue")
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut k = 0;
        while k < toks.len() {
            if matches!(toks[k], "c" | "m" | "g" | "g4" | "s") && k + 1 < toks.len() {
                let key = toks[k].to_string();
                let mut val = Vec::new();
                k += 1;
                while k < toks.len() && !matches!(toks[k], "c" | "m" | "g" | "g4" | "s") {
                    val.push(toks[k]);
                    k += 1;
                }
                pairs.push((key, val.join(" ")));
            } else {
                k += 1;
            }
        }
        let pick = pairs
            .iter()
            .find(|(k, _)| k == "c")
            .or_else(|| pairs.iter().find(|(k, _)| k == "g"))
            .or_else(|| pairs.iter().find(|(k, _)| k == "g4"))
            .or_else(|| pairs.iter().find(|(k, _)| k == "m"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let c = if pick.eq_ignore_ascii_case("none") {
            alpha = true;
            [0, 0, 0, 0]
        } else {
            match crate::color::PixelInfo::parse(&pick) {
                Some(p) => p.to_rgba16(),
                None => [0, 0, 0, 65535],
            }
        };
        table.insert(key, c);
    }
    let mut img = Image::blank(w, h);
    for y in 0..h {
        let row = &strs[1 + nc + y];
        for x in 0..w {
            let k = &row[(x * cpp).min(row.len())..((x + 1) * cpp).min(row.len())];
            let c = table.get(k).copied().unwrap_or([0, 0, 0, 65535]);
            img.set_px(x, y, c);
        }
    }
    img.alpha = alpha;
    img.colors = Some(nc);
    img.depth = 8;
    Ok(vec![img])
}

// ---------------------------------------------------------------------------
// PSD, from Adobe's Photoshop file format specification: the merged
// (composite) image of a version-1 file

fn packbits_row(data: &[u8], mut o: usize, out: &mut Vec<u8>, want: usize) -> usize {
    while out.len() < want && o < data.len() {
        let n = data[o] as i8;
        o += 1;
        if n >= 0 {
            let cnt = n as usize + 1;
            let end = (o + cnt).min(data.len());
            out.extend_from_slice(&data[o..end]);
            o = end;
        } else if n != -128 {
            let cnt = (-(n as i32)) as usize + 1;
            if o < data.len() {
                let v = data[o];
                o += 1;
                out.extend(std::iter::repeat_n(v, cnt));
            }
        }
    }
    o
}

pub fn decode_psd(data: &[u8]) -> MResult<Vec<Image>> {
    let bad = || corrupt("PSD", "improper image header");
    if data.len() < 26 || !data.starts_with(b"8BPS") {
        return Err(bad());
    }
    let be16 = |o: usize| u16::from_be_bytes([data[o], data[o + 1]]) as usize;
    let be32 = |o: usize| {
        if o + 4 > data.len() {
            0
        } else {
            u32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize
        }
    };
    let version = be16(4);
    if version != 1 {
        return Err(bad());
    }
    let nch = be16(12);
    let h = be32(14);
    let w = be32(18);
    let depth = be16(22);
    let mode = be16(24);
    if w == 0 || h == 0 || !(depth == 8 || depth == 16 || depth == 1) {
        return Err(bad());
    }
    let mut o = 26;
    let cmlen = be32(o);
    let cmap = data.get(o + 4..o + 4 + cmlen).unwrap_or(&[]).to_vec();
    o += 4 + cmlen;
    let rlen = be32(o);
    let resources = data.get(o + 4..o + 4 + rlen).map(|r| r.to_vec());
    o += 4 + rlen;
    let llen = be32(o);
    let layers_at = o + 4;
    o += 4 + llen;
    if o + 2 > data.len() {
        return Err(corrupt("PSD", "unexpected end-of-file"));
    }
    let comp = be16(o);
    o += 2;
    let bps = if depth == 16 { 2 } else { 1 };
    let row_bytes = if depth == 1 { w.div_ceil(8) } else { w * bps };
    let mut chans: Vec<Vec<u8>> = Vec::with_capacity(nch);
    if comp == 1 {
        let counts: Vec<usize> = (0..nch * h).map(|k| be16(o + k * 2)).collect();
        o += nch * h * 2;
        for c in 0..nch {
            let mut plane = Vec::with_capacity(row_bytes * h);
            for r in 0..h {
                let mut row = Vec::with_capacity(row_bytes);
                packbits_row(data, o, &mut row, row_bytes);
                row.resize(row_bytes, 0);
                plane.extend_from_slice(&row);
                o += counts[c * h + r];
            }
            chans.push(plane);
        }
    } else {
        for _ in 0..nch {
            let end = (o + row_bytes * h).min(data.len());
            let mut plane = data[o..end].to_vec();
            plane.resize(row_bytes * h, 0);
            chans.push(plane);
            o = end;
        }
    }
    let sample = |c: usize, k: usize| -> u16 {
        let p = &chans[c];
        match depth {
            16 => u16::from_be_bytes([p[k * 2], p[k * 2 + 1]]),
            1 => {
                let (y, x) = (k / w, k % w);
                if p[y * row_bytes + x / 8] & (0x80 >> (x % 8)) != 0 {
                    0
                } else {
                    65535
                }
            }
            _ => p[k] as u16 * 257,
        }
    };
    let mut img = Image::blank(w, h);
    let color_ch = match mode {
        0..=2 | 8 => 1,
        4 => 4,
        _ => 3,
    };
    let alpha = nch > color_ch;
    let mut black = if mode == 4 {
        Some(vec![0u16; w * h])
    } else {
        None
    };
    for k in 0..w * h {
        let a = if alpha { sample(color_ch, k) } else { 65535 };
        let p = match mode {
            2 => {
                let i = (sample(0, k) / 257) as usize;
                let g = |c: usize| cmap.get(c * 256 + i).copied().unwrap_or(0) as u16 * 257;
                [g(0), g(1), g(2), a]
            }
            0 | 1 | 8 => {
                let g = sample(0, k);
                [g, g, g, a]
            }
            4 => {
                // Photoshop stores CMYK inverted (0 = full ink)
                if let Some(b) = &mut black {
                    b[k] = 65535 - sample(3, k);
                }
                [
                    65535 - sample(0, k),
                    65535 - sample(1, k),
                    65535 - sample(2, k),
                    a,
                ]
            }
            _ => [sample(0, k), sample(1, k), sample(2, k), a],
        };
        img.pixels[k * 4..k * 4 + 4].copy_from_slice(&p);
    }
    img.alpha = alpha;
    if alpha {
        // the composite is stored blended over white; ImageMagick unblends
        // it (psd:alpha-unblend)
        for p in img.pixels.chunks_exact_mut(4) {
            let a = p[3] as f64 / 65535.0;
            if a > 0.0 {
                for c in p.iter_mut().take(3) {
                    *c = crate::color::to_q16((*c as f64 - 65535.0 * (1.0 - a)) / a);
                }
            }
        }
    }
    img.black = black;
    img.colorspace = match mode {
        0 | 1 | 8 => Colorspace::Gray,
        4 => Colorspace::CMYK,
        _ => Colorspace::SRGB,
    };
    if mode == 2 {
        img.colors = Some(cmap.len() / 3);
    }
    img.depth = depth.max(8) as u32;
    if depth == 1 {
        img.depth = 1;
    }
    if comp == 1 {
        img.compression = CompressionType::RLE;
    }
    if let Some(r) = resources {
        if !r.is_empty() {
            img.set_profile("8bim", r);
        }
    }
    let mut all = vec![img];
    if matches!(mode, 1 | 3 | 8) && depth != 1 {
        all.extend(psd_layers(data, layers_at, llen, (w, h), mode, depth));
    }
    Ok(all)
}

/// The layers of a PSD's layer-and-mask section, each its own image on
/// the document's canvas (ImageMagick lists them after the composite),
/// labelled with the layer name.
fn psd_layers(
    data: &[u8],
    start: usize,
    len: usize,
    canvas: (usize, usize),
    mode: usize,
    depth: usize,
) -> Vec<Image> {
    let mut out = Vec::new();
    let end = (start + len).min(data.len());
    let be16 = |o: usize| {
        if o + 2 <= end {
            u16::from_be_bytes([data[o], data[o + 1]])
        } else {
            0
        }
    };
    let be32 = |o: usize| {
        if o + 4 <= end {
            u32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
        } else {
            0
        }
    };
    if len < 4 {
        return out;
    }
    let info_len = be32(start) as usize;
    let mut o = start + 4;
    if info_len < 2 {
        return out;
    }
    let count = (be16(o) as i16).unsigned_abs() as usize;
    o += 2;
    struct Rec {
        top: i64,
        left: i64,
        w: usize,
        h: usize,
        chans: Vec<(i16, usize)>,
        opacity: u8,
        name: String,
    }
    let mut recs = Vec::new();
    for _ in 0..count {
        let top = be32(o) as i32 as i64;
        let left = be32(o + 4) as i32 as i64;
        let bottom = be32(o + 8) as i32 as i64;
        let right = be32(o + 12) as i32 as i64;
        let nch = be16(o + 16) as usize;
        o += 18;
        let mut chans = Vec::new();
        for _ in 0..nch {
            chans.push((be16(o) as i16, be32(o + 2) as usize));
            o += 6;
        }
        // "8BIM", the blend key, opacity, clipping, flags, filler
        let opacity = data.get(o + 8).copied().unwrap_or(255);
        o += 12;
        let extra = be32(o) as usize;
        o += 4;
        let xend = o + extra;
        let mlen = be32(o) as usize;
        let mut p = o + 4 + mlen;
        let blen = be32(p) as usize;
        p += 4 + blen;
        let nlen = data.get(p).copied().unwrap_or(0) as usize;
        let name =
            String::from_utf8_lossy(data.get(p + 1..p + 1 + nlen).unwrap_or(&[])).to_string();
        o = xend;
        recs.push(Rec {
            top,
            left,
            w: (right - left).max(0) as usize,
            h: (bottom - top).max(0) as usize,
            chans,
            opacity,
            name,
        });
    }
    let bps = if depth == 16 { 2 } else { 1 };
    for r in recs {
        let mut planes: Vec<(i16, Vec<u8>)> = Vec::new();
        for &(id, dlen) in &r.chans {
            let cstart = o;
            let comp = be16(o);
            let row_bytes = r.w * bps;
            let mut plane = Vec::with_capacity(row_bytes * r.h);
            let mut q = o + 2;
            if comp == 1 {
                let counts: Vec<usize> = (0..r.h).map(|k| be16(q + k * 2) as usize).collect();
                q += r.h * 2;
                for c in counts {
                    let mut row = Vec::with_capacity(row_bytes);
                    packbits_row(&data[..end], q, &mut row, row_bytes);
                    row.resize(row_bytes, 0);
                    plane.extend_from_slice(&row);
                    q += c;
                }
            } else if comp == 2 || comp == 3 {
                // ZIP, without (2) or with (3) per-row delta prediction
                let e = (cstart + dlen).min(end);
                plane = zlib_inflate(&data[q.min(e)..e]).unwrap_or_default();
                plane.resize(row_bytes * r.h, 0);
                if comp == 3 {
                    psd_unpredict(&mut plane, r.w, r.h, bps);
                }
            } else {
                let e = (q + row_bytes * r.h).min(end);
                plane.extend_from_slice(&data[q.min(e)..e]);
                plane.resize(row_bytes * r.h, 0);
            }
            planes.push((id, plane));
            o = cstart + dlen;
        }
        if r.w == 0 || r.h == 0 {
            continue;
        }
        let get = |id: i16, k: usize| -> Option<u16> {
            planes.iter().find(|(i, _)| *i == id).map(|(_, p)| {
                if bps == 2 {
                    u16::from_be_bytes([p[k * 2], p[k * 2 + 1]])
                } else {
                    p[k] as u16 * 257
                }
            })
        };
        let mut img = Image::blank(r.w, r.h);
        let has_alpha = r.chans.iter().any(|c| c.0 == -1);
        for k in 0..r.w * r.h {
            let a = get(-1, k).map_or(65535.0, |v| v as f64) * r.opacity as f64 / 255.0;
            let px = if matches!(mode, 1 | 8) {
                let g = get(0, k).unwrap_or(0);
                [g, g, g, crate::color::to_q16(a)]
            } else {
                [
                    get(0, k).unwrap_or(0),
                    get(1, k).unwrap_or(0),
                    get(2, k).unwrap_or(0),
                    crate::color::to_q16(a),
                ]
            };
            img.pixels[k * 4..k * 4 + 4].copy_from_slice(&px);
        }
        img.alpha = has_alpha || r.opacity != 255;
        if matches!(mode, 1 | 8) {
            img.colorspace = Colorspace::Gray;
        }
        img.depth = (bps * 8) as u32;
        img.page = crate::geometry::Rect::new(canvas.0, canvas.1, r.left, r.top);
        if !r.name.is_empty() {
            img.properties.set("label", &r.name);
        }
        out.push(img);
    }
    out
}

/// Undo PSD's ZIP prediction: each sample is stored as the difference
/// from the one before it in its row (16-bit samples big-endian).
fn psd_unpredict(plane: &mut [u8], w: usize, h: usize, bps: usize) {
    for y in 0..h {
        if bps == 2 {
            let row = &mut plane[y * w * 2..(y + 1) * w * 2];
            for x in 1..w {
                let prev = u16::from_be_bytes([row[x * 2 - 2], row[x * 2 - 1]]);
                let cur = u16::from_be_bytes([row[x * 2], row[x * 2 + 1]]);
                let v = prev.wrapping_add(cur).to_be_bytes();
                row[x * 2] = v[0];
                row[x * 2 + 1] = v[1];
            }
        } else {
            let row = &mut plane[y * w..(y + 1) * w];
            for x in 1..w {
                row[x] = row[x].wrapping_add(row[x - 1]);
            }
        }
    }
}
