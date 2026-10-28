//! Image type sniffing and dimensions: `getimagesize()`,
//! `getimagesizefromstring()`, `exif_imagetype()`, `image_type_to_*()`.
//!
//! The header layouts come from each format's specification (GIF89a,
//! JFIF/ITU T.81, PNG, SWF, PSD, BMP, TIFF 6.0, JPEG 2000 part 1, IFF
//! ILBM, WBMP, XBM, ICO, WebP container, ISO BMFF/HEIF/AVIF, SVG). How
//! much of a header PHP must be able to read, which fields it reports and
//! its diagnostics were observed on PHP 8.5 (tests
//! ext/standard/tests/image/*.phpt).

use crate::Diag;

pub const IMAGETYPE_UNKNOWN: i64 = 0;
pub const IMAGETYPE_GIF: i64 = 1;
pub const IMAGETYPE_JPEG: i64 = 2;
pub const IMAGETYPE_PNG: i64 = 3;
pub const IMAGETYPE_SWF: i64 = 4;
pub const IMAGETYPE_PSD: i64 = 5;
pub const IMAGETYPE_BMP: i64 = 6;
pub const IMAGETYPE_TIFF_II: i64 = 7;
pub const IMAGETYPE_TIFF_MM: i64 = 8;
pub const IMAGETYPE_JPC: i64 = 9;
pub const IMAGETYPE_JP2: i64 = 10;
pub const IMAGETYPE_JPX: i64 = 11;
pub const IMAGETYPE_JB2: i64 = 12;
pub const IMAGETYPE_SWC: i64 = 13;
pub const IMAGETYPE_IFF: i64 = 14;
pub const IMAGETYPE_WBMP: i64 = 15;
pub const IMAGETYPE_XBM: i64 = 16;
pub const IMAGETYPE_ICO: i64 = 17;
pub const IMAGETYPE_WEBP: i64 = 18;
pub const IMAGETYPE_AVIF: i64 = 19;
pub const IMAGETYPE_HEIF: i64 = 20;
pub const IMAGETYPE_SVG: i64 = 21;
pub const IMAGETYPE_COUNT: i64 = 22;

/// The `IMAGETYPE_*` constants (PHP manual: getimagesize, image type
/// constants; values observed on PHP 8.5).
pub const CONSTANTS: &[(&str, i64)] = &[
    ("IMAGETYPE_GIF", IMAGETYPE_GIF),
    ("IMAGETYPE_JPEG", IMAGETYPE_JPEG),
    ("IMAGETYPE_PNG", IMAGETYPE_PNG),
    ("IMAGETYPE_SWF", IMAGETYPE_SWF),
    ("IMAGETYPE_PSD", IMAGETYPE_PSD),
    ("IMAGETYPE_BMP", IMAGETYPE_BMP),
    ("IMAGETYPE_TIFF_II", IMAGETYPE_TIFF_II),
    ("IMAGETYPE_TIFF_MM", IMAGETYPE_TIFF_MM),
    ("IMAGETYPE_JPC", IMAGETYPE_JPC),
    ("IMAGETYPE_JP2", IMAGETYPE_JP2),
    ("IMAGETYPE_JPX", IMAGETYPE_JPX),
    ("IMAGETYPE_JB2", IMAGETYPE_JB2),
    ("IMAGETYPE_SWC", IMAGETYPE_SWC),
    ("IMAGETYPE_IFF", IMAGETYPE_IFF),
    ("IMAGETYPE_WBMP", IMAGETYPE_WBMP),
    ("IMAGETYPE_JPEG2000", IMAGETYPE_JPC),
    ("IMAGETYPE_XBM", IMAGETYPE_XBM),
    ("IMAGETYPE_ICO", IMAGETYPE_ICO),
    ("IMAGETYPE_WEBP", IMAGETYPE_WEBP),
    ("IMAGETYPE_AVIF", IMAGETYPE_AVIF),
    ("IMAGETYPE_HEIF", IMAGETYPE_HEIF),
    ("IMAGETYPE_UNKNOWN", IMAGETYPE_UNKNOWN),
    ("IMAGETYPE_COUNT", IMAGETYPE_COUNT),
    ("IMAGETYPE_SVG", IMAGETYPE_SVG),
];

/// `image_type_to_mime_type()` (observed on PHP 8.5).
pub fn mime_type(t: i64) -> &'static str {
    match t {
        IMAGETYPE_GIF => "image/gif",
        IMAGETYPE_JPEG => "image/jpeg",
        IMAGETYPE_PNG => "image/png",
        IMAGETYPE_SWF | IMAGETYPE_SWC => "application/x-shockwave-flash",
        IMAGETYPE_PSD => "image/psd",
        IMAGETYPE_BMP => "image/bmp",
        IMAGETYPE_TIFF_II | IMAGETYPE_TIFF_MM => "image/tiff",
        IMAGETYPE_JP2 => "image/jp2",
        IMAGETYPE_IFF => "image/iff",
        IMAGETYPE_WBMP => "image/vnd.wap.wbmp",
        IMAGETYPE_XBM => "image/xbm",
        IMAGETYPE_ICO => "image/vnd.microsoft.icon",
        IMAGETYPE_WEBP => "image/webp",
        IMAGETYPE_AVIF => "image/avif",
        IMAGETYPE_HEIF => "image/heif",
        IMAGETYPE_SVG => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// `image_type_to_extension()` without the dot (observed on PHP 8.5:
/// WBMP gives "bmp", JPEG "jpeg").
pub fn extension(t: i64) -> Option<&'static str> {
    Some(match t {
        IMAGETYPE_GIF => "gif",
        IMAGETYPE_JPEG => "jpeg",
        IMAGETYPE_PNG => "png",
        IMAGETYPE_SWF | IMAGETYPE_SWC => "swf",
        IMAGETYPE_PSD => "psd",
        IMAGETYPE_BMP | IMAGETYPE_WBMP => "bmp",
        IMAGETYPE_TIFF_II | IMAGETYPE_TIFF_MM => "tiff",
        IMAGETYPE_JPC => "jpc",
        IMAGETYPE_JP2 => "jp2",
        IMAGETYPE_JPX => "jpx",
        IMAGETYPE_JB2 => "jb2",
        IMAGETYPE_IFF => "iff",
        IMAGETYPE_XBM => "xbm",
        IMAGETYPE_ICO => "ico",
        IMAGETYPE_WEBP => "webp",
        IMAGETYPE_AVIF => "avif",
        IMAGETYPE_HEIF => "heif",
        IMAGETYPE_SVG => "svg",
        _ => return None,
    })
}

/// What getimagesize() reports for an image.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
    pub itype: i64,
    /// Bits per sample ("bits", left out when 0).
    pub bits: u32,
    /// Channels ("channels", left out when 0).
    pub channels: u32,
    /// The `width="W" height="H"` string (index 3); None when the units
    /// are not pixels (SVG).
    pub html: Option<String>,
    pub width_unit: String,
    pub height_unit: String,
}

impl ImageSize {
    fn px(width: u32, height: u32, itype: i64, bits: u32, channels: u32) -> ImageSize {
        ImageSize {
            width,
            height,
            itype,
            bits,
            channels,
            html: Some(format!("width=\"{width}\" height=\"{height}\"")),
            width_unit: "px".into(),
            height_unit: "px".into(),
        }
    }
}

/// A byte buffer read like a PHP stream: short reads at the end.
struct Rd<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn new(d: &'a [u8], pos: usize) -> Rd<'a> {
        Rd { d, pos }
    }
    /// Exactly `n` bytes, or None (the position still moves to the end).
    fn read(&mut self, n: usize) -> Option<&'a [u8]> {
        let start = self.pos.min(self.d.len());
        let end = start.saturating_add(n);
        if end > self.d.len() {
            self.pos = self.d.len();
            return None;
        }
        self.pos = end;
        Some(&self.d[start..end])
    }
    fn getc(&mut self) -> Option<u8> {
        let b = self.d.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }
    /// A big-endian u16 as PHP reads it: a short read leaves the missing
    /// bytes zero.
    fn be16(&mut self) -> u16 {
        let mut b = [0u8; 2];
        for x in b.iter_mut() {
            match self.getc() {
                Some(c) => *x = c,
                None => break,
            }
        }
        u16::from_be_bytes(b)
    }
    fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n);
    }
}

fn be16(b: &[u8], o: usize) -> u32 {
    u16::from_be_bytes([b[o], b[o + 1]]) as u32
}
fn le16(b: &[u8], o: usize) -> u32 {
    u16::from_le_bytes([b[o], b[o + 1]]) as u32
}
fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// The image type of `d` as PHP sniffs it (getimagesize and
/// exif_imagetype share it). Signatures from the format specifications;
/// the read sizes behind the "Error reading from" notice observed on
/// PHP 8.5: 3 bytes, then 4, then 12.
pub fn image_type(d: &[u8], diags: &mut Vec<Diag>) -> i64 {
    if d.len() < 3 {
        diags.push(Diag::ReadError);
        return IMAGETYPE_UNKNOWN;
    }
    match &d[..3] {
        b"GIF" => return IMAGETYPE_GIF,
        [0xFF, 0xD8, 0xFF] => return IMAGETYPE_JPEG,
        [0x89, b'P', b'N'] => {
            if d.len() < 8 {
                diags.push(Diag::ReadError);
                return IMAGETYPE_UNKNOWN;
            }
            if &d[3..8] == b"G\r\n\x1a\n" {
                return IMAGETYPE_PNG;
            }
            diags.push(Diag::Warning(
                "PNG file corrupted by ASCII conversion".into(),
            ));
            return IMAGETYPE_UNKNOWN;
        }
        b"FWS" => return IMAGETYPE_SWF,
        b"CWS" => return IMAGETYPE_SWC,
        b"8BP" => return IMAGETYPE_PSD,
        [0xFF, 0x4F, 0xFF] => return IMAGETYPE_JPC,
        _ => {}
    }
    if &d[..2] == b"BM" {
        return IMAGETYPE_BMP;
    }
    if d.len() < 4 {
        diags.push(Diag::ReadError);
        return IMAGETYPE_UNKNOWN;
    }
    match &d[..4] {
        b"II*\0" => return IMAGETYPE_TIFF_II,
        b"MM\0*" => return IMAGETYPE_TIFF_MM,
        b"FORM" => return IMAGETYPE_IFF,
        [0, 0, 1, 0] => return IMAGETYPE_ICO,
        _ => {}
    }
    if d.len() < 12 {
        if wbmp_size(d).is_some() {
            return IMAGETYPE_WBMP;
        }
        diags.push(Diag::ReadError);
        return IMAGETYPE_UNKNOWN;
    }
    if &d[..12] == b"\0\0\0\x0cjP  \r\n\x87\n" {
        return IMAGETYPE_JP2;
    }
    if &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        return IMAGETYPE_WEBP;
    }
    if let Some(t) = heif_brand(d) {
        return t;
    }
    if wbmp_size(d).is_some() {
        return IMAGETYPE_WBMP;
    }
    if xbm_size(d).is_some() {
        return IMAGETYPE_XBM;
    }
    if svg_size(d).is_some() {
        return IMAGETYPE_SVG;
    }
    IMAGETYPE_UNKNOWN
}

/// getimagesize(): the size of `d` and, when `want_info`, the JPEG APPn
/// segments (`("APPn", data)`, first of each kept).
pub fn image_size(
    d: &[u8],
    want_info: bool,
    diags: &mut Vec<Diag>,
) -> (Option<ImageSize>, Vec<(String, Vec<u8>)>) {
    let mut info = Vec::new();
    let t = image_type(d, diags);
    let r = match t {
        IMAGETYPE_GIF => gif(d),
        IMAGETYPE_JPEG => jpeg(d, want_info.then_some(&mut info), diags),
        IMAGETYPE_PNG => png(d),
        IMAGETYPE_SWF => swf(d),
        IMAGETYPE_SWC => swc(d),
        IMAGETYPE_PSD => psd(d),
        IMAGETYPE_BMP => bmp(d),
        IMAGETYPE_TIFF_II | IMAGETYPE_TIFF_MM => tiff(d, t),
        IMAGETYPE_JPC => jpc(d, 0, IMAGETYPE_JPC, diags),
        IMAGETYPE_JP2 => jp2(d, diags),
        IMAGETYPE_IFF => iff(d),
        IMAGETYPE_WBMP => wbmp_size(d).map(|(w, h)| ImageSize::px(w, h, t, 0, 0)),
        IMAGETYPE_XBM => xbm_size(d).map(|(w, h)| {
            let mut s = ImageSize::px(w as u32, h as u32, t, 0, 0);
            s.html = Some(format!("width=\"{w}\" height=\"{h}\""));
            s
        }),
        IMAGETYPE_ICO => ico(d),
        IMAGETYPE_WEBP => webp(d),
        IMAGETYPE_AVIF | IMAGETYPE_HEIF => heif_size(d, t),
        IMAGETYPE_SVG => svg_size(d),
        _ => None,
    };
    (r, info)
}

fn gif(d: &[u8]) -> Option<ImageSize> {
    let b = Rd::new(d, 3).read(8)?;
    let w = le16(b, 3);
    let h = le16(b, 5);
    let flags = b[7];
    // GIF89a: the global color table size gives the bits.
    let bits = if flags & 0x80 != 0 {
        (flags & 7) as u32 + 1
    } else {
        0
    };
    Some(ImageSize::px(w, h, IMAGETYPE_GIF, bits, 3))
}

fn png(d: &[u8]) -> Option<ImageSize> {
    let b = Rd::new(d, 8).read(17)?;
    Some(ImageSize::px(
        be32(b, 8),
        be32(b, 12),
        IMAGETYPE_PNG,
        b[16] as u32,
        0,
    ))
}

/// The RECT at the start of an SWF header body (SWF spec: RECT record).
fn swf_rect(b: &[u8]) -> (u32, u32) {
    let nbits = (b[0] >> 3) as u32;
    let mut pos = 5u32;
    let mut take = |n: u32| -> i64 {
        let mut v: u64 = 0;
        for _ in 0..n {
            let byte = b.get((pos / 8) as usize).copied().unwrap_or(0);
            let bit = (byte >> (7 - pos % 8)) & 1;
            v = (v << 1) | bit as u64;
            pos += 1;
        }
        v as i64
    };
    let xmin = take(nbits);
    let xmax = take(nbits);
    let ymin = take(nbits);
    let ymax = take(nbits);
    (
        ((xmax - xmin) / 20) as u32,
        ((ymax - ymin) / 20) as u32,
    )
}

fn swf(d: &[u8]) -> Option<ImageSize> {
    let b = Rd::new(d, 8).read(32)?;
    let (w, h) = swf_rect(b);
    Some(ImageSize::px(w, h, IMAGETYPE_SWF, 0, 0))
}

fn swc(d: &[u8]) -> Option<ImageSize> {
    // The zlib stream after the 8-byte header; PHP needs 64 bytes of it
    // (observed on PHP 8.5).
    let z = Rd::new(d, 8).read(64)?;
    let mut out = Vec::new();
    let mut inf = goro_ext_zlib::inflate::Inflater::new(goro_ext_zlib::inflate::InflateWrap::Zlib, 15);
    let rest = &d[8..];
    let (_, st) = inf.inflate(rest, &mut out, 4096, goro_ext_zlib::inflate::InflateFlush::None);
    let _ = z;
    if let goro_ext_zlib::inflate::InflateStatus::DataError(_) = st {
        return None;
    }
    if out.is_empty() {
        return None;
    }
    let mut b = out;
    b.resize(b.len().max(32), 0);
    let (w, h) = swf_rect(&b);
    Some(ImageSize::px(w, h, IMAGETYPE_SWC, 0, 0))
}

fn psd(d: &[u8]) -> Option<ImageSize> {
    let b = Rd::new(d, 0).read(22)?;
    Some(ImageSize::px(
        be32(b, 18),
        be32(b, 14),
        IMAGETYPE_PSD,
        0,
        0,
    ))
}

fn bmp(d: &[u8]) -> Option<ImageSize> {
    let b = Rd::new(d, 14).read(16)?;
    let size = le32(b, 0);
    if size == 12 {
        // OS/2 BITMAPCOREHEADER: 16-bit dimensions.
        Some(ImageSize::px(le16(b, 4), le16(b, 6), IMAGETYPE_BMP, 0, 0))
    } else if size > 12 {
        let w = le32(b, 4) as i32;
        let h = le32(b, 8) as i32;
        Some(ImageSize::px(
            w as u32,
            h.unsigned_abs(),
            IMAGETYPE_BMP,
            le16(b, 14),
            0,
        ))
    } else {
        None
    }
}

fn tiff(d: &[u8], t: i64) -> Option<ImageSize> {
    let mm = t == IMAGETYPE_TIFF_MM;
    let r16 = |b: &[u8], o: usize| if mm { be16(b, o) } else { le16(b, o) };
    let r32 = |b: &[u8], o: usize| if mm { be32(b, o) } else { le32(b, o) };
    let hdr = Rd::new(d, 0).read(8)?;
    let off = r32(hdr, 4) as usize;
    let mut rd = Rd::new(d, off);
    let n = r16(rd.read(2)?, 0) as usize;
    let dir = rd.read(n * 12 + 4)?;
    let (mut w, mut h) = (0u32, 0u32);
    for i in 0..n {
        let e = &dir[i * 12..i * 12 + 12];
        let tag = r16(e, 0);
        let ty = r16(e, 2);
        let v = match ty {
            1 | 6 => e[8] as u32,
            3 | 8 => r16(e, 8),
            4 | 9 => r32(e, 8),
            _ => 0,
        };
        match tag {
            0x100 => w = v,
            0x101 => h = v,
            _ => {}
        }
    }
    if w == 0 || h == 0 {
        return None;
    }
    Some(ImageSize::px(w, h, t, 0, 0))
}

/// A JPEG 2000 codestream at `start` (ISO/IEC 15444-1 annex A: SOC then
/// the SIZ marker segment).
fn jpc(d: &[u8], start: usize, t: i64, diags: &mut Vec<Diag>) -> Option<ImageSize> {
    let mut r = Rd::new(d, start + 2);
    match r.read(2) {
        Some([0xFF, 0x51]) => {}
        _ => {
            diags.push(Diag::Warning(
                "JPEG2000 codestream corrupt(Expected SIZ marker not found after SOC)".into(),
            ));
            return None;
        }
    }
    let b = r.read(38)?;
    let w = be32(b, 4);
    let h = be32(b, 8);
    let comps = be16(b, 36);
    let mut bits = 0u32;
    for _ in 0..comps {
        let Some(ssiz) = r.getc() else { break };
        bits = bits.max((ssiz & 0x7f) as u32 + 1);
        if r.getc().is_none() || r.getc().is_none() {
            break;
        }
    }
    Some(ImageSize::px(w, h, t, bits, comps))
}

/// JP2: the first `jp2c` box at the top level holds the codestream.
fn jp2(d: &[u8], diags: &mut Vec<Diag>) -> Option<ImageSize> {
    let mut pos = 0usize;
    loop {
        let Some(h) = Rd::new(d, pos).read(8) else {
            break;
        };
        let len = be32(h, 0) as usize;
        if &h[4..8] == b"jp2c" {
            return jpc(d, pos + 8, IMAGETYPE_JP2, diags);
        }
        if len < 8 {
            break;
        }
        pos += len;
    }
    diags.push(Diag::Warning(
        "JP2 file has no codestreams at root level".into(),
    ));
    None
}

/// IFF ILBM/PBM: the BMHD chunk (EA IFF 85, ILBM).
fn iff(d: &[u8]) -> Option<ImageSize> {
    let mut r = Rd::new(d, 0);
    let h = r.read(12)?;
    if &h[8..12] != b"ILBM" && &h[8..12] != b"PBM " {
        return None;
    }
    loop {
        let c = r.read(8)?;
        let size = be32(c, 4) as usize;
        if &c[..4] == b"BMHD" {
            if size < 9 {
                return None;
            }
            let b = r.read(9)?;
            let w = be16(b, 0);
            let h = be16(b, 2);
            if w == 0 || h == 0 {
                return None;
            }
            return Some(ImageSize::px(w, h, IMAGETYPE_IFF, b[8] as u32, 0));
        }
        r.skip(size + (size & 1));
    }
}

/// A WBMP multi-byte integer (WAP WBMP: 7 bits per byte).
fn mbi(r: &mut Rd) -> Option<u32> {
    let mut v: u32 = 0;
    loop {
        let c = r.getc()?;
        v = v.checked_mul(128)?.checked_add((c & 0x7f) as u32)?;
        if c & 0x80 == 0 {
            return Some(v);
        }
        if v > 0x00FF_FFFF {
            return None;
        }
    }
}

/// WBMP type 0: width and height of 1 to 2048 pixels (observed on
/// PHP 8.5, test bug70052.phpt).
fn wbmp_size(d: &[u8]) -> Option<(u32, u32)> {
    let mut r = Rd::new(d, 0);
    if mbi(&mut r)? != 0 {
        return None;
    }
    if r.getc()? != 0 {
        return None;
    }
    let w = mbi(&mut r)?;
    let h = mbi(&mut r)?;
    if w == 0 || h == 0 || w > 2048 || h > 2048 {
        return None;
    }
    Some((w, h))
}

/// `%d` of sscanf: optional sign and digits, after optional whitespace.
fn scan_int(s: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let st = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((s[i] - b'0') as i64);
        i += 1;
    }
    if i == st {
        return None;
    }
    Some(if neg { -v } else { v })
}

/// XBM: `#define <name>_width N` and `_height` lines (X11 bitmap format).
fn xbm_size(d: &[u8]) -> Option<(i32, i32)> {
    let (mut w, mut h) = (0i32, 0i32);
    for line in d.split(|&c| c == b'\n') {
        let Some(rest) = line.strip_prefix(b"#define") else {
            continue;
        };
        let mut i = 0;
        while i < rest.len() && rest[i].is_ascii_whitespace() {
            i += 1;
        }
        let st = i;
        while i < rest.len() && !rest[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == st {
            continue;
        }
        let name = &rest[st..i];
        let Some(v) = scan_int(&rest[i..]) else {
            continue;
        };
        let base = match name.iter().rposition(|&c| c == b'_') {
            Some(p) => &name[p + 1..],
            None => name,
        };
        if base == b"width" {
            w = v as i32;
        } else if base == b"height" {
            h = v as i32;
        }
        if w != 0 && h != 0 {
            return Some((w, h));
        }
    }
    None
}

fn ico(d: &[u8]) -> Option<ImageSize> {
    let mut r = Rd::new(d, 0);
    let h = r.read(6)?;
    let n = le16(h, 4);
    if n == 0 {
        return None;
    }
    let (mut w, mut hh, mut bits) = (0u32, 0u32, 0u32);
    for _ in 0..n {
        let Some(e) = r.read(16) else { break };
        let b = le16(e, 6);
        if b >= bits {
            w = e[0] as u32;
            hh = e[1] as u32;
            bits = b;
        }
    }
    if w == 0 {
        w = 256;
    }
    if hh == 0 {
        hh = 256;
    }
    Some(ImageSize::px(w, hh, IMAGETYPE_ICO, bits, 0))
}

fn webp(d: &[u8]) -> Option<ImageSize> {
    let b = Rd::new(d, 12).read(18)?;
    let (w, h) = match &b[..4] {
        b"VP8 " => (le16(b, 14) & 0x3fff, le16(b, 16) & 0x3fff),
        b"VP8L" => {
            let v = le32(b, 9);
            ((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1)
        }
        b"VP8X" => {
            let w = u32::from_le_bytes([b[12], b[13], b[14], 0]) + 1;
            let h = u32::from_le_bytes([b[15], b[16], b[17], 0]) + 1;
            (w, h)
        }
        _ => return None,
    };
    Some(ImageSize::px(w, h, IMAGETYPE_WEBP, 8, 0))
}

/// AVIF or HEIF from the `ftyp` box brands (observed on PHP 8.5: AVIF
/// when "avif"/"avis" is the major or a compatible brand, HEIF when the
/// major brand is "heic", "heix" or "mif1").
pub fn heif_brand(d: &[u8]) -> Option<i64> {
    if d.len() < 12 || &d[4..8] != b"ftyp" {
        return None;
    }
    let size = (be32(d, 0) as usize).min(d.len());
    let major = &d[8..12];
    let mut brands = vec![major];
    let mut o = 16;
    while o + 4 <= size {
        brands.push(&d[o..o + 4]);
        o += 4;
    }
    if brands.iter().any(|b| *b == b"avif" || *b == b"avis") {
        return Some(IMAGETYPE_AVIF);
    }
    if major == b"heic" || major == b"heix" || major == b"mif1" {
        return Some(IMAGETYPE_HEIF);
    }
    None
}

/// ISO BMFF boxes of `d[start..end]`: `(type, body start, body end)`.
pub fn boxes(d: &[u8], start: usize, end: usize) -> Vec<([u8; 4], usize, usize)> {
    let end = end.min(d.len());
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 8 <= end {
        let mut size = be32(d, pos) as u64;
        let typ = [d[pos + 4], d[pos + 5], d[pos + 6], d[pos + 7]];
        let mut hdr = 8u64;
        if size == 1 {
            if pos + 16 > end {
                break;
            }
            size = u64::from_be_bytes(d[pos + 8..pos + 16].try_into().unwrap());
            hdr = 16;
        } else if size == 0 {
            size = (end - pos) as u64;
        }
        if size < hdr || pos as u64 + size > end as u64 {
            break;
        }
        out.push((typ, pos + hdr as usize, pos + size as usize));
        pos += size as usize;
    }
    out
}

/// Width, height, bits and channels of the primary item of a HEIF/AVIF
/// file: its `ispe` and `pixi` properties (ISO/IEC 23008-12), plus one
/// channel when an alpha auxiliary property exists (observed on PHP 8.5).
fn heif_size(d: &[u8], t: i64) -> Option<ImageSize> {
    let top = boxes(d, 0, d.len());
    let &(_, ms, me) = top.iter().find(|b| &b.0 == b"meta")?;
    let meta = boxes(d, ms + 4, me);
    let mut primary: Option<u32> = None;
    let mut props: Vec<([u8; 4], usize, usize)> = Vec::new();
    let mut assoc: Vec<(u32, Vec<u16>)> = Vec::new();
    for &(ty, s, e) in &meta {
        match &ty {
            b"pitm" => {
                if e < s + 4 {
                    continue;
                }
                primary = Some(if d[s] == 0 {
                    if e < s + 6 {
                        continue;
                    }
                    be16(d, s + 4)
                } else {
                    if e < s + 8 {
                        continue;
                    }
                    be32(d, s + 4)
                });
            }
            b"iprp" => {
                for &(pt, ps, pe) in &boxes(d, s, e) {
                    if &pt == b"ipco" {
                        props = boxes(d, ps, pe);
                    } else if &pt == b"ipma" && pe >= ps + 8 {
                        let ver = d[ps];
                        let flags = be32(d, ps) & 0xFF_FFFF;
                        let n = be32(d, ps + 4);
                        let mut p = ps + 8;
                        for _ in 0..n {
                            let id = if ver < 1 {
                                if p + 2 > pe {
                                    break;
                                }
                                p += 2;
                                be16(d, p - 2)
                            } else {
                                if p + 4 > pe {
                                    break;
                                }
                                p += 4;
                                be32(d, p - 4)
                            };
                            if p >= pe {
                                break;
                            }
                            let cnt = d[p] as usize;
                            p += 1;
                            let mut list = Vec::new();
                            for _ in 0..cnt {
                                if flags & 1 != 0 {
                                    if p + 2 > pe {
                                        break;
                                    }
                                    list.push((be16(d, p) & 0x7fff) as u16);
                                    p += 2;
                                } else {
                                    if p >= pe {
                                        break;
                                    }
                                    list.push((d[p] & 0x7f) as u16);
                                    p += 1;
                                }
                            }
                            assoc.push((id, list));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let primary = primary?;
    let list = &assoc.iter().find(|a| a.0 == primary)?.1;
    let (mut size, mut pix) = (None, None);
    for &i in list {
        let Some(&(pt, ps, pe)) = props.get((i as usize).wrapping_sub(1)) else {
            continue;
        };
        if &pt == b"ispe" && pe >= ps + 12 && size.is_none() {
            size = Some((be32(d, ps + 4), be32(d, ps + 8)));
        } else if &pt == b"pixi" && pe >= ps + 5 && pix.is_none() {
            let n = d[ps + 4] as usize;
            if n == 0 || ps + 5 + n > pe {
                return None;
            }
            let bits = &d[ps + 5..ps + 5 + n];
            if bits.iter().any(|&b| b != bits[0]) {
                return None;
            }
            pix = Some((bits[0] as u32, n as u32));
        }
    }
    let (w, h) = size?;
    let (bits, mut ch) = pix?;
    let alpha = props.iter().any(|&(pt, ps, pe)| {
        &pt == b"auxC" && pe > ps + 4 && {
            let urn = &d[ps + 4..pe];
            let urn = urn.split(|&c| c == 0).next().unwrap_or(b"");
            urn == b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha"
                || urn == b"urn:mpeg:hevc:2015:auxid:1"
        }
    });
    if alpha {
        ch += 1;
    }
    Some(ImageSize::px(w, h, t, bits, ch))
}

/// SVG: the root element's `width` and `height` (an integer and an
/// optional unit of letters, "px" when absent; observed on PHP 8.5).
fn svg_size(d: &[u8]) -> Option<ImageSize> {
    if d.first() != Some(&b'<') {
        return None;
    }
    let (name, attrs) = first_element(d)?;
    let local = match name.iter().position(|&c| c == b':') {
        Some(p) => &name[p + 1..],
        None => &name[..],
    };
    if !local.eq_ignore_ascii_case(b"svg") {
        return None;
    }
    let get = |n: &[u8]| attrs.iter().find(|a| a.0 == n).map(|a| a.1.clone());
    let (w, wu) = svg_len(&get(b"width")?)?;
    let (h, hu) = svg_len(&get(b"height")?)?;
    let html = (wu == "px" && hu == "px").then(|| format!("width=\"{w}\" height=\"{h}\""));
    Some(ImageSize {
        width: w,
        height: h,
        itype: IMAGETYPE_SVG,
        bits: 0,
        channels: 0,
        html,
        width_unit: wu,
        height_unit: hu,
    })
}

fn svg_len(v: &[u8]) -> Option<(u32, String)> {
    let digits = v.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let unit = &v[digits..];
    if !unit.iter().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut n: u64 = 0;
    for &c in &v[..digits] {
        n = n.wrapping_mul(10).wrapping_add((c - b'0') as u64);
    }
    let unit = if unit.is_empty() {
        "px".to_string()
    } else {
        String::from_utf8_lossy(unit).into_owned()
    };
    Some((n as u32, unit))
}

type Attrs = Vec<(Vec<u8>, Vec<u8>)>;

/// The first element's start tag of an XML document (XML 1.0: prolog of
/// declarations, comments, processing instructions and a DOCTYPE).
fn first_element(d: &[u8]) -> Option<(Vec<u8>, Attrs)> {
    let mut i = 0;
    let n = d.len();
    loop {
        while i < n && d[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n || d[i] != b'<' {
            return None;
        }
        if d[i..].starts_with(b"<?") {
            let e = find(d, i + 2, b"?>")?;
            i = e + 2;
        } else if d[i..].starts_with(b"<!--") {
            let e = find(d, i + 4, b"-->")?;
            i = e + 3;
        } else if d[i..].starts_with(b"<!") {
            // DOCTYPE, with an optional internal subset.
            let mut j = i + 2;
            let mut depth = 0i32;
            let mut quote = 0u8;
            loop {
                let c = *d.get(j)?;
                if quote != 0 {
                    if c == quote {
                        quote = 0;
                    }
                } else if c == b'"' || c == b'\'' {
                    quote = c;
                } else if c == b'[' {
                    depth += 1;
                } else if c == b']' {
                    depth -= 1;
                } else if c == b'>' && depth <= 0 {
                    break;
                }
                j += 1;
            }
            i = j + 1;
        } else {
            break;
        }
    }
    i += 1;
    let st = i;
    while i < n && !d[i].is_ascii_whitespace() && d[i] != b'>' && d[i] != b'/' {
        i += 1;
    }
    if i == st {
        return None;
    }
    let name = d[st..i].to_vec();
    let mut attrs: Attrs = Vec::new();
    loop {
        while i < n && d[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n {
            return None;
        }
        if d[i] == b'>' || d[i] == b'/' {
            break;
        }
        let st = i;
        while i < n && d[i] != b'=' && !d[i].is_ascii_whitespace() && d[i] != b'>' {
            i += 1;
        }
        let an = d[st..i].to_vec();
        while i < n && d[i].is_ascii_whitespace() {
            i += 1;
        }
        if d.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while i < n && d[i].is_ascii_whitespace() {
            i += 1;
        }
        let q = *d.get(i)?;
        if q != b'"' && q != b'\'' {
            return None;
        }
        let vs = i + 1;
        let ve = vs + d[vs..].iter().position(|&c| c == q)?;
        let v = decode_entities(&d[vs..ve])?;
        i = ve + 1;
        if attrs.iter().any(|a| a.0 == an) {
            return None;
        }
        attrs.push((an, v));
    }
    Some((name, attrs))
}

fn find(d: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    d.get(from..)?
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + from)
}

fn decode_entities(v: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'&' {
            let e = i + v[i..].iter().position(|&c| c == b';')?;
            let ent = &v[i + 1..e];
            match ent {
                b"lt" => out.push(b'<'),
                b"gt" => out.push(b'>'),
                b"amp" => out.push(b'&'),
                b"quot" => out.push(b'"'),
                b"apos" => out.push(b'\''),
                _ if ent.first() == Some(&b'#') => {
                    let num = &ent[1..];
                    let cp = if let Some(h) = num.strip_prefix(b"x") {
                        u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?
                    } else {
                        std::str::from_utf8(num).ok()?.parse::<u32>().ok()?
                    };
                    let ch = char::from_u32(cp)?;
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => return None,
            }
            i = e + 1;
        } else {
            out.push(v[i]);
            i += 1;
        }
    }
    Some(out)
}

/// The JPEG markers getimagesize() scans (ITU T.81 table B.1).
const M_SOI: u8 = 0xD8;
const M_EOI: u8 = 0xD9;
const M_SOS: u8 = 0xDA;
/// "Not a marker": data that does not start with 0xFF after SOI.
const M_PSEUDO: u8 = 0xFF;

/// The next marker: 0xFF fill bytes are skipped; bytes before the 0xFF
/// are counted for the "Corrupt JPEG data" warning (observed on PHP
/// 8.5, test bug13213.phpt).
fn next_marker(r: &mut Rd, first: bool, diags: &mut Vec<Diag>) -> u8 {
    let mut extraneous = 0usize;
    let mut c;
    loop {
        match r.getc() {
            None => return M_EOI,
            Some(0xFF) => break,
            Some(_) => {
                if first {
                    return M_PSEUDO;
                }
                extraneous += 1;
            }
        }
    }
    loop {
        c = match r.getc() {
            None => return M_EOI,
            Some(c) => c,
        };
        if c != 0xFF {
            break;
        }
    }
    if extraneous > 0 {
        diags.push(Diag::Warning(format!(
            "Corrupt JPEG data: {extraneous} extraneous bytes before marker"
        )));
    }
    c
}

fn jpeg(
    d: &[u8],
    mut info: Option<&mut Vec<(String, Vec<u8>)>>,
    diags: &mut Vec<Diag>,
) -> Option<ImageSize> {
    let mut r = Rd::new(d, 2);
    let mut result: Option<ImageSize> = None;
    let mut first = true;
    loop {
        let m = next_marker(&mut r, first, diags);
        first = false;
        match m {
            M_PSEUDO => return None,
            0xC0..=0xCF if m != 0xC4 && m != 0xC8 && m != 0xCC => {
                if result.is_none() {
                    let len = r.be16() as usize;
                    let bits = r.getc().map(|b| b as u32).unwrap_or(u32::MAX);
                    let h = r.be16() as u32;
                    let w = r.be16() as u32;
                    let ch = r.getc().map(|b| b as u32).unwrap_or(u32::MAX);
                    result = Some(ImageSize::px(w, h, IMAGETYPE_JPEG, bits, ch));
                    if info.is_none() {
                        return result;
                    }
                    r.skip(len.saturating_sub(8));
                } else {
                    let len = r.be16() as usize;
                    if len < 2 {
                        return None;
                    }
                    r.skip(len - 2);
                }
            }
            0xE0..=0xEF if info.is_some() => {
                let len = r.be16() as usize;
                if len < 2 {
                    return None;
                }
                let data = r.read(len - 2)?;
                let key = format!("APP{}", m - 0xE0);
                let list = info.as_deref_mut().unwrap();
                if !list.iter().any(|(k, _)| *k == key) {
                    list.push((key, data.to_vec()));
                }
            }
            M_SOS | M_EOI => return result,
            M_SOI => {}
            _ => {
                let len = r.be16() as usize;
                if len < 2 {
                    return None;
                }
                r.skip(len - 2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_and_png() {
        let mut d = Vec::new();
        let (r, _) = image_size(b"GIF89a\x01\x02\x03\x04\x85\0\0", false, &mut d);
        let r = r.unwrap();
        assert_eq!((r.width, r.height, r.bits, r.channels), (513, 1027, 6, 3));
        let (r, _) = image_size(
            b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x05\0\0\0\x06\x10\x06",
            false,
            &mut d,
        );
        assert_eq!(r.unwrap().bits, 16);
        assert!(d.is_empty());
    }

    #[test]
    fn short_input_notice() {
        let mut d = Vec::new();
        assert_eq!(image_type(b"ab", &mut d), IMAGETYPE_UNKNOWN);
        assert_eq!(d, vec![Diag::ReadError]);
    }

    #[test]
    fn svg() {
        let mut d = Vec::new();
        let (r, _) = image_size(b"<svg width=\"10mm\" height=\"2\"/>", false, &mut d);
        let r = r.unwrap();
        assert_eq!((r.width, r.height, r.html.is_none()), (10, 2, true));
        assert_eq!(r.width_unit, "mm");
    }
}
