//! Image formats ("coders"): magic sniffing, decoding into Q16 images and
//! encoding them, on the OxideAV format crates' standalone APIs, plus the
//! raw sample formats (RGB, RGBA, GRAY, CMYK...) and farbfeld done here.

use crate::color::PixelInfo;
use crate::error::{MResult, MagickError, severity};
use crate::geometry::Rect;
use crate::image::{Image, q_to_u8};
use crate::types::*;

/// A format ("magick") the engine knows.
pub struct FormatInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub mime: &'static str,
    pub read: bool,
    pub write: bool,
    /// Whether one file can hold several images.
    pub multi: bool,
}

const fn f(
    name: &'static str,
    description: &'static str,
    mime: &'static str,
    read: bool,
    write: bool,
    multi: bool,
) -> FormatInfo {
    FormatInfo {
        name,
        description,
        mime,
        read,
        write,
        multi,
    }
}

/// The formats, sorted by name.
pub static FORMATS: &[FormatInfo] = &[
    f(
        "BGR",
        "Raw blue, green, and red samples",
        "image/x-bgr",
        true,
        true,
        false,
    ),
    f(
        "BGRA",
        "Raw blue, green, red, and alpha samples",
        "image/x-bgra",
        true,
        true,
        false,
    ),
    f(
        "BMP",
        "Microsoft Windows bitmap image",
        "image/bmp",
        true,
        true,
        false,
    ),
    f(
        "BMP2",
        "Microsoft Windows bitmap image (V2)",
        "image/x-bmp2",
        true,
        true,
        false,
    ),
    f(
        "BMP3",
        "Microsoft Windows bitmap image (V3)",
        "image/x-bmp3",
        true,
        true,
        false,
    ),
    f(
        "CANVAS",
        "Constant image uniform color",
        "image/x-canvas",
        true,
        false,
        false,
    ),
    f(
        "CMYK",
        "Raw cyan, magenta, yellow, and black samples",
        "image/x-cmyk",
        true,
        true,
        false,
    ),
    f(
        "CMYKA",
        "Raw cyan, magenta, yellow, black, and alpha samples",
        "image/x-cmyka",
        true,
        true,
        false,
    ),
    f(
        "CUR",
        "Microsoft icon",
        "image/x-win-bitmap",
        true,
        true,
        true,
    ),
    f(
        "DDS",
        "Microsoft DirectDraw Surface",
        "image/x-dds",
        true,
        false,
        true,
    ),
    f(
        "FARBFELD",
        "Farbfeld",
        "image/x-farbfeld",
        true,
        true,
        false,
    ),
    f(
        "GIF",
        "CompuServe graphics interchange format",
        "image/gif",
        true,
        true,
        true,
    ),
    f(
        "GIF87",
        "CompuServe graphics interchange format",
        "image/x-gif87",
        true,
        true,
        false,
    ),
    f(
        "GRADIENT",
        "Gradual linear passing from one shade to another",
        "image/x-gradient",
        true,
        false,
        false,
    ),
    f(
        "GRANITE",
        "Granite texture",
        "image/x-granite",
        true,
        false,
        false,
    ),
    f(
        "GRAY",
        "Raw gray samples",
        "image/x-gray",
        true,
        true,
        false,
    ),
    f(
        "GRAYA",
        "Raw gray and alpha samples",
        "image/x-graya",
        true,
        true,
        false,
    ),
    f(
        "HDR",
        "Radiance RGBE image format",
        "image/vnd.radiance",
        true,
        false,
        false,
    ),
    f("ICO", "Microsoft icon", "image/x-ico", true, true, true),
    f(
        "JPEG",
        "Joint Photographic Experts Group JFIF format",
        "image/jpeg",
        true,
        true,
        false,
    ),
    f(
        "JPG",
        "Joint Photographic Experts Group JFIF format",
        "image/jpeg",
        true,
        true,
        false,
    ),
    f(
        "JXL",
        "JPEG XL (ISO/IEC 18181)",
        "image/jxl",
        true,
        false,
        true,
    ),
    f(
        "LOGO",
        "ImageMagick Logo",
        "image/x-logo",
        true,
        false,
        false,
    ),
    f(
        "MAGICK",
        "Predefined Magick Image (LOGO, ROSE, etc.); output same as 'XC'",
        "image/x-magick",
        true,
        false,
        false,
    ),
    f(
        "MIFF",
        "Magick Image File Format",
        "image/x-miff",
        true,
        true,
        true,
    ),
    f(
        "MSVG",
        "ImageMagick's own SVG internal renderer",
        "image/svg+xml",
        true,
        false,
        false,
    ),
    f(
        "NULL",
        "Constant image of uniform color",
        "image/x-null",
        true,
        true,
        false,
    ),
    f(
        "PAM",
        "Common 2-dimensional bitmap format",
        "image/x-pam",
        true,
        true,
        true,
    ),
    f(
        "PATTERN",
        "Predefined pattern",
        "image/x-pattern",
        true,
        false,
        false,
    ),
    f(
        "PBM",
        "Portable bitmap format (black and white)",
        "image/x-portable-bitmap",
        true,
        true,
        true,
    ),
    f(
        "PCX",
        "ZSoft IBM PC Paintbrush",
        "image/x-pcx",
        true,
        false,
        false,
    ),
    f(
        "PGM",
        "Portable graymap format (gray scale)",
        "image/x-portable-graymap",
        true,
        true,
        true,
    ),
    f(
        "PLASMA",
        "Plasma fractal image",
        "image/x-plasma",
        true,
        false,
        false,
    ),
    f(
        "PNG",
        "Portable Network Graphics",
        "image/png",
        true,
        true,
        false,
    ),
    f(
        "PNG00",
        "PNG inheriting bit-depth, color-type from original, if possible",
        "image/x-png00",
        true,
        true,
        false,
    ),
    f(
        "PNG24",
        "opaque or binary transparent 24-bit RGB",
        "image/x-png24",
        true,
        true,
        false,
    ),
    f(
        "PNG32",
        "opaque or transparent 32-bit RGBA",
        "image/x-png32",
        true,
        true,
        false,
    ),
    f(
        "PNG48",
        "opaque or binary transparent 48-bit RGB",
        "image/x-png48",
        true,
        true,
        false,
    ),
    f(
        "PNG64",
        "opaque or transparent 64-bit RGBA",
        "image/x-png64",
        true,
        true,
        false,
    ),
    f(
        "PNG8",
        "8-bit indexed with optional binary transparency",
        "image/x-png8",
        true,
        true,
        false,
    ),
    f(
        "PNM",
        "Portable anymap",
        "image/x-portable-anymap",
        true,
        true,
        true,
    ),
    f(
        "PPM",
        "Portable pixmap format (color)",
        "image/x-portable-pixmap",
        true,
        true,
        true,
    ),
    f(
        "PSD",
        "Adobe Photoshop bitmap",
        "image/vnd.adobe.photoshop",
        true,
        false,
        true,
    ),
    f(
        "QOI",
        "Quite OK image format",
        "image/x-qoi",
        true,
        true,
        false,
    ),
    f(
        "RADIAL-GRADIENT",
        "Gradual radial passing from one shade to another",
        "image/x-radial-gradient",
        true,
        false,
        false,
    ),
    f(
        "RGB",
        "Raw red, green, and blue samples",
        "image/x-rgb",
        true,
        true,
        false,
    ),
    f(
        "RGBA",
        "Raw red, green, blue, and alpha samples",
        "image/x-rgba",
        true,
        true,
        false,
    ),
    f(
        "RGBO",
        "Raw red, green, blue, and opacity samples",
        "image/x-rgbo",
        true,
        true,
        false,
    ),
    f(
        "ROSE",
        "70x46 Truecolor rose",
        "image/x-rose",
        true,
        false,
        false,
    ),
    f("SGI", "Irix RGB image", "image/x-sgi", true, false, true),
    f(
        "SVG",
        "Scalable Vector Graphics",
        "image/svg+xml",
        true,
        false,
        false,
    ),
    f(
        "SVGZ",
        "Compressed Scalable Vector Graphics",
        "image/svg+xml",
        true,
        false,
        false,
    ),
    f(
        "TGA",
        "Truevision Targa image",
        "image/x-tga",
        true,
        true,
        false,
    ),
    f(
        "TIF",
        "Tagged Image File Format",
        "image/tiff",
        true,
        true,
        true,
    ),
    f(
        "TIFF",
        "Tagged Image File Format",
        "image/tiff",
        true,
        true,
        true,
    ),
    f("TXT", "Text", "text/plain", false, true, false),
    f("WEBP", "WebP Image Format", "image/webp", true, true, true),
    f(
        "WIZARD",
        "ImageMagick Wizard",
        "image/x-wizard",
        true,
        false,
        false,
    ),
    f(
        "XC",
        "Constant image uniform color",
        "image/x-xc",
        true,
        false,
        false,
    ),
    f(
        "XPM",
        "X Windows system pixmap (color)",
        "image/x-xpixmap",
        true,
        false,
        false,
    ),
];

pub fn format_info(name: &str) -> Option<&'static FormatInfo> {
    let up = canonical_format(name);
    FORMATS.iter().find(|f| f.name == up)
}

/// Format aliases.
pub fn canonical_format(name: &str) -> String {
    let up = name.to_ascii_uppercase();
    match up.as_str() {
        "JPE" | "JFIF" | "PJPEG" => "JPEG".to_string(),
        "TIFF64" | "PTIF" => "TIFF".to_string(),
        "ICON" => "ICO".to_string(),
        "DIB" => "BMP".to_string(),
        "FF" => "FARBFELD".to_string(),
        "RADIAL_GRADIENT" => "RADIAL-GRADIENT".to_string(),
        _ => up,
    }
}

/// The magick of a blob from its signature.
pub fn sniff(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("PNG");
    }
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("JPEG");
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("GIF");
    }
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("WEBP");
    }
    if data.starts_with(b"BM") && data.len() > 14 {
        return Some("BMP");
    }
    if data.starts_with(b"II*\0") || data.starts_with(b"MM\0*") || data.starts_with(b"II+\0") {
        return Some("TIFF");
    }
    if data.starts_with(b"qoif") {
        return Some("QOI");
    }
    if data.starts_with(b"farbfeld") {
        return Some("FARBFELD");
    }
    if data.starts_with(b"DDS ") {
        return Some("DDS");
    }
    if data.starts_with(b"#?RADIANCE") || data.starts_with(b"#?RGBE") {
        return Some("HDR");
    }
    if data.starts_with(&[0xff, 0x0a]) || data.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n") {
        return Some("JXL");
    }
    if data.starts_with(b"id=ImageMagick") {
        return Some("MIFF");
    }
    if data.starts_with(b"8BPS") {
        return Some("PSD");
    }
    if data.starts_with(&[0x01, 0xda]) {
        return Some("SGI");
    }
    if data.starts_with(b"/* XPM */") {
        return Some("XPM");
    }
    if data.len() > 128 && data[0] == 0x0a && data[1] <= 5 && data[2] <= 1 {
        return Some("PCX");
    }
    if data.len() > 2 && data[0] == b'P' && (b'1'..=b'7').contains(&data[1]) {
        return Some(match data[1] {
            b'1' | b'4' => "PBM",
            b'2' | b'5' => "PGM",
            b'7' => "PAM",
            _ => "PPM",
        });
    }
    // an icon directory needs at least one entry (a type-1/2 TGA header
    // starts 00 00 01/02 00 too, with a zero colour-map origin)
    if data.len() >= 22
        && data[0] == 0
        && data[1] == 0
        && (data[2] == 1 || data[2] == 2)
        && data[3] == 0
        && u16::from_le_bytes([data[4], data[5]]) > 0
    {
        return Some(if data[2] == 2 { "CUR" } else { "ICO" });
    }
    if !data.starts_with(&[0x1f, 0x8b]) && oxideav_svg::probe(data) {
        return Some("SVG");
    }
    None
}

pub(crate) fn corrupt<E: std::fmt::Display>(magick: &str, e: E) -> MagickError {
    MagickError::with_desc(
        severity::CORRUPT_IMAGE_ERROR,
        &format!("{}", e),
        "",
        &format!(
            "error/{}.c/Read{}Image",
            magick.to_ascii_lowercase(),
            magick
        ),
    )
}

fn coder_err<E: std::fmt::Display>(magick: &str, e: E) -> MagickError {
    MagickError::with_desc(
        severity::CODER_ERROR,
        &format!("{}", e),
        "",
        &format!(
            "error/{}.c/Write{}Image",
            magick.to_ascii_lowercase(),
            magick
        ),
    )
}

/// Options that affect decoding (the wand's ImageInfo).
#[derive(Clone, Debug, Default)]
pub struct ReadOptions {
    /// `-size` (for the raw formats).
    pub size: Option<(usize, usize)>,
    /// `-depth` for raw formats.
    pub depth: Option<u32>,
    pub ping: bool,
    /// `-density` (vector formats render at density / 96).
    pub density: Option<(f64, f64)>,
    /// `-background` (vector formats are composited over it).
    pub background: PixelInfo,
}

/// Decode a blob in a given (or sniffed) format.
pub fn decode(data: &[u8], magick: &str, opts: &ReadOptions) -> MResult<Vec<Image>> {
    let m = canonical_format(magick);
    let mut images = match m.as_str() {
        "PNG" | "PNG8" | "PNG24" | "PNG32" | "PNG48" | "PNG64" | "PNG00" => decode_png(data)?,
        "JPEG" | "JPG" => decode_jpeg(data)?,
        "GIF" | "GIF87" => decode_gif(data)?,
        "WEBP" => decode_webp(data)?,
        "BMP" | "BMP2" | "BMP3" => decode_bmp(data)?,
        "TIFF" | "TIF" => decode_tiff(data)?,
        "ICO" | "CUR" => decode_ico(data)?,
        "TGA" => decode_tga(data)?,
        "QOI" => decode_qoi(data)?,
        "PNM" | "PPM" | "PGM" | "PBM" | "PAM" => decode_pnm(data)?,
        "FARBFELD" => decode_farbfeld(data)?,
        "PCX" => crate::formats::decode_pcx(data)?,
        "DDS" => crate::formats::decode_dds(data)?,
        "JXL" => crate::formats::decode_jxl(data)?,
        "HDR" => crate::formats::decode_hdr(data)?,
        "SVG" | "SVGZ" | "MSVG" | "RSVG" => {
            crate::svg::decode(data, opts.density, &opts.background)?
        }
        "RGB" | "RGBA" | "RGBO" | "BGR" | "BGRA" | "GRAY" | "GRAYA" | "CMYK" | "CMYKA" => {
            decode_raw(data, &m, opts)?
        }
        _ => return Err(MagickError::no_blob_delegate(magick)),
    };
    let fmt = match m.as_str() {
        "JPG" => "JPEG".to_string(),
        "TIF" => "TIFF".to_string(),
        "SVGZ" | "MSVG" | "RSVG" => "SVG".to_string(),
        "PNG8" | "PNG24" | "PNG32" | "PNG48" | "PNG64" | "PNG00" => "PNG".to_string(),
        _ => m.clone(),
    };
    for (i, img) in images.iter_mut().enumerate() {
        if img.format.is_empty() {
            img.format = fmt.clone();
        }
        img.scene = i;
        img.file_size = data.len() as u64;
        img.magick_columns = img.columns;
        img.magick_rows = img.rows;
        if img.page.width == 0 && img.page.height == 0 {
            img.page = Rect::new(img.columns, img.rows, 0, 0);
        }
        crate::properties::parse_profiles(img);
    }
    Ok(images)
}

fn from_rgba8(w: u32, h: u32, rgba: &[u8], alpha: bool) -> Image {
    let mut img = Image::blank(w as usize, h as usize);
    img.alpha = alpha;
    img.set_from_rgba8(rgba);
    img.depth = 8;
    img
}

fn from_rgba16le(
    w: u32,
    h: u32,
    data: &[u8],
    channels: usize,
    stride: usize,
    alpha: bool,
) -> Image {
    let mut img = Image::blank(w as usize, h as usize);
    img.alpha = alpha;
    img.depth = 16;
    for y in 0..h as usize {
        let row = &data[y * stride..];
        for x in 0..w as usize {
            let s = &row[x * channels * 2..];
            let rd = |i: usize| u16::from_le_bytes([s[i * 2], s[i * 2 + 1]]);
            let p = match channels {
                1 => {
                    let g = rd(0);
                    [g, g, g, 65535]
                }
                2 => {
                    let g = rd(0);
                    [g, g, g, rd(1)]
                }
                3 => [rd(0), rd(1), rd(2), 65535],
                _ => [rd(0), rd(1), rd(2), rd(3)],
            };
            img.set_px(x, y, p);
        }
    }
    img
}

fn decode_png(data: &[u8]) -> MResult<Vec<Image>> {
    use oxideav_png::PixelFormat as P;
    let png = oxideav_png::decode(data).map_err(|e| corrupt("PNG", e))?;
    let (w, h) = (png.width, png.height);
    let plane = &png.planes[0];
    let mut img = match png.format {
        P::Gray16Le => from_rgba16le(w, h, &plane.data, 1, plane.stride, false),
        P::Rgb48Le => from_rgba16le(w, h, &plane.data, 3, plane.stride, false),
        P::Rgba64Le => from_rgba16le(w, h, &plane.data, 4, plane.stride, true),
        _ => {
            let alpha = matches!(png.format, P::Rgba | P::Ya8)
                || (png.format == P::Pal8
                    && png
                        .palette
                        .as_ref()
                        .is_some_and(|p| p.entries.iter().any(|e| e[3] != 255)))
                || png.transparency.is_some();
            from_rgba8(w, h, &png.to_rgba8(), alpha)
        }
    };
    if matches!(png.format, P::Gray8 | P::Gray16Le | P::Ya8) {
        img.colorspace = Colorspace::Gray;
    }
    // a gray PNG below 8 bits keeps its bit depth (IHDR byte 24, colour
    // type 0 at byte 25)
    if data.len() > 25 && data[25] == 0 && data[24] < 8 {
        img.depth = data[24] as u32;
    }
    if png.format == P::Pal8 {
        img.colors = png.palette.as_ref().map(|p| p.entries.len());
    }
    if let Some(icc) = &png.metadata.icc {
        img.set_profile("icc", icc.clone());
    }
    if let Some(exif) = &png.metadata.exif {
        img.set_profile("exif", exif.clone());
    }
    if let Some(xmp) = &png.metadata.xmp {
        img.set_profile("xmp", xmp.clone());
    }
    if let Some(g) = png.metadata.gamma {
        img.gamma = g as f64;
    }
    if let Ok(meta) = oxideav_png::parse_metadata(data) {
        if let Some(p) = &meta.phys {
            if matches!(p.unit, oxideav_png::PhysUnit::Metre) {
                img.units = ResolutionType::PixelsPerCentimeter;
                img.x_resolution = p.pixels_per_unit_x as f64 / 100.0;
                img.y_resolution = p.pixels_per_unit_y as f64 / 100.0;
            } else {
                img.x_resolution = p.pixels_per_unit_x as f64;
                img.y_resolution = p.pixels_per_unit_y as f64;
            }
        }
        // the page: ImageMagick's caNv, else oFFs (pixels) and vpAg
        let (mut pw, mut ph, mut px, mut py) = (w as usize, h as usize, 0i64, 0i64);
        let be32 = |b: &[u8], i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut canvas = false;
        for u in &meta.unknowns {
            let d = &u.data;
            match &u.chunk_type {
                b"caNv" if d.len() >= 16 => {
                    pw = be32(d, 0) as usize;
                    ph = be32(d, 4) as usize;
                    px = be32(d, 8) as i32 as i64;
                    py = be32(d, 12) as i32 as i64;
                    canvas = true;
                }
                b"oFFs" if d.len() >= 9 && d[8] == 0 && !canvas => {
                    px = be32(d, 0) as i32 as i64;
                    py = be32(d, 4) as i32 as i64;
                }
                b"vpAg" if d.len() >= 9 && !canvas => {
                    pw = be32(d, 0) as usize;
                    ph = be32(d, 4) as usize;
                }
                _ => {}
            }
        }
        if pw == 0 && ph == 0 {
            // an offset alone: the canvas is the image's size
            (pw, ph) = (w as usize, h as usize);
        }
        img.page = Rect::new(pw, ph, px, py);
        for t in &meta.texts {
            img.properties.set(&t.keyword, &t.text);
        }
        for t in &meta.ztxts {
            img.properties.set(&t.keyword, &t.text);
        }
        for t in &meta.itxts {
            if t.keyword != "XML:com.adobe.xmp" {
                img.properties.set(&t.keyword, &t.text);
            }
        }
    }
    png_chunk_properties(&mut img, data);
    img.compression = CompressionType::Zip;
    img.quality = 0;
    Ok(vec![img])
}

/// What ImageMagick's PNG reader records of the chunks it met (png:*
/// properties), the depth of the file, its orNT orientation and its
/// primaries (none for a gray file without cHRM).
fn png_chunk_properties(img: &mut Image, data: &[u8]) {
    let mut chunks: Vec<(&[u8], &[u8])> = Vec::new();
    let mut i = 8;
    while i + 8 <= data.len() {
        let n = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let t = &data[i + 4..i + 8];
        let end = (i + 8 + n).min(data.len());
        chunks.push((t, &data[i + 8..end]));
        if t == b"IEND" {
            break;
        }
        i += 12 + n;
    }
    let find = |t: &[u8]| chunks.iter().find(|(c, _)| *c == t).map(|(_, d)| *d);
    let Some(ihdr) = find(b"IHDR").filter(|d| d.len() >= 13) else {
        return;
    };
    let be32 = |d: &[u8], o: usize| u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let (bit_depth, colour_type, interlace) = (ihdr[8], ihdr[9], ihdr[12]);
    img.depth = bit_depth as u32;
    let mut props: Vec<(&str, String)> = Vec::new();
    let found = "chunk was found";
    if find(b"bKGD").is_some() {
        props.push((
            "png:bKGD",
            "chunk was found (see Background color, above)".into(),
        ));
    }
    if find(b"cHRM").is_some() {
        props.push((
            "png:cHRM",
            "chunk was found (see Chromaticity, above)".into(),
        ));
    } else if matches!(colour_type, 0 | 4) {
        img.chromaticity = Default::default();
    }
    if let Some(g) = find(b"gAMA").filter(|d| d.len() >= 4) {
        props.push((
            "png:gAMA",
            format!(
                "gamma={} (See Gamma, above)",
                crate::color::fmt_g(be32(g, 0) as f64 / 100_000.0)
            ),
        ));
    }
    if find(b"iCCP").is_some() {
        props.push(("png:iCCP", found.into()));
    }
    props.push(("png:IHDR.bit-depth-orig", bit_depth.to_string()));
    props.push(("png:IHDR.bit_depth", bit_depth.to_string()));
    props.push(("png:IHDR.color-type-orig", colour_type.to_string()));
    let ct_name = match colour_type {
        0 => "Grayscale",
        2 => "Truecolor",
        3 => "Indexed",
        4 => "GrayAlpha",
        6 => "RGBA",
        _ => "Unknown",
    };
    props.push(("png:IHDR.color_type", format!("{colour_type} ({ct_name})")));
    props.push((
        "png:IHDR.interlace_method",
        if interlace == 0 {
            "0 (Not interlaced)".into()
        } else {
            format!("{interlace} (Adam7 method)")
        },
    ));
    props.push((
        "png:IHDR.width,height",
        format!("{}, {}", be32(ihdr, 0), be32(ihdr, 4)),
    ));
    if let Some(d) = find(b"pHYs").filter(|d| d.len() >= 9) {
        props.push((
            "png:pHYs",
            format!("x_res={}, y_res={}, units={}", be32(d, 0), be32(d, 4), d[8]),
        ));
    }
    if let Some(d) = find(b"PLTE") {
        props.push(("png:PLTE.number_colors", (d.len() / 3).to_string()));
    }
    if let Some(d) = find(b"sRGB").filter(|d| !d.is_empty()) {
        let name = match d[0] {
            0 => "Perceptual Intent",
            1 => "Relative Intent",
            2 => "Saturation Intent",
            _ => "Absolute Intent",
        };
        props.push(("png:sRGB", format!("intent={} ({name})", d[0])));
    }
    let texts = chunks
        .iter()
        .filter(|(c, _)| matches!(*c, b"tEXt" | b"zTXt" | b"iTXt"))
        .count();
    if texts > 0 {
        props.push((
            "png:text",
            format!("{texts} tEXt/zTXt/iTXt chunks were found"),
        ));
    }
    if let Some(d) = find(b"tIME").filter(|d| d.len() >= 7) {
        props.push((
            "png:tIME",
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                u16::from_be_bytes([d[0], d[1]]),
                d[2],
                d[3],
                d[4],
                d[5],
                d[6]
            ),
        ));
    }
    if find(b"tRNS").is_some() {
        props.push(("png:tRNS", found.into()));
    }
    for (k, v) in props {
        img.properties.set(k, &v);
    }
    if let Some(d) = find(b"orNT").filter(|d| !d.is_empty()) {
        img.orientation = OrientationType(d[0] as i32);
    }
    // the background colour; a palette entry's samples are scaled as if
    // they had the file's bit depth (ImageMagick does so, wrapping at 16
    // bits)
    if let Some(b) = find(b"bKGD") {
        let max = ((1u32 << bit_depth.min(16)) - 1).max(1);
        let scale = |v: u32| (v.wrapping_mul(65535 / max) & 0xffff) as f64;
        let rgb = match (colour_type, b.len()) {
            (3, 1..) => find(b"PLTE")
                .filter(|p| p.len() >= 3 * (b[0] as usize + 1))
                .map(|p| {
                    let e = &p[3 * b[0] as usize..];
                    [scale(e[0] as u32), scale(e[1] as u32), scale(e[2] as u32)]
                }),
            (0 | 4, 2..) => {
                let v = scale(u16::from_be_bytes([b[0], b[1]]) as u32);
                Some([v, v, v])
            }
            (2 | 6, 6..) => {
                let c = |o: usize| scale(u16::from_be_bytes([b[o], b[o + 1]]) as u32);
                Some([c(0), c(2), c(4)])
            }
            _ => None,
        };
        if let Some([r, g, bl]) = rgb {
            img.background_color = PixelInfo {
                red: r,
                green: g,
                blue: bl,
                alpha: 65535.0,
                ..PixelInfo::white()
            };
        }
    }
}

fn decode_jpeg(data: &[u8]) -> MResult<Vec<Image>> {
    use oxideav_mjpeg::PixelFormat as P;
    let jpg = oxideav_mjpeg::decode(data).map_err(|e| corrupt("JPEG", e))?;
    let (w, h) = (jpg.width, jpg.height);
    let mut img;
    if jpg.format == P::Cmyk {
        img = Image::blank(w as usize, h as usize);
        img.colorspace = Colorspace::CMYK;
        let exact = jpeg_islow_cmyk(data).filter(|v| v.len() == (w * h * 4) as usize);
        let plane = &jpg.planes[0];
        let mut k = vec![0u16; (w * h) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let s = match &exact {
                    Some(v) => &v[(y * w as usize + x) * 4..],
                    None => &plane.data[y * plane.stride + x * 4..],
                };
                img.set_px(
                    x,
                    y,
                    [
                        s[0] as u16 * 257,
                        s[1] as u16 * 257,
                        s[2] as u16 * 257,
                        65535,
                    ],
                );
                k[y * w as usize + x] = s[3] as u16 * 257;
            }
        }
        img.black = Some(k);
    } else if let Some((rgba, gray)) =
        jpeg_islow_rgba(data).filter(|(r, _)| r.len() == w as usize * h as usize * 4)
    {
        img = from_rgba8(w, h, &rgba, false);
        if gray {
            img.colorspace = Colorspace::Gray;
        }
    } else {
        let rgba = jpeg_ycc_to_rgba(&jpg).unwrap_or_else(|| jpg.to_rgba8());
        img = from_rgba8(w, h, &rgba, false);
        if matches!(
            jpg.format,
            P::Gray8 | P::Gray10Le | P::Gray12Le | P::Gray16Le
        ) {
            img.colorspace = Colorspace::Gray;
        }
    }
    img.depth = 8;
    if let Some(icc) = &jpg.metadata.icc {
        img.set_profile("icc", icc.clone());
    }
    if let Some(exif) = &jpg.metadata.exif {
        img.set_profile("exif", exif.clone());
    }
    if let Some(xmp) = &jpg.metadata.xmp {
        img.set_profile("xmp", xmp.clone());
    }
    if let Some(ps) = jpeg_photoshop(data) {
        if let Some(iptc) = photoshop_resource(&ps, 0x0404) {
            img.set_profile("iptc", iptc);
        }
        img.set_profile("8bim", ps);
    }
    if let Ok(info) = oxideav_mjpeg::inspect_jpeg(data) {
        if let Some(j) = &info.jfif {
            match j.units {
                oxideav_mjpeg::JfifUnits::DotsPerInch => {
                    img.units = ResolutionType::PixelsPerInch;
                    img.x_resolution = j.h_density as f64;
                    img.y_resolution = j.v_density as f64;
                }
                oxideav_mjpeg::JfifUnits::DotsPerCm => {
                    img.units = ResolutionType::PixelsPerCentimeter;
                    img.x_resolution = j.h_density as f64;
                    img.y_resolution = j.v_density as f64;
                }
                _ => {
                    img.x_resolution = j.h_density as f64;
                    img.y_resolution = j.v_density as f64;
                }
            }
        }
        let comment: Vec<u8> = info
            .comments
            .iter()
            .flat_map(|c| c.bytes.iter().copied())
            .collect();
        if !comment.is_empty() {
            img.properties
                .set("comment", &String::from_utf8_lossy(&comment));
        }
    }
    img.compression = CompressionType::JPEG;
    img.quality = crate::properties::estimate_jpeg_quality(data).unwrap_or(0);
    img.interlace = InterlaceType::No;
    jpeg_frame_properties(&mut img, data);
    Ok(vec![img])
}

/// jpeg:colorspace (libjpeg's J_COLOR_SPACE) and jpeg:sampling-factor of
/// the frame, as ImageMagick's reader records them.
fn jpeg_frame_properties(img: &mut Image, data: &[u8]) {
    let mut i = 2;
    let mut adobe: Option<u8> = None;
    while i + 4 <= data.len() && data[i] == 0xff {
        let m = data[i + 1];
        if m == 0xd8 || (0xd0..=0xd7).contains(&m) || m == 0x01 {
            i += 2;
            continue;
        }
        let n = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let seg = &data[(i + 4).min(data.len())..(i + 2 + n).min(data.len())];
        if m == 0xee && seg.len() >= 12 && seg.starts_with(b"Adobe") {
            adobe = Some(seg[11]);
        }
        if matches!(m, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) && seg.len() >= 6 {
            let nc = seg[5] as usize;
            let factors: Vec<String> = (0..nc)
                .filter_map(|k| seg.get(6 + 3 * k + 1))
                .map(|f| format!("{}x{}", f >> 4, f & 15))
                .collect();
            let cs = match (nc, adobe) {
                (1, _) => 1,
                (3, Some(0)) => 3,
                (3, _) => 2,
                (4, Some(2)) => 5,
                (4, _) => 4,
                _ => 0,
            };
            img.properties.set("jpeg:colorspace", &cs.to_string());
            img.properties
                .set("jpeg:sampling-factor", &factors.join(","));
            return;
        }
        if m == 0xda {
            return;
        }
        i += 2 + n;
    }
}

fn decode_gif(data: &[u8]) -> MResult<Vec<Image>> {
    let file = oxideav_gif::parse(data).map_err(|e| corrupt("GIF", e))?;
    let (sw, sh) = (file.screen_width as usize, file.screen_height as usize);
    let global = file.global_palette.clone();
    let iterations = file.loop_count().map(|n| n as u64).unwrap_or(1);
    let comment = file.concatenated_comment();
    let bg = global
        .as_ref()
        .and_then(|p| p.get(file.background_index as usize))
        .map(|c| PixelInfo::from_rgba8([c.r, c.g, c.b, 255]));
    let mut out = Vec::new();
    for fr in file.frames() {
        let (w, h) = (fr.width as usize, fr.height as usize);
        let pal = fr
            .local_palette
            .as_ref()
            .or(global.as_ref())
            .cloned()
            .unwrap_or_default();
        let gc = fr.graphic_control.as_ref();
        let ti = gc.and_then(|g| g.transparent_index);
        let mut img = Image::blank(w, h);
        img.depth = 8;
        img.alpha = ti.is_some();
        for (i, &ix) in fr.indices.iter().enumerate().take(w * h) {
            let (x, y) = (i % w, i / w);
            let c = pal
                .get(ix as usize)
                .copied()
                .unwrap_or(oxideav_gif::Rgb::new(0, 0, 0));
            let a = if Some(ix) == ti { 0 } else { 65535 };
            img.set_px(
                x,
                y,
                [c.r as u16 * 257, c.g as u16 * 257, c.b as u16 * 257, a],
            );
        }
        img.page = Rect::new(sw, sh, fr.left as i64, fr.top as i64);
        img.colors = Some(pal.len().max(1));
        if let Some(g) = gc {
            img.delay = g.delay_centis as u64;
            img.dispose = match g.disposal {
                oxideav_gif::DisposalMethod::None => DisposeType::Undefined,
                oxideav_gif::DisposalMethod::Keep => DisposeType::No,
                oxideav_gif::DisposalMethod::RestoreBackground => DisposeType::Background,
                oxideav_gif::DisposalMethod::RestorePrevious => DisposeType::Previous,
            };
        }
        img.ticks_per_second = 100;
        img.iterations = iterations;
        img.compression = CompressionType::LZW;
        if fr.interlaced {
            img.interlace = InterlaceType::GIF;
        }
        if let Some(b) = &bg {
            img.background_color = b.clone();
        }
        if let Some(c) = &comment {
            img.properties.set("comment", &String::from_utf8_lossy(c));
        }
        out.push(img);
    }
    if out.is_empty() {
        return Err(corrupt("GIF", "no images"));
    }
    Ok(out)
}

/// libwebp's default lossy output (and so ImageMagick's): "fancy" 4:2:0
/// upsampling, each output chroma sample about (9·near + 3·side + 3·side +
/// far)/16 of the four surrounding chroma samples, then libwebp's
/// 14-bit fixed-point BT.601 conversion. oxideav-webp upsamples nearest
/// and converts in Q16, a level or more off.
fn webp_yuv_to_rgba(im: &oxideav_webp::WebpImage) -> Option<Vec<u8>> {
    use oxideav_webp::PixelFormat as P;
    if !matches!(im.format, P::Yuv420P | P::Yuva420P) || im.planes.len() < 3 {
        return None;
    }
    let (w, h) = (im.width as usize, im.height as usize);
    let (yp, up, vp) = (&im.planes[0], &im.planes[1], &im.planes[2]);
    let ap = im.planes.get(3);
    let cw = w.div_ceil(2);
    let ch = h.div_ceil(2);
    let mult_hi = |v: i32, c: i32| (v * c) >> 8;
    let clip8 = |v: i32| -> u8 {
        if v & !((256 << 6) - 1) == 0 {
            (v >> 6) as u8
        } else if v < 0 {
            0
        } else {
            255
        }
    };
    let to_rgb = |y: i32, u: i32, v: i32| -> [u8; 3] {
        let yy = mult_hi(y, 19077);
        [
            clip8(yy + mult_hi(v, 26149) - 14234),
            clip8(yy - mult_hi(u, 6419) - mult_hi(v, 13320) + 8708),
            clip8(yy + mult_hi(u, 33050) - 17685),
        ]
    };
    let cu = |x: usize, y: usize| up.data[y.min(ch - 1) * up.stride + x.min(cw - 1)] as i32;
    let cv = |x: usize, y: usize| vp.data[y.min(ch - 1) * vp.stride + x.min(cw - 1)] as i32;
    // the chroma for pixel (x, y): output rows pair chroma rows j-1, j
    // (row 0 and an even last row repeat theirs); columns likewise
    let sample = |x: usize, y: usize, get: &dyn Fn(usize, usize) -> i32| -> i32 {
        // nearest and far chroma rows
        let (jn, jf) = if y == 0 {
            (0, 0)
        } else {
            let j = (y + 1) / 2; // the lower chroma row of the pair
            if y % 2 == 1 {
                (j - 1, j.min(ch - 1))
            } else {
                (j.min(ch - 1), j - 1)
            }
        };
        let (in_, if_) = if x == 0 {
            (0, 0)
        } else {
            let i = (x + 1) / 2;
            if x % 2 == 1 {
                (i - 1, i.min(cw - 1))
            } else {
                (i.min(cw - 1), i - 1)
            }
        };
        let (a, c) = (get(in_, jn), get(in_, jf));
        if in_ == if_ {
            // the first and an even last column interpolate vertically only
            return (3 * a + c + 2) >> 2;
        }
        let (b, d) = (get(if_, jn), get(if_, jf));
        // libwebp's two-stage rounding: a diagonal average, then halfway
        (((a + 3 * b + 3 * c + d + 8) >> 3) + a) >> 1
    };
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let yy = yp.data[y * yp.stride + x] as i32;
            let u = sample(x, y, &cu);
            let v = sample(x, y, &cv);
            let rgb = to_rgb(yy, u, v);
            let o = (y * w + x) * 4;
            out[o..o + 3].copy_from_slice(&rgb);
            out[o + 3] = ap.map_or(255, |a| a.data[y * a.stride + x]);
        }
    }
    Some(out)
}

fn decode_webp(data: &[u8]) -> MResult<Vec<Image>> {
    let info = oxideav_webp::info(data).map_err(|e| corrupt("WEBP", e))?;
    let meta = oxideav_webp::read_metadata(data).ok();
    let mut out = Vec::new();
    if info.frames > 1 {
        let frames = oxideav_webp::decode_all(data).map_err(|e| corrupt("WEBP", e))?;
        for fr in frames {
            let mut img = from_rgba8(fr.image.width, fr.image.height, &fr.image.to_rgba8(), true);
            if let Some(d) = fr.delay {
                img.delay = (d.as_millis() as u64).div_ceil(10);
            }
            img.iterations = info.loop_count.map_or(0, |n| n as u64);
            out.push(img);
        }
    } else {
        let im = oxideav_webp::decode(data).map_err(|e| corrupt("WEBP", e))?;
        let alpha = info.has_alpha;
        let rgba = webp_yuv_to_rgba(&im).unwrap_or_else(|| im.to_rgba8());
        out.push(from_rgba8(im.width, im.height, &rgba, alpha));
    }
    for img in out.iter_mut() {
        img.depth = 8;
        img.compression = CompressionType::WebP;
        if let Some(m) = &meta {
            if let Some(icc) = &m.icc {
                img.set_profile("icc", icc.clone());
            }
            if let Some(exif) = &m.exif {
                img.set_profile("exif", exif.clone());
            }
            if let Some(xmp) = &m.xmp {
                img.set_profile("xmp", xmp.clone());
            }
        }
    }
    Ok(out)
}

fn decode_bmp(data: &[u8]) -> MResult<Vec<Image>> {
    let bmp = oxideav_bmp::decode(data).map_err(|e| corrupt("BMP", e))?;
    let alpha = matches!(
        bmp.format,
        oxideav_bmp::PixelFormat::Rgba | oxideav_bmp::PixelFormat::Bgra
    ) && {
        let rgba = bmp.to_rgba8();
        rgba.chunks_exact(4).any(|p| p[3] != 255)
    };
    let mut img = from_rgba8(bmp.width, bmp.height, &bmp.to_rgba8(), alpha);
    if let Some(p) = &bmp.palette {
        img.colors = Some(p.entries.len());
    }
    if let Some(icc) = &bmp.metadata.icc {
        img.set_profile("icc", icc.clone());
    }
    if data.len() > 46 {
        let xr = i32::from_le_bytes([data[38], data[39], data[40], data[41]]);
        let yr = i32::from_le_bytes([data[42], data[43], data[44], data[45]]);
        if xr > 0 && yr > 0 {
            img.units = ResolutionType::PixelsPerCentimeter;
            img.x_resolution = xr as f64 / 100.0;
            img.y_resolution = yr as f64 / 100.0;
        }
    }
    Ok(vec![img])
}

fn decode_tiff(data: &[u8]) -> MResult<Vec<Image>> {
    use oxideav_tiff::PixelFormat as P;
    let pages = oxideav_tiff::decode_pages(data).map_err(|e| corrupt("TIFF", e))?;
    let mut out = Vec::new();
    for page in pages {
        let t = &page.image;
        let plane = &t.planes[0];
        let mut img = match t.format {
            P::Gray16Le => from_rgba16le(t.width, t.height, &plane.data, 1, plane.stride, false),
            P::Rgb48Le => from_rgba16le(t.width, t.height, &plane.data, 3, plane.stride, false),
            P::Cmyk => {
                let mut img = Image::blank(t.width as usize, t.height as usize);
                img.colorspace = Colorspace::CMYK;
                let mut k = vec![0u16; (t.width * t.height) as usize];
                for y in 0..t.height as usize {
                    for x in 0..t.width as usize {
                        let s = &plane.data[y * plane.stride + x * 4..];
                        img.set_px(
                            x,
                            y,
                            [
                                s[0] as u16 * 257,
                                s[1] as u16 * 257,
                                s[2] as u16 * 257,
                                65535,
                            ],
                        );
                        k[y * t.width as usize + x] = s[3] as u16 * 257;
                    }
                }
                img.black = Some(k);
                img.depth = 8;
                img
            }
            _ => from_rgba8(t.width, t.height, &t.to_rgba8(), t.format == P::Rgba),
        };
        if matches!(t.format, P::Gray8 | P::Gray16Le | P::GrayF32Le) {
            img.colorspace = Colorspace::Gray;
        }
        let m = &page.metadata;
        if let (Some(xr), Some(yr)) = (m.x_resolution, m.y_resolution) {
            if xr.1 != 0 && yr.1 != 0 {
                img.x_resolution = xr.0 as f64 / xr.1 as f64;
                img.y_resolution = yr.0 as f64 / yr.1 as f64;
            }
            img.units = match m.resolution_unit {
                Some(oxideav_tiff::ResolutionUnit::Inch) => ResolutionType::PixelsPerInch,
                Some(oxideav_tiff::ResolutionUnit::Centimeter) => {
                    ResolutionType::PixelsPerCentimeter
                }
                _ => ResolutionType::Undefined,
            };
        }
        let mut setp = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                img.properties.set(k, v);
            }
        };
        setp("comment", &m.image_description);
        setp("tiff:software", &m.software);
        setp("tiff:artist", &m.artist);
        setp("tiff:copyright", &m.copyright);
        setp("tiff:timestamp", &m.date_time);
        setp("tiff:make", &m.make);
        setp("tiff:model", &m.model);
        setp("tiff:document", &m.document_name);
        setp("label", &m.page_name);
        setp("tiff:hostcomputer", &m.host_computer);
        if let Some(icc) = &m.icc_profile {
            img.set_profile("icc", icc.clone());
        }
        if let Some(xmp) = &m.xmp {
            img.set_profile("xmp", xmp.clone());
        }
        if let Some(icc) = &t.metadata.icc {
            img.set_profile("icc", icc.clone());
        }
        if let Some(exif) = &t.metadata.exif {
            img.set_profile("exif", exif.clone());
        }
        if let Some(o) = m.orientation {
            img.orientation = OrientationType(o as i32);
        }
        out.push(img);
    }
    if out.is_empty() {
        return Err(corrupt("TIFF", "no pages"));
    }
    Ok(out)
}

fn decode_ico(data: &[u8]) -> MResult<Vec<Image>> {
    let frames = oxideav_ico::decode_all(data).map_err(|e| corrupt("ICON", e))?;
    Ok(frames
        .into_iter()
        .map(|fr| {
            let rgba = fr.image.to_rgba8();
            let alpha = rgba.chunks_exact(4).any(|p| p[3] != 255);
            from_rgba8(fr.image.width, fr.image.height, &rgba, alpha)
        })
        .collect())
}

fn decode_tga(data: &[u8]) -> MResult<Vec<Image>> {
    let t = oxideav_tga::decode(data).map_err(|e| corrupt("TGA", e))?;
    let alpha = t.format == oxideav_tga::PixelFormat::Rgba;
    let mut img = from_rgba8(t.width, t.height, &t.to_rgba8(), alpha);
    if t.format == oxideav_tga::PixelFormat::Gray8 {
        img.colorspace = Colorspace::Gray;
    }
    Ok(vec![img])
}

fn decode_qoi(data: &[u8]) -> MResult<Vec<Image>> {
    let q = oxideav_qoi::decode(data).map_err(|e| corrupt("QOI", e))?;
    let alpha = q.format == oxideav_qoi::PixelFormat::Rgba;
    Ok(vec![from_rgba8(q.width, q.height, &q.to_rgba8(), alpha)])
}

fn decode_pnm(data: &[u8]) -> MResult<Vec<Image>> {
    use oxideav_pbm::PixelFormat as P;
    let frames = oxideav_pbm::decode_all(data).map_err(|e| corrupt("PNM", e))?;
    let mut out = Vec::new();
    for fr in frames {
        let p = &fr.image;
        let plane = &p.planes[0];
        let mut img = match p.format {
            P::Gray16Le => from_rgba16le(p.width, p.height, &plane.data, 1, plane.stride, false),
            P::Ya16Le => from_rgba16le(p.width, p.height, &plane.data, 2, plane.stride, true),
            P::Rgb48Le => from_rgba16le(p.width, p.height, &plane.data, 3, plane.stride, false),
            P::Rgba64Le => from_rgba16le(p.width, p.height, &plane.data, 4, plane.stride, true),
            _ => {
                let alpha = matches!(p.format, P::Ya8 | P::Rgba);
                from_rgba8(p.width, p.height, &p.to_rgba8(), alpha)
            }
        };
        if matches!(
            p.format,
            P::Gray8 | P::Gray16Le | P::Ya8 | P::Ya16Le | P::MonoBlack
        ) {
            img.colorspace = Colorspace::Gray;
        }
        if p.format == P::MonoBlack {
            img.depth = 1;
        }
        out.push(img);
    }
    Ok(out)
}

fn decode_farbfeld(data: &[u8]) -> MResult<Vec<Image>> {
    if data.len() < 16 || &data[..8] != b"farbfeld" {
        return Err(corrupt("FARBFELD", "improper image header"));
    }
    let w = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let h = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let need = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(8))
        .ok_or_else(|| corrupt("FARBFELD", "image too large"))?;
    if data.len() < 16 + need {
        return Err(corrupt("FARBFELD", "insufficient image data in file"));
    }
    let mut img = Image::blank(w, h);
    img.alpha = true;
    for (i, px) in img.pixels.chunks_exact_mut(4).enumerate() {
        let s = &data[16 + i * 8..];
        for (c, v) in px.iter_mut().enumerate() {
            *v = u16::from_be_bytes([s[c * 2], s[c * 2 + 1]]);
        }
    }
    Ok(vec![img])
}

/// The channel layout of the raw formats: (map, gray, cmyk, opacity).
fn raw_map(m: &str) -> &'static str {
    match m {
        "RGB" => "RGB",
        "RGBA" => "RGBA",
        "RGBO" => "RGBO",
        "BGR" => "BGR",
        "BGRA" => "BGRA",
        "GRAY" => "I",
        "GRAYA" => "IA",
        "CMYK" => "CMYK",
        _ => "CMYKA",
    }
}

fn decode_raw(data: &[u8], m: &str, opts: &ReadOptions) -> MResult<Vec<Image>> {
    let Some((w, h)) = opts.size else {
        return Err(MagickError::with_desc(
            severity::OPTION_ERROR,
            "MustSpecifyImageSize",
            "",
            "error/raw.c/ReadRAWImage",
        ));
    };
    let depth = opts.depth.unwrap_or(8);
    let map = raw_map(m);
    let n = map.len();
    let bps = if depth > 8 { 2 } else { 1 };
    if data.len() < w * h * n * bps {
        return Err(MagickError::with_desc(
            severity::CORRUPT_IMAGE_ERROR,
            "UnexpectedEndOfFile",
            "",
            "error/raw.c/ReadRAWImage",
        ));
    }
    let mut img = Image::blank(w, h);
    let mut samples = Vec::with_capacity(w * h * n);
    for i in 0..w * h * n {
        samples.push(if bps == 2 {
            u16::from_be_bytes([data[i * 2], data[i * 2 + 1]])
        } else {
            data[i] as u16 * 257
        });
    }
    crate::pixels::import_q16(&mut img, 0, 0, w, h, map, &samples)?;
    img.depth = depth;
    Ok(vec![img])
}

// ---------------------------------------------------------------------------
// Encoding

/// Options that affect encoding.
#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    /// `-define` style options ("png:bit-depth", "webp:lossless"...).
    pub defines: Vec<(String, String)>,
}

impl WriteOptions {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.defines
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }
}

/// Encode images (one, or all when the format holds several and
/// `adjoin`) in a format.
pub fn encode(images: &[&Image], magick: &str, opts: &WriteOptions) -> MResult<Vec<u8>> {
    let m = canonical_format(magick);
    let converted: Vec<std::borrow::Cow<'_, Image>> = images
        .iter()
        .map(|i| crate::colorspace::for_output(i))
        .collect();
    let refs: Vec<&Image> = converted.iter().map(|c| c.as_ref()).collect();
    let images = &refs[..];
    let first = images[0];
    match m.as_str() {
        "PNG" | "PNG8" | "PNG24" | "PNG32" | "PNG48" | "PNG64" | "PNG00" => {
            encode_png(first, &m, opts)
        }
        "JPEG" | "JPG" => encode_jpeg(first, opts),
        "GIF" | "GIF87" => encode_gif(images, &m),
        "WEBP" => encode_webp(images, opts),
        "BMP" | "BMP2" | "BMP3" => encode_bmp(first),
        "TIFF" | "TIF" => encode_tiff(images),
        "ICO" | "CUR" => encode_ico(images, &m),
        "TGA" => encode_tga(first),
        "QOI" => encode_qoi(first),
        "PNM" | "PPM" | "PGM" | "PBM" | "PAM" => encode_pnm(first, &m),
        "FARBFELD" => Ok(encode_farbfeld(first)),
        "RGB" | "RGBA" | "RGBO" | "BGR" | "BGRA" | "GRAY" | "GRAYA" | "CMYK" | "CMYKA" => {
            encode_raw(first, &m)
        }
        "TXT" => Ok(crate::properties::encode_txt(first)),
        "NULL" => Ok(Vec::new()),
        _ => Err(MagickError::no_encode_delegate(magick)),
    }
}

fn rgb8(img: &Image) -> Vec<u8> {
    let rgba = img.to_rgba8();
    let mut out = Vec::with_capacity(img.columns * img.rows * 3);
    for p in rgba.chunks_exact(4) {
        out.extend_from_slice(&p[..3]);
    }
    out
}

fn gray8(img: &Image) -> Vec<u8> {
    let rgba = img.to_rgba8();
    rgba.chunks_exact(4).map(|p| p[0]).collect()
}

fn le16(vals: impl Iterator<Item = u16>) -> Vec<u8> {
    let mut out = Vec::new();
    for v in vals {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn is_gray_image(img: &Image) -> bool {
    img.colorspace == Colorspace::Gray
        || matches!(
            img.image_type,
            ImageType::Grayscale | ImageType::GrayscaleAlpha | ImageType::Bilevel
        )
}

/// A sample at 8 bits as ImageMagick's PNG writer stores it (truncated).
fn png8(v: u16) -> u8 {
    (v / 257) as u8
}

/// The smallest PNG sample depth (1, 2, 4 or 8) holding these 8-bit
/// gray levels exactly.
fn png_gray_depth(levels: impl Iterator<Item = u8> + Clone) -> u8 {
    for d in [1u8, 2, 4] {
        let step = 255 / ((1u32 << d) - 1);
        if levels.clone().all(|v| (v as u32).is_multiple_of(step)) {
            return d;
        }
    }
    8
}

/// The colour type, depth and samples ImageMagick 7 picks for a PNG
/// when nothing forces them (black-box): gray when every pixel is,
/// at the least depth holding its levels (a tRNS key for a lone fully
/// transparent gray, else gray+alpha); a palette for at most 256
/// colours (in first-appearance order, the background appended when
/// there is room), at the least index depth; else RGB, or RGBA when
/// the image has an alpha channel; 16 bits only for a 16-bit image
/// whose samples need them.
struct PngLayout {
    format: oxideav_png::PixelFormat,
    stride: usize,
    data: Vec<u8>,
    bit_depth: Option<u8>,
    palette: Option<Vec<[u8; 4]>>,
    bkgd: Option<oxideav_png::Bkgd>,
    trns: Option<oxideav_png::Trns>,
}

fn png_auto_layout(img: &Image) -> PngLayout {
    use oxideav_png::{Bkgd, PixelFormat as P, Trns};
    let w = img.columns;
    let px = || img.pixels.chunks_exact(4);
    let opaque = !img.alpha || px().all(|p| p[3] == 65535);
    // 16 bits only when the colour samples need them (an alpha that does
    // not fit 8 bits is truncated, as in a shadowImage() result)
    let needs16 = img.depth > 8 && px().any(|p| p[..3].iter().any(|v| v % 257 != 0));
    let bg = img.background_color.to_rgba16();
    if needs16 {
        let gray = px().all(|p| p[0] == p[1] && p[1] == p[2]);
        // a lone fully transparent gray, unused by the opaque pixels
        let key = if gray && !opaque && px().all(|p| p[3] == 0 || p[3] == 65535) {
            let t: Vec<u16> = px().filter(|p| p[3] == 0).map(|p| p[0]).collect();
            (t.iter().all(|&v| v == t[0]) && !px().any(|p| p[3] == 65535 && p[0] == t[0]))
                .then_some(t[0])
        } else {
            None
        };
        let gray_only = gray && (opaque || key.is_some());
        let (format, stride, data) = if gray_only {
            (P::Gray16Le, w * 2, le16(px().map(|p| p[0])))
        } else if img.alpha {
            // (ImageMagick writes 16-bit gray+alpha for gray pixels;
            // oxideav-png has no such format)
            (P::Rgba64Le, w * 8, le16(img.pixels.iter().copied()))
        } else {
            (
                P::Rgb48Le,
                w * 6,
                le16(px().flat_map(|p| [p[0], p[1], p[2]])),
            )
        };
        let bkgd = if gray_only {
            Bkgd::Grayscale(bg[0])
        } else {
            Bkgd::Rgb(bg[0], bg[1], bg[2])
        };
        return PngLayout {
            format,
            stride,
            data,
            bit_depth: None,
            palette: None,
            bkgd: Some(bkgd),
            trns: key.map(Trns::Grayscale),
        };
    }
    let p8: Vec<[u8; 4]> = px()
        .map(|p| {
            [
                png8(p[0]),
                png8(p[1]),
                png8(p[2]),
                if img.alpha { png8(p[3]) } else { 255 },
            ]
        })
        .collect();
    let bg8 = [png8(bg[0]), png8(bg[1]), png8(bg[2])];
    let gray = p8.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
    if gray {
        let binary = p8.iter().all(|p| p[3] == 0 || p[3] == 255);
        let mut key = None;
        if !opaque && binary {
            // one gray for every transparent pixel, unused by opaque ones
            let t: Vec<u8> = p8.iter().filter(|p| p[3] == 0).map(|p| p[0]).collect();
            if t.iter().all(|&v| v == t[0]) && !p8.iter().any(|p| p[3] == 255 && p[0] == t[0]) {
                key = Some(t[0]);
            }
        }
        if opaque || key.is_some() {
            let d = png_gray_depth(p8.iter().map(|p| p[0]).chain([bg8[0]]));
            let scale = 255 / ((1u32 << d) - 1) as u8;
            let data: Vec<u8> = p8.iter().map(|p| p[0] / scale).collect();
            return PngLayout {
                format: P::Gray8,
                stride: w,
                data,
                bit_depth: (d < 8).then_some(d),
                palette: None,
                bkgd: Some(Bkgd::Grayscale((bg8[0] / scale) as u16)),
                trns: key.map(|k| Trns::Grayscale((k / scale) as u16)),
            };
        }
        return PngLayout {
            format: P::Ya8,
            stride: w * 2,
            data: p8.iter().flat_map(|p| [p[0], p[3]]).collect(),
            bit_depth: None,
            palette: None,
            bkgd: Some(Bkgd::Grayscale(bg8[0] as u16)),
            trns: None,
        };
    }
    // a palette, colours in the order they first appear
    let mut index = std::collections::HashMap::new();
    let mut entries: Vec<[u8; 4]> = Vec::new();
    let mut fits = true;
    for p in &p8 {
        if !index.contains_key(p) {
            if entries.len() == 256 {
                fits = false;
                break;
            }
            index.insert(*p, entries.len());
            entries.push(*p);
        }
    }
    if fits {
        // the entries with transparency lead (a shorter tRNS)
        let mut order: Vec<usize> = (0..entries.len())
            .filter(|&i| entries[i][3] != 255)
            .collect();
        order.extend((0..entries.len()).filter(|&i| entries[i][3] == 255));
        let mut remap = vec![0usize; entries.len()];
        for (new, &old) in order.iter().enumerate() {
            remap[old] = new;
        }
        entries = order.iter().map(|&i| entries[i]).collect();
        for v in index.values_mut() {
            *v = remap[*v];
        }
        let bgp = [bg8[0], bg8[1], bg8[2], 255];
        let bkgd = match index.get(&bgp) {
            Some(&i) => Some(i),
            None if entries.len() < 256 => {
                entries.push(bgp);
                Some(entries.len() - 1)
            }
            None => None,
        };
        let d = match entries.len() {
            0..=2 => 1,
            3..=4 => 2,
            5..=16 => 4,
            _ => 8,
        };
        let data: Vec<u8> = p8.iter().map(|p| index[p] as u8).collect();
        return PngLayout {
            format: P::Pal8,
            stride: w,
            data,
            bit_depth: (d < 8).then_some(d),
            palette: Some(entries),
            bkgd: bkgd.map(|i| Bkgd::Palette(i as u8)),
            trns: None,
        };
    }
    let bkgd = Some(Bkgd::Rgb(bg8[0] as u16, bg8[1] as u16, bg8[2] as u16));
    if img.alpha {
        PngLayout {
            format: P::Rgba,
            stride: w * 4,
            data: p8.iter().flatten().copied().collect(),
            bit_depth: None,
            palette: None,
            bkgd,
            trns: None,
        }
    } else {
        PngLayout {
            format: P::Rgb24,
            stride: w * 3,
            data: p8.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(),
            bit_depth: None,
            palette: None,
            bkgd,
            trns: None,
        }
    }
}

/// PNG8 for an opaque image of at most 256 colours: an 8-bit palette of
/// the image's colours (more colours, or alpha, are a quantize matter).
fn png8_layout(img: &Image) -> Option<PngLayout> {
    use oxideav_png::{Bkgd, PixelFormat as P};
    if img.alpha && !img.is_opaque() {
        return None;
    }
    let mut index = std::collections::HashMap::new();
    let mut entries: Vec<[u8; 4]> = Vec::new();
    let mut data = Vec::with_capacity(img.columns * img.rows);
    for p in img.pixels.chunks_exact(4) {
        let e = [png8(p[0]), png8(p[1]), png8(p[2]), 255];
        let i = match index.get(&e) {
            Some(&i) => i,
            None => {
                if entries.len() == 256 {
                    return None;
                }
                index.insert(e, entries.len());
                entries.push(e);
                entries.len() - 1
            }
        };
        data.push(i as u8);
    }
    let bg = img.background_color.to_rgba16();
    let bgp = [png8(bg[0]), png8(bg[1]), png8(bg[2]), 255];
    Some(PngLayout {
        format: P::Pal8,
        stride: img.columns,
        data,
        bit_depth: None,
        palette: Some(entries),
        bkgd: index.get(&bgp).map(|&i| Bkgd::Palette(i as u8)),
        trns: None,
    })
}

fn encode_png(img: &Image, m: &str, opts: &WriteOptions) -> MResult<Vec<u8>> {
    use oxideav_png::{PixelFormat as P, Plane, PngImage};
    let mut img = img.clone();
    // cHRM goes with primaries (gray and linear images have none)
    let chrm = img.chromaticity.white.0 > 0.0;
    img.cmyk_to_rgb();
    let (w, h) = (img.columns as u32, img.rows as u32);
    let bit_depth_opt = opts.get("png:bit-depth");
    let ct_opt = opts.get("png:color-type");
    let forced = matches!(m, "PNG24" | "PNG32" | "PNG48" | "PNG64")
        || bit_depth_opt.is_some_and(|d| d == "16")
        || ct_opt.is_some_and(|c| c != "0" || is_gray_image(&img));
    let layout = if forced {
        None
    } else if m == "PNG8" {
        png8_layout(&img)
    } else {
        let mut l = png_auto_layout(&img);
        if bit_depth_opt == Some("8") {
            // png:bit-depth=8: the same choices at 8 bits a sample
            if let (Some(d), P::Gray8) = (l.bit_depth, l.format) {
                let scale = 255 / ((1u32 << d) - 1) as u8;
                l.data.iter_mut().for_each(|v| *v *= scale);
                l.bkgd = l.bkgd.map(|b| match b {
                    oxideav_png::Bkgd::Grayscale(v) => {
                        oxideav_png::Bkgd::Grayscale(v * scale as u16)
                    }
                    o => o,
                });
                l.trns = l.trns.map(|t| match t {
                    oxideav_png::Trns::Grayscale(v) => {
                        oxideav_png::Trns::Grayscale(v * scale as u16)
                    }
                    o => o,
                });
            }
            l.bit_depth = None;
        }
        Some(l)
    };
    let has_alpha = img.alpha && !img.is_opaque();
    let depth16 = match m {
        "PNG48" | "PNG64" => true,
        "PNG8" | "PNG24" | "PNG32" => false,
        _ => {
            opts.get("png:bit-depth")
                .map(|d| d == "16")
                .unwrap_or(false)
                || (img.depth == 16
                    && img.pixels.iter().any(|&v| v % 257 != 0)
                    && opts.get("png:bit-depth").is_none())
        }
    };
    let gray =
        is_gray_image(&img) && img.is_gray() && !matches!(m, "PNG24" | "PNG32" | "PNG48" | "PNG64");
    let want_alpha = match m {
        "PNG32" | "PNG64" => true,
        "PNG24" | "PNG48" => false,
        _ => has_alpha || matches!(ct_opt, Some("4") | Some("6")),
    };
    let n = img.columns * img.rows;
    // ImageMagick writes a gray image with alpha as 8-bit gray+alpha
    // whatever its depth, each sample truncated to 8 bits
    let gray_alpha8 = img.is_gray()
        && want_alpha
        && !matches!(m, "PNG48" | "PNG64")
        && opts.get("png:bit-depth").is_none()
        && opts.get("png:color-type").is_none();
    let (format, stride, data) = if gray_alpha8 {
        (
            P::Ya8,
            w as usize * 2,
            img.pixels
                .chunks_exact(4)
                .flat_map(|p| [(p[0] / 257) as u8, (p[3] / 257) as u8])
                .collect(),
        )
    } else if depth16 {
        if gray && !want_alpha {
            (
                P::Gray16Le,
                w as usize * 2,
                le16(img.pixels.chunks_exact(4).map(|p| p[0])),
            )
        } else if want_alpha {
            (
                P::Rgba64Le,
                w as usize * 8,
                le16(img.pixels.iter().copied()),
            )
        } else {
            (
                P::Rgb48Le,
                w as usize * 6,
                le16(img.pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]])),
            )
        }
    } else if gray && !want_alpha {
        (
            P::Gray8,
            w as usize,
            img.pixels.chunks_exact(4).map(|p| q_to_u8(p[0])).collect(),
        )
    } else if gray && want_alpha {
        let rgba: Vec<u8> = img.pixels.iter().map(|&v| q_to_u8(v)).collect();
        (
            P::Ya8,
            w as usize * 2,
            rgba.chunks_exact(4).flat_map(|p| [p[0], p[3]]).collect(),
        )
    } else if want_alpha {
        (
            P::Rgba,
            w as usize * 4,
            img.pixels.iter().map(|&v| q_to_u8(v)).collect(),
        )
    } else {
        (
            P::Rgb24,
            w as usize * 3,
            img.pixels
                .chunks_exact(4)
                .flat_map(|p| [q_to_u8(p[0]), q_to_u8(p[1]), q_to_u8(p[2])])
                .collect(),
        )
    };
    let _ = n;
    let (format, stride, data, bit_depth, palette, bkgd, trns) = match layout {
        Some(l) => (
            l.format,
            l.stride,
            l.data,
            l.bit_depth,
            l.palette,
            l.bkgd,
            l.trns,
        ),
        None => {
            let bg = img.background_color.to_rgba16();
            let bkgd = match format {
                P::Gray16Le => oxideav_png::Bkgd::Grayscale(bg[0]),
                P::Gray8 | P::Ya8 => oxideav_png::Bkgd::Grayscale(q_to_u8(bg[0]) as u16),
                P::Rgb48Le | P::Rgba64Le => oxideav_png::Bkgd::Rgb(bg[0], bg[1], bg[2]),
                _ => oxideav_png::Bkgd::Rgb(
                    q_to_u8(bg[0]) as u16,
                    q_to_u8(bg[1]) as u16,
                    q_to_u8(bg[2]) as u16,
                ),
            };
            let key = (matches!(m, "PNG24" | "PNG48") && has_alpha)
                .then_some(oxideav_png::Trns::Rgb(0, 0, 0));
            (format, stride, data, None, None, Some(bkgd), key)
        }
    };
    let mut png = PngImage::new(w, h, format, vec![Plane::new(stride, data)])
        .map_err(|e| coder_err("PNG", e))?;
    if let Some(entries) = palette {
        png = png.with_palette(oxideav_png::Palette::new(entries));
    }
    let mut md = oxideav_png::Metadata::default();
    if let Some(icc) = img.profile("icc") {
        md = md.with_icc(icc.to_vec());
    }
    if let Some(exif) = img.profile("exif") {
        md = md.with_exif(exif.to_vec());
    }
    if let Some(xmp) = img.profile("xmp") {
        // an iTXt chunk: no NUL (a JPEG APP1 packet may carry padding)
        let end = xmp.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
        let xmp = &xmp[..end];
        if !xmp.is_empty() && !xmp.contains(&0) {
            md = md.with_xmp(xmp.to_vec());
        }
    }
    png = png.with_metadata(md);
    let mut md_exif_drop = false;
    let mut eo = oxideav_png::EncodeOptions::default();
    let level = if img.quality > 0 {
        ((img.quality / 10).clamp(1, 9)) as u8
    } else {
        7
    };
    eo = eo.with_level(level);
    if let Some(d) = bit_depth {
        eo = eo.with_bit_depth(d);
    }
    let mut pm = oxideav_png::PngMetadata::default();
    pm.bkgd = bkgd;
    pm.trns = trns;
    if chrm {
        let c = &img.chromaticity;
        let q = |v: f64| (v * 100_000.0 + 0.5) as u32;
        pm.chrm = Some(oxideav_png::Chrm::new(
            (q(c.white.0), q(c.white.1)),
            (q(c.red.0), q(c.red.1)),
            (q(c.green.0), q(c.green.1)),
            (q(c.blue.0), q(c.blue.1)),
        ));
    }
    if img.x_resolution > 0.0 && img.y_resolution > 0.0 {
        let (xr, yr) = match img.units {
            ResolutionType::PixelsPerInch => (img.x_resolution / 0.0254, img.y_resolution / 0.0254),
            ResolutionType::PixelsPerCentimeter => {
                (img.x_resolution * 100.0, img.y_resolution * 100.0)
            }
            _ => (img.x_resolution, img.y_resolution),
        };
        let unit = if img.units == ResolutionType::Undefined {
            oxideav_png::PhysUnit::Unknown
        } else {
            oxideav_png::PhysUnit::Metre
        };
        // (truncated, as ImageMagick writes it)
        pm.phys = Some(oxideav_png::Phys::new(
            (xr + 1e-6) as u32,
            (yr + 1e-6) as u32,
            unit,
        ));
    }
    // every property but the png:* ones, as ImageMagick lists them
    let mut props: Vec<&(String, String)> = img
        .properties
        .0
        .iter()
        .filter(|(k, _)| !k.to_ascii_lowercase().starts_with("png:"))
        .collect();
    props.sort_by_key(|(k, _)| k.to_ascii_lowercase());
    // tIME for an image that is dated (read, or made by a coder)
    if img
        .properties
        .0
        .iter()
        .any(|(k, _)| k.starts_with("date:") || k == "png:tIME")
    {
        let (y, mo, d, hh, mm, ss) = crate::properties::civil_utc(crate::properties::now_secs());
        pm.time = Some(oxideav_png::Time::new(y as u16, mo, d, hh, mm, ss));
    }
    for (k, v) in props {
        let valid_key = !k.is_empty()
            && k.len() < 80
            && k.bytes().all(|b| (0x20..0x7f).contains(&b))
            && !k.starts_with(' ')
            && !k.ends_with(' ')
            && !k.contains("  ");
        if valid_key && v.is_ascii() && !v.contains('\0') {
            pm.texts.push(oxideav_png::Text::new(k.clone(), v.clone()));
        }
    }
    // ImageMagick's private orNT chunk: the orientation
    if img.orientation != OrientationType::Undefined {
        pm.unknowns.push(oxideav_png::UnknownChunk::new(
            *b"orNT",
            vec![img.orientation.0 as u8],
            false,
        ));
    }
    // ImageMagick's private caNv chunk carries a page other than the
    // image's own size at the origin: width, height (u32), x, y (i32)
    let pg = img.page;
    if (pg.width != 0 || pg.height != 0 || pg.x != 0 || pg.y != 0)
        && !(pg.width == img.columns && pg.height == img.rows && pg.x == 0 && pg.y == 0)
    {
        let mut d = Vec::with_capacity(16);
        d.extend_from_slice(&(pg.width as u32).to_be_bytes());
        d.extend_from_slice(&(pg.height as u32).to_be_bytes());
        d.extend_from_slice(&(pg.x as i32).to_be_bytes());
        d.extend_from_slice(&(pg.y as i32).to_be_bytes());
        pm.unknowns
            .push(oxideav_png::UnknownChunk::new(*b"caNv", d, false));
    }
    // -define png:exclude-chunk (StripImage() sets it)
    if let Some(ex) = img
        .artifacts
        .get("png:exclude-chunk")
        .or_else(|| opts.get("png:exclude-chunk"))
    {
        let ex: Vec<String> = ex
            .split(',')
            .map(|c| c.trim().to_ascii_lowercase())
            .collect();
        let has = |c: &str| ex.iter().any(|e| e == c || e == "all");
        if has("bkgd") {
            pm.bkgd = None;
        }
        if has("chrm") {
            pm.chrm = None;
        }
        if has("phys") {
            pm.phys = None;
        }
        if has("time") {
            pm.time = None;
        }
        if has("text") {
            pm.texts
                .retain(|t| t.keyword.starts_with("date:") && !has("date"));
        }
        if has("date") {
            pm.texts.retain(|t| !t.keyword.starts_with("date:"));
        }
        if has("exif") {
            md_exif_drop = true;
        }
    }
    if md_exif_drop {
        png = png.with_metadata(oxideav_png::Metadata::default());
    }
    eo = eo.with_metadata(pm);
    oxideav_png::encode(&png, &eo).map_err(|e| coder_err("PNG", e))
}

/// The chroma layout of a JPEG: the jpeg:sampling-factor define (its
/// first component's HxV), else 4:2:0 below quality 90 and 4:4:4 from
/// it, as ImageMagick picks.
fn jpeg_chroma(quality: u32, opts: &WriteOptions) -> oxideav_mjpeg::ChromaSubsampling {
    use oxideav_mjpeg::ChromaSubsampling as C;
    if let Some(sf) = opts.get("jpeg:sampling-factor") {
        let first = sf.split(',').next().unwrap_or("").trim();
        let named = match first {
            "4:4:4" => Some(C::Yuv444),
            "4:2:2" => Some(C::Yuv422),
            "4:2:0" => Some(C::Yuv420),
            "4:1:1" => Some(C::Yuv411),
            _ => None,
        };
        if let Some(c) = named {
            return c;
        }
        let mut it = first.split(['x', 'X']);
        let h: f64 = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(1.0);
        let v: f64 = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(h);
        return match (h as u32, v as u32) {
            (2, 2) => C::Yuv420,
            (2, 1) => C::Yuv422,
            (4, 1) => C::Yuv411,
            _ => C::Yuv444,
        };
    }
    if quality < 90 { C::Yuv420 } else { C::Yuv444 }
}

/// Insert a COM segment (the image's comment) after the JFIF header.
fn jpeg_with_comment(data: Vec<u8>, comment: &str) -> Vec<u8> {
    if comment.is_empty() || data.len() < 4 || comment.len() > 65533 {
        return data;
    }
    // after SOI and an APP0, where ImageMagick puts it
    let mut at = 2;
    if data[2] == 0xff && data[3] == 0xe0 && data.len() >= 6 {
        at = 4 + u16::from_be_bytes([data[4], data[5]]) as usize;
    }
    let mut out = Vec::with_capacity(data.len() + comment.len() + 4);
    out.extend_from_slice(&data[..at]);
    out.extend_from_slice(&[0xff, 0xfe]);
    out.extend_from_slice(&((comment.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(comment.as_bytes());
    out.extend_from_slice(&data[at..]);
    out
}

fn encode_jpeg(img: &Image, wopts: &WriteOptions) -> MResult<Vec<u8>> {
    use oxideav_mjpeg::{ColorSignalling, JpegImage, PixelFormat as P, Plane};
    let img = img.clone();
    let quality = if img.quality == 0 {
        92
    } else {
        img.quality.clamp(1, 100)
    };
    let (w, h) = (img.columns as u32, img.rows as u32);
    let mut opts = oxideav_mjpeg::EncodeOptions::default().with_quality(quality as u8);
    if img.interlace != InterlaceType::No && img.interlace != InterlaceType::Undefined {
        opts = opts.with_progressive(true);
    }
    let mut md = oxideav_mjpeg::Metadata::default();
    if let Some(icc) = img.profile("icc") {
        md = md.with_icc(icc.to_vec());
    }
    if let Some(exif) = img.profile("exif") {
        md = md.with_exif(exif.to_vec());
    }
    if let Some(xmp) = img.profile("xmp") {
        md = md.with_xmp(xmp.to_vec());
    }
    opts = opts.with_metadata(md);
    let out = if img.colorspace == Colorspace::CMYK {
        // Adobe CMYK: APP14 transform 0, the samples inverted on the wire
        let k = img
            .black
            .clone()
            .unwrap_or_else(|| vec![0; img.columns * img.rows]);
        let mut data = Vec::with_capacity(img.columns * img.rows * 4);
        for (p, &kk) in img.pixels.chunks_exact(4).zip(k.iter()) {
            data.extend_from_slice(&[q_to_u8(p[0]), q_to_u8(p[1]), q_to_u8(p[2]), q_to_u8(kk)]);
        }
        let ji = JpegImage::new(w, h, P::Cmyk, vec![Plane::new(w as usize * 4, data)])
            .map_err(|e| coder_err("JPEG", e))?;
        let opts = opts.with_signalling(ColorSignalling::Cmyk {
            adobe_transform: Some(0),
        });
        oxideav_mjpeg::encode(&ji, &opts)
    } else if is_gray_image(&img) && img.is_gray() {
        let ji = JpegImage::new(w, h, P::Gray8, vec![Plane::new(w as usize, gray8(&img))])
            .map_err(|e| coder_err("JPEG", e))?;
        oxideav_mjpeg::encode(&ji, &opts)
    } else {
        // JFIF YCbCr; an alpha channel is dropped (ImageMagick writes the
        // colour samples as they are)
        let opts = opts.with_chroma(jpeg_chroma(quality, wopts));
        oxideav_mjpeg::encode_rgb8(w, h, &rgb8(&img), &opts)
    }
    .map_err(|e| coder_err("JPEG", e))?;
    let out = set_jfif_density(out, &img);
    Ok(match img.properties.get("comment") {
        Some(c) if c.is_ascii() => jpeg_with_comment(out, c),
        _ => out,
    })
}

/// Patch the JFIF APP0 density of an encoded JPEG.
fn set_jfif_density(mut out: Vec<u8>, img: &Image) -> Vec<u8> {
    if out.len() > 20
        && &out[6..11] == b"JFIF\0"
        && img.x_resolution > 0.0
        && img.y_resolution > 0.0
    {
        out[13] = match img.units {
            ResolutionType::PixelsPerInch => 1,
            ResolutionType::PixelsPerCentimeter => 2,
            _ => 0,
        };
        let xr = (img.x_resolution.round() as u32).min(65535) as u16;
        let yr = (img.y_resolution.round() as u32).min(65535) as u16;
        out[14..16].copy_from_slice(&xr.to_be_bytes());
        out[16..18].copy_from_slice(&yr.to_be_bytes());
    }
    out
}

fn encode_gif(images: &[&Image], m: &str) -> MResult<Vec<u8>> {
    use oxideav_gif::{DisposalMethod, Frame, GifImage};
    // frames of one size; sub-images are placed on their page canvas
    let (cw, ch) = {
        let p = images[0].page;
        if images.len() > 1 && p.width > 0 && p.height > 0 {
            (p.width, p.height)
        } else {
            (images[0].columns, images[0].rows)
        }
    };
    let single = images.len() == 1 || m == "GIF87";
    let mut frames = Vec::new();
    for (i, img) in images.iter().enumerate() {
        if single && i > 0 {
            break;
        }
        let mut img = (*img).clone();
        img.cmyk_to_rgb();
        let rgba = if single
            || (img.columns == cw && img.rows == ch && img.page.x == 0 && img.page.y == 0)
        {
            let mut v = img.to_rgba8();
            if !img.alpha {
                for p in v.chunks_exact_mut(4) {
                    p[3] = 255;
                }
            }
            v
        } else {
            let mut canvas = vec![0u8; cw * ch * 4];
            let src = img.to_rgba8();
            for y in 0..img.rows {
                let ty = y as i64 + img.page.y;
                if ty < 0 || ty >= ch as i64 {
                    continue;
                }
                for x in 0..img.columns {
                    let tx = x as i64 + img.page.x;
                    if tx < 0 || tx >= cw as i64 {
                        continue;
                    }
                    let s = (y * img.columns + x) * 4;
                    let d = (ty as usize * cw + tx as usize) * 4;
                    canvas[d..d + 4].copy_from_slice(&src[s..s + 4]);
                }
            }
            canvas
        };
        let (w, h) = if single {
            (img.columns as u32, img.rows as u32)
        } else {
            (cw as u32, ch as u32)
        };
        let gi = GifImage::from_rgba8(w, h, rgba).map_err(|e| coder_err("GIF", e))?;
        let delay = if img.delay > 0 && !single {
            let tps = if img.ticks_per_second > 0 {
                img.ticks_per_second as u64
            } else {
                100
            };
            Some(std::time::Duration::from_millis(img.delay * 1000 / tps))
        } else if !single {
            Some(std::time::Duration::from_millis(0))
        } else {
            None
        };
        let disposal = match img.dispose {
            DisposeType::No => DisposalMethod::Keep,
            DisposeType::Background => DisposalMethod::RestoreBackground,
            DisposeType::Previous => DisposalMethod::RestorePrevious,
            _ => DisposalMethod::None,
        };
        frames.push(Frame::new(gi, delay).with_disposal(disposal));
    }
    let iterations = images[0].iterations;
    let opts = oxideav_gif::EncodeOptions::default().with_loop_count(if single {
        None
    } else {
        Some(iterations.min(65535) as u16)
    });
    if frames.len() == 1 {
        let fr = frames.pop().unwrap();
        return oxideav_gif::encode(&fr.image, &opts).map_err(|e| coder_err("GIF", e));
    }
    oxideav_gif::encode_all(&frames, &opts).map_err(|e| coder_err("GIF", e))
}

fn encode_webp(images: &[&Image], opts: &WriteOptions) -> MResult<Vec<u8>> {
    use oxideav_webp::{Frame, WebpImage};
    let first = images[0];
    let lossless = opts
        .get("webp:lossless")
        .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1")
        || first.quality >= 100;
    let mut eo = oxideav_webp::EncodeOptions::default();
    if !lossless {
        let q = if first.quality == 0 {
            75
        } else {
            first.quality
        };
        eo = eo.with_quality(q as f32);
    } else {
        eo = eo.with_lossless();
    }
    let mk = |img: &Image| -> MResult<WebpImage> {
        let mut img = img.clone();
        img.cmyk_to_rgb();
        if img.alpha && !img.is_opaque() {
            WebpImage::from_rgba8(img.columns as u32, img.rows as u32, img.to_rgba8())
                .map_err(|e| coder_err("WEBP", e))
        } else {
            WebpImage::from_rgb8(img.columns as u32, img.rows as u32, rgb8(&img))
                .map_err(|e| coder_err("WEBP", e))
        }
    };
    if images.len() == 1 {
        return oxideav_webp::encode(&mk(first)?, &eo).map_err(|e| coder_err("WEBP", e));
    }
    let mut frames = Vec::new();
    for img in images {
        let tps = if img.ticks_per_second > 0 {
            img.ticks_per_second as u64
        } else {
            100
        };
        let mut i2 = (*img).clone();
        i2.enable_alpha();
        let wi = WebpImage::from_rgba8(i2.columns as u32, i2.rows as u32, i2.to_rgba8())
            .map_err(|e| coder_err("WEBP", e))?;
        frames.push(Frame::new(
            wi,
            Some(std::time::Duration::from_millis(img.delay * 1000 / tps)),
        ));
    }
    let eo = oxideav_webp::EncodeOptions::default()
        .with_lossless()
        .with_loop_count(first.iterations.min(65535) as u16);
    oxideav_webp::encode_all(&frames, &eo).map_err(|e| coder_err("WEBP", e))
}

fn encode_bmp(img: &Image) -> MResult<Vec<u8>> {
    let mut img = img.clone();
    img.cmyk_to_rgb();
    let (w, h) = (img.columns as u32, img.rows as u32);
    let opts = oxideav_bmp::EncodeOptions::default();
    let mut out = if img.alpha && !img.is_opaque() {
        oxideav_bmp::encode_rgba8(w, h, &img.to_rgba8(), &opts)
    } else {
        oxideav_bmp::encode_rgb8(w, h, &rgb8(&img), &opts)
    }
    .map_err(|e| coder_err("BMP", e))?;
    if out.len() > 46 && img.x_resolution > 0.0 {
        let f = match img.units {
            ResolutionType::PixelsPerInch => 100.0 / 2.54,
            ResolutionType::PixelsPerCentimeter => 100.0,
            _ => 1.0,
        };
        let xr = (img.x_resolution * f).round() as i32;
        let yr = (img.y_resolution * f).round() as i32;
        out[38..42].copy_from_slice(&xr.to_le_bytes());
        out[42..46].copy_from_slice(&yr.to_le_bytes());
    }
    Ok(out)
}

fn tiff_image(img: &Image) -> MResult<oxideav_tiff::TiffImage> {
    use oxideav_tiff::{PixelFormat as P, Plane, TiffImage};
    let (w, h) = (img.columns as u32, img.rows as u32);
    let (format, stride, data) = if img.colorspace == Colorspace::CMYK {
        let k = img
            .black
            .clone()
            .unwrap_or_else(|| vec![0; img.columns * img.rows]);
        let mut data = Vec::new();
        for (p, &kk) in img.pixels.chunks_exact(4).zip(k.iter()) {
            data.extend_from_slice(&[q_to_u8(p[0]), q_to_u8(p[1]), q_to_u8(p[2]), q_to_u8(kk)]);
        }
        (P::Cmyk, w as usize * 4, data)
    } else if is_gray_image(img) && img.is_gray() && !img.alpha {
        if img.depth == 16 {
            (
                P::Gray16Le,
                w as usize * 2,
                le16(img.pixels.chunks_exact(4).map(|p| p[0])),
            )
        } else {
            (P::Gray8, w as usize, gray8(img))
        }
    } else if img.alpha {
        (P::Rgba, w as usize * 4, img.to_rgba8())
    } else if img.depth == 16 {
        (
            P::Rgb48Le,
            w as usize * 6,
            le16(img.pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]])),
        )
    } else {
        (P::Rgb24, w as usize * 3, rgb8(img))
    };
    TiffImage::new(w, h, format, vec![Plane::new(stride, data)]).map_err(|e| coder_err("TIFF", e))
}

fn encode_tiff(images: &[&Image]) -> MResult<Vec<u8>> {
    let first = images[0];
    let compression = match first.compression {
        CompressionType::No => oxideav_tiff::TiffCompression::None,
        CompressionType::Zip => oxideav_tiff::TiffCompression::Deflate,
        CompressionType::RLE => oxideav_tiff::TiffCompression::PackBits,
        CompressionType::LZW => oxideav_tiff::TiffCompression::Lzw,
        _ => oxideav_tiff::TiffCompression::None,
    };
    let mut opts = oxideav_tiff::EncodeOptions::default().with_compression(compression);
    if first.x_resolution > 0.0 && first.y_resolution > 0.0 {
        let unit = match first.units {
            ResolutionType::PixelsPerInch => 2,
            ResolutionType::PixelsPerCentimeter => 3,
            _ => 1,
        };
        let r = |v: f64| ((v * 10000.0).round() as u32, 10000u32);
        opts = opts.with_resolution(oxideav_tiff::PageResolution {
            x: r(first.x_resolution),
            y: r(first.y_resolution),
            unit,
        });
    }
    if images.len() == 1 {
        return oxideav_tiff::encode(&tiff_image(first)?, &opts).map_err(|e| coder_err("TIFF", e));
    }
    let mut frames = Vec::new();
    for img in images {
        frames.push(oxideav_tiff::Frame::new(
            tiff_image(img)?,
            frames.len() as u32,
        ));
    }
    oxideav_tiff::encode_all(&frames, &opts).map_err(|e| coder_err("TIFF", e))
}

fn encode_ico(images: &[&Image], m: &str) -> MResult<Vec<u8>> {
    let mut frames = Vec::new();
    for img in images {
        if img.columns > 256 || img.rows > 256 {
            return Err(MagickError::with_desc(
                severity::IMAGE_ERROR,
                "width or height exceeds limit",
                &format!("{}x{}", img.columns, img.rows),
                "error/icon.c/WriteICONImage",
            ));
        }
        let mut i2 = (*img).clone();
        i2.cmyk_to_rgb();
        i2.enable_alpha();
        let ii =
            oxideav_ico::IcoImage::from_rgba8(i2.columns as u32, i2.rows as u32, i2.to_rgba8())
                .map_err(|e| coder_err("ICON", e))?;
        frames.push(oxideav_ico::Frame::new(ii, frames.len() as u32));
    }
    let mut opts = oxideav_ico::EncodeOptions::default();
    if m == "CUR" {
        opts = opts.with_icon_type(oxideav_ico::IconType::Cur);
    }
    oxideav_ico::encode_all(&frames, &opts).map_err(|e| coder_err("ICON", e))
}

fn encode_tga(img: &Image) -> MResult<Vec<u8>> {
    let mut img = img.clone();
    img.cmyk_to_rgb();
    let (w, h) = (img.columns as u32, img.rows as u32);
    let opts = oxideav_tga::EncodeOptions::default();
    if img.alpha && !img.is_opaque() {
        oxideav_tga::encode_rgba8(w, h, &img.to_rgba8(), &opts)
    } else {
        oxideav_tga::encode_rgb8(w, h, &rgb8(&img), &opts)
    }
    .map_err(|e| coder_err("TGA", e))
}

fn encode_qoi(img: &Image) -> MResult<Vec<u8>> {
    let mut img = img.clone();
    img.cmyk_to_rgb();
    let (w, h) = (img.columns as u32, img.rows as u32);
    let opts = oxideav_qoi::EncodeOptions::default();
    if img.alpha {
        oxideav_qoi::encode_rgba8(w, h, &img.to_rgba8(), &opts)
    } else {
        oxideav_qoi::encode_rgb8(w, h, &rgb8(&img), &opts)
    }
    .map_err(|e| coder_err("QOI", e))
}

fn encode_pnm(img: &Image, m: &str) -> MResult<Vec<u8>> {
    use oxideav_pbm::{PbmImage, PixelFormat as P, Plane};
    let mut img = img.clone();
    img.cmyk_to_rgb();
    let (w, h) = (img.columns as u32, img.rows as u32);
    let opts = oxideav_pbm::EncodeOptions::default();
    let kind = match m {
        "PNM" => {
            if img.is_gray() {
                if img
                    .pixels
                    .chunks_exact(4)
                    .all(|p| p[0] == 0 || p[0] == 65535)
                {
                    "PBM"
                } else {
                    "PGM"
                }
            } else {
                "PPM"
            }
        }
        k => k,
    };
    let deep = img.depth == 16;
    match kind {
        "PBM" => {
            let stride = (w as usize).div_ceil(8);
            let mut data = vec![0u8; stride * h as usize];
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let p = img.px(x, y);
                    if Image::intensity(&p) < 32768.0 {
                        data[y * stride + x / 8] |= 0x80 >> (x % 8);
                    }
                }
            }
            let pi = PbmImage::new(w, h, P::MonoBlack, vec![Plane::new(stride, data)])
                .map_err(|e| coder_err("PNM", e))?;
            oxideav_pbm::encode(&pi, &opts)
        }
        "PGM" => {
            let pi = if deep {
                PbmImage::new(
                    w,
                    h,
                    P::Gray16Le,
                    vec![Plane::new(
                        w as usize * 2,
                        le16(img.pixels.chunks_exact(4).map(|p| p[0])),
                    )],
                )
            } else {
                PbmImage::new(w, h, P::Gray8, vec![Plane::new(w as usize, gray8(&img))])
            }
            .map_err(|e| coder_err("PNM", e))?;
            oxideav_pbm::encode(&pi, &opts)
        }
        "PAM" => {
            let pi = PbmImage::new(
                w,
                h,
                P::Rgba,
                vec![Plane::new(w as usize * 4, img.to_rgba8())],
            )
            .map_err(|e| coder_err("PNM", e))?;
            oxideav_pbm::encode(&pi, &opts.with_pam(true))
        }
        _ => {
            if deep {
                let pi = PbmImage::new(
                    w,
                    h,
                    P::Rgb48Le,
                    vec![Plane::new(
                        w as usize * 6,
                        le16(img.pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]])),
                    )],
                )
                .map_err(|e| coder_err("PNM", e))?;
                oxideav_pbm::encode(&pi, &opts)
            } else {
                oxideav_pbm::encode_rgb8(w, h, &rgb8(&img), &opts)
            }
        }
    }
    .map_err(|e| coder_err("PNM", e))
}

fn encode_farbfeld(img: &Image) -> Vec<u8> {
    let mut img = img.clone();
    img.cmyk_to_rgb();
    let mut out = Vec::with_capacity(16 + img.pixels.len() * 2);
    out.extend_from_slice(b"farbfeld");
    out.extend_from_slice(&(img.columns as u32).to_be_bytes());
    out.extend_from_slice(&(img.rows as u32).to_be_bytes());
    for p in img.pixels.chunks_exact(4) {
        for (c, &v) in p.iter().enumerate() {
            let v = if c == 3 && !img.alpha { 65535 } else { v };
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out
}

fn encode_raw(img: &Image, m: &str) -> MResult<Vec<u8>> {
    let map = raw_map(m);
    let samples = crate::pixels::export_q16(img, 0, 0, img.columns, img.rows, map)?;
    if img.depth > 8 {
        let mut out = Vec::with_capacity(samples.len() * 2);
        for v in samples {
            out.extend_from_slice(&v.to_be_bytes());
        }
        Ok(out)
    } else {
        Ok(samples.into_iter().map(q_to_u8).collect())
    }
}

/// 8-bit planar YCbCr (4:4:4, 4:2:2 or 4:2:0) to packed RGBA the way
/// libjpeg does it by default (and so ImageMagick): "fancy" triangle-
/// filter chroma upsampling, then the fixed-point (16-bit fraction)
/// YCbCr → RGB tables. oxideav-mjpeg's own conversion replicates chroma
/// samples (nearest neighbour) and rounds in exact arithmetic.
fn jpeg_ycc_to_rgba(jpg: &oxideav_mjpeg::JpegImage) -> Option<Vec<u8>> {
    use oxideav_mjpeg::PixelFormat as P;
    let (dh, dv) = match jpg.format {
        P::Yuv444P | P::YuvJ444P => (1, 1),
        P::Yuv422P | P::YuvJ422P => (2, 1),
        P::Yuv420P | P::YuvJ420P => (2, 2),
        _ => return None,
    };
    if jpg.planes.len() < 3 {
        return None;
    }
    let (cw, ch) = jpg.format.plane_dimensions(jpg.width, jpg.height, 1);
    let pl = |i: usize, w: usize, h: usize| JpegPlane {
        data: &jpg.planes[i].data,
        stride: jpg.planes[i].stride,
        w,
        h,
    };
    let (w, h) = (jpg.width as usize, jpg.height as usize);
    Some(ycc_planes_to_rgba(
        w,
        h,
        (dh, dv),
        &pl(0, w, h),
        &pl(1, cw, ch),
        &pl(2, cw, ch),
        true,
    ))
}

/// One component plane of a decoded JPEG.
struct JpegPlane<'a> {
    data: &'a [u8],
    stride: usize,
    w: usize,
    h: usize,
}

/// libjpeg's "fancy" chroma upsampling (h1v1, h2v1, h2v2) or sample
/// replication (other factors) of one plane to w × h.
fn jpeg_upsample(p: &JpegPlane, w: usize, h: usize, (dh, dv): (usize, usize)) -> Vec<u8> {
    let (cw, ch) = (p.w.max(1), p.h.max(1));
    let at = |x: usize, y: usize| p.data[y * p.stride + x] as i32;
    if (dh, dv) == (1, 2) {
        // libjpeg-turbo's h1v2 fancy upsampling: (3·nearer + further + 1
        // or 2) / 4 down each column
        let mut out = vec![0u8; w * h];
        for oy in 0..h {
            let cy = (oy / 2).min(ch - 1);
            let (far, bias) = if oy % 2 == 0 {
                (cy.saturating_sub(1), 1)
            } else {
                ((cy + 1).min(ch - 1), 2)
            };
            for ox in 0..w {
                let x = ox.min(cw - 1);
                out[oy * w + ox] = ((3 * at(x, cy) + at(x, far) + bias) >> 2) as u8;
            }
        }
        return out;
    }
    if !matches!((dh, dv), (1, 1) | (2, 1) | (2, 2)) {
        let mut out = vec![0u8; w * h];
        for oy in 0..h {
            for ox in 0..w {
                out[oy * w + ox] = at((ox / dh).min(cw - 1), (oy / dv).min(ch - 1)) as u8;
            }
        }
        return out;
    }
    {
        // vertical: (3·nearer + further) row sums (×4 scale) for 4:2:0
        let mut out = vec![0u8; w * h];
        for oy in 0..h {
            let row: Vec<i32> = if dv == 2 {
                let cy = oy / 2;
                let far = if oy % 2 == 0 {
                    cy.saturating_sub(1)
                } else {
                    (cy + 1).min(ch - 1)
                };
                (0..cw).map(|x| 3 * at(x, cy) + at(x, far)).collect()
            } else {
                (0..cw).map(|x| at(x, oy.min(ch - 1))).collect()
            };
            for ox in 0..w {
                let v = if dh == 1 {
                    row[ox.min(cw - 1)] * if dv == 2 { 4 } else { 16 }
                } else {
                    let cx = ox / 2;
                    let c = row[cx];
                    if dv == 2 {
                        // ×16 scale
                        if cw == 1 {
                            c * 4
                        } else if ox % 2 == 0 {
                            if cx == 0 {
                                c * 4 + 8
                            } else {
                                3 * c + row[cx - 1] + 8
                            }
                        } else if cx + 1 >= cw {
                            c * 4 + 7
                        } else {
                            3 * c + row[cx + 1] + 7
                        }
                    } else if cw == 1 {
                        c * 16
                    } else if ox % 2 == 0 {
                        if cx == 0 {
                            c * 16
                        } else {
                            (3 * c + row[cx - 1] + 1) * 4
                        }
                    } else if cx + 1 >= cw {
                        c * 16
                    } else {
                        (3 * c + row[cx + 1] + 2) * 4
                    }
                };
                out[oy * w + ox] = if dh == 1 && dv == 1 {
                    (v >> 4) as u8
                } else {
                    (v >> 4).clamp(0, 255) as u8
                };
            }
        }
        out
    }
}

/// Planar YCbCr (or RGB when `ycc` is false) to packed RGBA through
/// libjpeg's upsampling and fixed-point conversion tables.
fn ycc_planes_to_rgba(
    w: usize,
    h: usize,
    fac: (usize, usize),
    yp: &JpegPlane,
    cbp: &JpegPlane,
    crp: &JpegPlane,
    ycc: bool,
) -> Vec<u8> {
    let cb = jpeg_upsample(cbp, w, h, fac);
    let cr = jpeg_upsample(crp, w, h, fac);
    let fix = |x: f64| (x * 65536.0 + 0.5) as i32;
    let half = 1 << 15;
    let mut out = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let yy = yp.data[y * yp.stride + x] as i32;
            if !ycc {
                let o = &mut out[(y * w + x) * 4..];
                o[0] = yy as u8;
                o[1] = cb[y * w + x];
                o[2] = cr[y * w + x];
                continue;
            }
            let b = cb[y * w + x] as i32 - 128;
            let r = cr[y * w + x] as i32 - 128;
            let rr = yy + ((fix(1.40200) * r + half) >> 16);
            let gg = yy + ((-fix(0.34414) * b - fix(0.71414) * r + half) >> 16);
            let bb = yy + ((fix(1.77200) * b + half) >> 16);
            let o = &mut out[(y * w + x) * 4..];
            o[0] = rr.clamp(0, 255) as u8;
            o[1] = gg.clamp(0, 255) as u8;
            o[2] = bb.clamp(0, 255) as u8;
        }
    }
    out
}

/// The pixels of a baseline/progressive 8-bit gray or three-component
/// JPEG through goro's ISLOW coefficient decoder (bit-exact with
/// libjpeg), and whether it is gray; None for anything else.
fn jpeg_islow_rgba(data: &[u8]) -> Option<(Vec<u8>, bool)> {
    let d = crate::jpeg_coef::decode(data)?;
    let (w, h) = (d.width, d.height);
    let c = &d.components;
    let plane = |k: usize| JpegPlane {
        data: &c[k].data,
        stride: c[k].stride,
        w: c[k].width,
        h: c[k].height,
    };
    match c.len() {
        1 => {
            let p = plane(0);
            let mut out = vec![255u8; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    let v = p.data[y * p.stride + x];
                    let o = &mut out[(y * w + x) * 4..];
                    o[0] = v;
                    o[1] = v;
                    o[2] = v;
                }
            }
            Some((out, true))
        }
        3 => {
            if c[1].h != c[2].h || c[1].v != c[2].v {
                return None;
            }
            if c[0].h % c[1].h != 0 || c[0].v % c[1].v != 0 {
                return None;
            }
            let fac = (c[0].h / c[1].h, c[0].v / c[1].v);
            let rgb = d.adobe_transform == Some(0)
                || (d.adobe_transform.is_none()
                    && !d.jfif
                    && c[0].id == b'R'
                    && c[1].id == b'G'
                    && c[2].id == b'B');
            Some((
                ycc_planes_to_rgba(w, h, fac, &plane(0), &plane(1), &plane(2), !rgb),
                false,
            ))
        }
        _ => None,
    }
}

/// The CMYK samples (packed, 4 per pixel) of a four-component JPEG through
/// the ISLOW decoder: YCCK goes through the YCbCr tables and is inverted,
/// and Adobe-marked CMYK is stored inverted.
fn jpeg_islow_cmyk(data: &[u8]) -> Option<Vec<u8>> {
    let d = crate::jpeg_coef::decode(data)?;
    let c = &d.components;
    if c.len() != 4 {
        return None;
    }
    let (w, h) = (d.width, d.height);
    let hmax = c.iter().map(|c| c.h).max()?;
    let vmax = c.iter().map(|c| c.v).max()?;
    let mut planes = Vec::new();
    for k in c {
        if hmax % k.h != 0 || vmax % k.v != 0 {
            return None;
        }
        let p = JpegPlane {
            data: &k.data,
            stride: k.stride,
            w: k.width,
            h: k.height,
        };
        planes.push(jpeg_upsample(&p, w, h, (hmax / k.h, vmax / k.v)));
    }
    let ycck = d.adobe_transform == Some(2);
    let fix = |x: f64| (x * 65536.0 + 0.5) as i32;
    let half = 1 << 15;
    let mut out = vec![0u8; w * h * 4];
    for i in 0..w * h {
        let mut s = [planes[0][i], planes[1][i], planes[2][i], planes[3][i]];
        if ycck {
            let yy = s[0] as i32;
            let b = s[1] as i32 - 128;
            let r = s[2] as i32 - 128;
            let rr = (yy + ((fix(1.40200) * r + half) >> 16)).clamp(0, 255);
            let gg = (yy + ((-fix(0.34414) * b - fix(0.71414) * r + half) >> 16)).clamp(0, 255);
            let bb = (yy + ((fix(1.77200) * b + half) >> 16)).clamp(0, 255);
            s[0] = 255 - rr as u8;
            s[1] = 255 - gg as u8;
            s[2] = 255 - bb as u8;
        }
        if d.adobe_transform.is_some() {
            for v in &mut s {
                *v = 255 - *v;
            }
        }
        out[i * 4..i * 4 + 4].copy_from_slice(&s);
    }
    Some(out)
}

/// The Photoshop (APP13 "Photoshop 3.0") resource data of a JPEG.
fn jpeg_photoshop(data: &[u8]) -> Option<Vec<u8>> {
    const SIG: &[u8] = b"Photoshop 3.0\0";
    let mut out = Vec::new();
    let mut i = 2;
    while i + 4 <= data.len() && data[i] == 0xff {
        let m = data[i + 1];
        if m == 0xda || m == 0xd9 {
            break;
        }
        if (0xd0..=0xd7).contains(&m) || m == 0x01 || m == 0xff {
            i += if m == 0xff { 1 } else { 2 };
            continue;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let seg = data.get(i + 4..i + 2 + len)?;
        if m == 0xed && seg.starts_with(SIG) {
            out.extend_from_slice(&seg[SIG.len()..]);
        }
        i += 2 + len;
    }
    (!out.is_empty()).then_some(out)
}

/// One resource of Photoshop 8BIM resource data.
pub fn photoshop_resource(p: &[u8], want: u16) -> Option<Vec<u8>> {
    let mut i = 0;
    while i + 12 <= p.len() {
        if &p[i..i + 4] != b"8BIM" {
            return None;
        }
        let id = u16::from_be_bytes([p[i + 4], p[i + 5]]);
        let nlen = p[i + 6] as usize;
        let mut j = i + 7 + nlen;
        if !(1 + nlen).is_multiple_of(2) {
            j += 1;
        }
        let size = u32::from_be_bytes(p.get(j..j + 4)?.try_into().ok()?) as usize;
        j += 4;
        if id == want {
            return p.get(j..j + size).map(|d| d.to_vec());
        }
        i = j + size + (size & 1);
    }
    None
}
