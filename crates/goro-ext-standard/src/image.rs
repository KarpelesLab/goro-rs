//! Image size detection for getimagesize() and the IPTC helpers
//! iptcparse() / iptcembed() (PHP manual pages of these functions; the
//! file formats from their public specifications; behaviours observed on
//! PHP 8.5 and in tests ext/standard/tests/image/*.phpt).

pub const GIF: i64 = 1;
pub const JPEG: i64 = 2;
pub const PNG: i64 = 3;
pub const SWF: i64 = 4;
pub const PSD: i64 = 5;
pub const BMP: i64 = 6;
pub const TIFF_II: i64 = 7;
pub const TIFF_MM: i64 = 8;
pub const JPC: i64 = 9;
pub const JP2: i64 = 10;
pub const JPX: i64 = 11;
pub const JB2: i64 = 12;
pub const SWC: i64 = 13;
pub const IFF: i64 = 14;
pub const WBMP: i64 = 15;
pub const XBM: i64 = 16;
pub const ICO: i64 = 17;
pub const WEBP: i64 = 18;
pub const AVIF: i64 = 19;
pub const HEIF: i64 = 20;
pub const SVG: i64 = 21;
pub const COUNT: i64 = 22;

/// image_type_to_mime_type().
pub fn mime_type(t: i64) -> &'static str {
    match t {
        GIF => "image/gif",
        JPEG => "image/jpeg",
        PNG => "image/png",
        SWF | SWC => "application/x-shockwave-flash",
        PSD => "image/psd",
        BMP => "image/bmp",
        TIFF_II | TIFF_MM => "image/tiff",
        JP2 => "image/jp2",
        IFF => "image/iff",
        WBMP => "image/vnd.wap.wbmp",
        XBM => "image/xbm",
        ICO => "image/vnd.microsoft.icon",
        WEBP => "image/webp",
        AVIF => "image/avif",
        HEIF => "image/heif",
        SVG => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// image_type_to_extension(), without the dot.
pub fn extension(t: i64) -> Option<&'static str> {
    Some(match t {
        GIF => "gif",
        JPEG => "jpeg",
        PNG => "png",
        SWF | SWC => "swf",
        PSD => "psd",
        BMP | WBMP => "bmp",
        TIFF_II | TIFF_MM => "tiff",
        JPC => "jpc",
        JP2 => "jp2",
        JPX => "jpx",
        JB2 => "jb2",
        IFF => "iff",
        XBM => "xbm",
        ICO => "ico",
        WEBP => "webp",
        AVIF => "avif",
        HEIF => "heif",
        SVG => "svg",
        _ => return None,
    })
}

/// What getimagesize() reports.
#[derive(Debug, Default, Clone)]
pub struct Info {
    pub width: u64,
    pub height: u64,
    pub kind: i64,
    pub bits: Option<u64>,
    pub channels: Option<u64>,
}

/// Diagnostics and APP segments gathered while scanning.
#[derive(Debug, Default)]
pub struct Extra {
    /// JPEG APPn segments, first occurrence of each.
    pub app: Vec<(u8, Vec<u8>)>,
    pub warnings: Vec<String>,
}

fn be16(b: &[u8], i: usize) -> Option<u64> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as u64)
}
fn le16(b: &[u8], i: usize) -> Option<u64> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as u64)
}
fn be32(b: &[u8], i: usize) -> Option<u64> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?) as u64)
}
fn le32(b: &[u8], i: usize) -> Option<u64> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as u64)
}
fn le24(b: &[u8], i: usize) -> Option<u64> {
    let s = b.get(i..i + 3)?;
    Some(s[0] as u64 | (s[1] as u64) << 8 | (s[2] as u64) << 16)
}

/// Detects the image type and size of `data`. `want_app` gathers the
/// JPEG APPn segments; `inflate` decompresses zlib data (SWC files).
pub fn image_size(
    data: &[u8],
    want_app: bool,
    inflate: &dyn Fn(&[u8]) -> Option<Vec<u8>>,
    extra: &mut Extra,
) -> Option<Info> {
    let d = data;
    if d.starts_with(b"GIF") {
        return gif(d);
    }
    if d.starts_with(b"\xFF\xD8\xFF") {
        return jpeg(d, want_app, extra);
    }
    if d.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png(d);
    }
    if d.starts_with(b"FWS") {
        return swf(d.get(8..)?, SWF);
    }
    if d.starts_with(b"CWS") {
        let raw = inflate(d.get(8..)?)?;
        return swf(&raw, SWC);
    }
    if d.starts_with(b"8BPS") {
        return Some(Info {
            width: be32(d, 18)?,
            height: be32(d, 14)?,
            kind: PSD,
            ..Info::default()
        });
    }
    if d.starts_with(b"BM") {
        return bmp(d);
    }
    if d.starts_with(b"II\x2a\x00") {
        return tiff(d, false);
    }
    if d.starts_with(b"MM\x00\x2a") {
        return tiff(d, true);
    }
    if d.starts_with(b"\xFF\x4F\xFF\x51") {
        return jpc(d, JPC);
    }
    if d.starts_with(b"\x00\x00\x00\x0cjP  \r\n\x87\n") {
        return jp2(d);
    }
    if d.starts_with(b"FORM") {
        return iff(d);
    }
    if d.starts_with(b"\x00\x00\x01\x00") {
        return ico(d);
    }
    if d.len() >= 12 && d.starts_with(b"RIFF") && &d[8..12] == b"WEBP" {
        return webp(d);
    }
    if d.len() >= 12 && &d[4..8] == b"ftyp" {
        if let Some(i) = isobmff(d) {
            return Some(i);
        }
    }
    if let Some(i) = wbmp(d) {
        return Some(i);
    }
    xbm(d)
}

fn gif(d: &[u8]) -> Option<Info> {
    if !(d.starts_with(b"GIF87a") || d.starts_with(b"GIF89a")) {
        return None;
    }
    let flags = *d.get(10)?;
    Some(Info {
        width: le16(d, 6)?,
        height: le16(d, 8)?,
        kind: GIF,
        bits: Some((flags & 7) as u64 + 1),
        channels: Some(3),
    })
}

fn png(d: &[u8]) -> Option<Info> {
    if d.get(12..16)? != b"IHDR" {
        return None;
    }
    // the IHDR chunk is read whole (with its CRC)
    d.get(..33)?;
    Some(Info {
        width: be32(d, 16)?,
        height: be32(d, 20)?,
        kind: PNG,
        bits: Some(*d.get(24)? as u64),
        channels: None,
    })
}

fn jpeg(d: &[u8], want_app: bool, extra: &mut Extra) -> Option<Info> {
    let mut i = 2;
    let mut found: Option<Info> = None;
    loop {
        // the next marker: bytes other than 0xFF before it are extraneous
        let mut junk = 0usize;
        while i < d.len() && d[i] != 0xFF {
            junk += 1;
            i += 1;
        }
        while i < d.len() && d[i] == 0xFF {
            i += 1;
        }
        if i >= d.len() {
            return found;
        }
        if junk > 0 {
            extra.warnings.push(format!(
                "Corrupt JPEG data: {junk} extraneous bytes before marker"
            ));
        }
        let m = d[i];
        i += 1;
        match m {
            0xD9 | 0xDA => return found,
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        let len = be16(d, i)? as usize;
        if len < 2 {
            return found;
        }
        let seg = &d[i + 2..(i + len).min(d.len())];
        match m {
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF if found.is_none() => {
                found = Some(Info {
                    width: be16(seg, 3)?,
                    height: be16(seg, 1)?,
                    kind: JPEG,
                    bits: Some(*seg.first()? as u64),
                    channels: Some(*seg.get(5)? as u64),
                });
                if !want_app {
                    return found;
                }
            }
            0xE0..=0xEF if want_app => {
                let n = m - 0xE0;
                if seg.len() < len - 2 {
                    return found;
                }
                if !extra.app.iter().any(|(k, _)| *k == n) {
                    extra.app.push((n, seg.to_vec()));
                }
            }
            _ => {}
        }
        i += len;
    }
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn get(&mut self, n: u32) -> Option<u64> {
        let mut v = 0u64;
        for _ in 0..n {
            let byte = *self.d.get(self.pos / 8)?;
            let bit = (byte >> (7 - self.pos % 8)) & 1;
            v = v << 1 | bit as u64;
            self.pos += 1;
        }
        Some(v)
    }
}

fn swf(d: &[u8], kind: i64) -> Option<Info> {
    let mut b = Bits { d, pos: 0 };
    let n = b.get(5)? as u32;
    let mut v = [0i64; 4];
    for x in v.iter_mut() {
        let raw = b.get(n)?;
        // signed n-bit values
        *x = if n > 0 && raw >> (n - 1) & 1 == 1 {
            raw as i64 - (1i64 << n)
        } else {
            raw as i64
        };
    }
    Some(Info {
        width: ((v[1] - v[0]) / 20) as u64,
        height: ((v[3] - v[2]) / 20) as u64,
        kind,
        ..Info::default()
    })
}

fn bmp(d: &[u8]) -> Option<Info> {
    d.get(..30)?;
    let hs = le32(d, 14)?;
    if hs == 12 {
        return Some(Info {
            width: le16(d, 18)?,
            height: le16(d, 20)?,
            kind: BMP,
            ..Info::default()
        });
    }
    if hs < 40 {
        return None;
    }
    let w = le32(d, 18)? as u32 as i32;
    let h = le32(d, 22)? as u32 as i32;
    Some(Info {
        width: w.unsigned_abs() as u64,
        height: h.unsigned_abs() as u64,
        kind: BMP,
        bits: Some(le16(d, 28)?),
        channels: None,
    })
}

fn tiff(d: &[u8], be: bool) -> Option<Info> {
    let r16 = |i| if be { be16(d, i) } else { le16(d, i) };
    let r32 = |i| if be { be32(d, i) } else { le32(d, i) };
    let ifd = r32(4)? as usize;
    let n = r16(ifd)? as usize;
    let (mut w, mut h) = (None, None);
    for k in 0..n {
        let e = ifd + 2 + k * 12;
        let tag = r16(e)?;
        let typ = r16(e + 2)?;
        let val = match typ {
            3 | 8 => r16(e + 8)?,
            4 | 9 => r32(e + 8)?,
            1 | 6 => *d.get(e + 8)? as u64,
            _ => continue,
        };
        match tag {
            0x100 => w = Some(val),
            0x101 => h = Some(val),
            _ => {}
        }
    }
    Some(Info {
        width: w?,
        height: h?,
        kind: if be { TIFF_MM } else { TIFF_II },
        ..Info::default()
    })
}

fn jpc(d: &[u8], kind: i64) -> Option<Info> {
    // SIZ marker segment right after SOC
    let xs = be32(d, 8)?;
    let ys = be32(d, 12)?;
    let xo = be32(d, 16)?;
    let yo = be32(d, 20)?;
    let comps = be16(d, 40)?;
    let mut bits = 0;
    for c in 0..comps as usize {
        let s = *d.get(42 + c * 3)?;
        bits = bits.max((s & 0x7f) as u64 + 1);
    }
    Some(Info {
        width: xs.wrapping_sub(xo),
        height: ys.wrapping_sub(yo),
        kind,
        bits: Some(bits),
        channels: Some(comps),
    })
}

fn jp2(d: &[u8]) -> Option<Info> {
    let mut i = 0;
    while i + 8 <= d.len() {
        let mut size = be32(d, i)? as usize;
        let typ = &d[i + 4..i + 8];
        let mut hdr = 8;
        if size == 1 {
            size =
                usize::try_from(u64::from_be_bytes(d.get(i + 8..i + 16)?.try_into().ok()?)).ok()?;
            hdr = 16;
        } else if size == 0 {
            size = d.len() - i;
        }
        if typ == b"jp2c" {
            let cs = d.get(i + hdr..)?;
            if !cs.starts_with(b"\xFF\x4F\xFF\x51") {
                return None;
            }
            let mut r = jpc(cs, JP2)?;
            r.kind = JP2;
            return Some(r);
        }
        if size < hdr {
            return None;
        }
        i += size;
    }
    None
}

fn iff(d: &[u8]) -> Option<Info> {
    let form = d.get(8..12)?;
    if form != b"ILBM" && form != b"PBM " {
        return None;
    }
    let mut i = 12;
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let size = be32(d, i + 4)? as usize;
        if id == b"BMHD" {
            let c = d.get(i + 8..i + 8 + 9)?;
            return Some(Info {
                width: be16(c, 0)?,
                height: be16(c, 2)?,
                kind: IFF,
                bits: Some(c[8] as u64),
                channels: None,
            });
        }
        i += 8 + size + (size & 1);
    }
    None
}

fn ico(d: &[u8]) -> Option<Info> {
    let n = le16(d, 4)? as usize;
    let mut best: Option<Info> = None;
    let mut best_bits = 0;
    for k in 0..n {
        let e = d.get(6 + k * 16..6 + k * 16 + 16)?;
        let bits = le16(e, 6)?;
        // the deepest entry wins, the later one on a tie
        if bits >= best_bits {
            best_bits = bits;
            let w = if e[0] == 0 { 256 } else { e[0] as u64 };
            let h = if e[1] == 0 { 256 } else { e[1] as u64 };
            best = Some(Info {
                width: w,
                height: h,
                kind: ICO,
                bits: Some(bits),
                channels: None,
            });
        }
    }
    best
}

fn webp(d: &[u8]) -> Option<Info> {
    let chunk = d.get(12..16)?;
    let (w, h) = match chunk {
        b"VP8 " => {
            if d.get(23..26)? != b"\x9d\x01\x2a" {
                return None;
            }
            (le16(d, 26)? & 0x3fff, le16(d, 28)? & 0x3fff)
        }
        b"VP8L" => {
            if *d.get(20)? != 0x2f {
                return None;
            }
            let v = le32(d, 21)?;
            ((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1)
        }
        b"VP8X" => (le24(d, 24)? + 1, le24(d, 27)? + 1),
        _ => return None,
    };
    Some(Info {
        width: w,
        height: h,
        kind: WEBP,
        bits: Some(8),
        channels: None,
    })
}

/// A box of an ISO base media file: (type, payload range).
fn boxes(d: &[u8], mut i: usize, end: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    while i + 8 <= end {
        let Some(mut size) = be32(d, i).map(|s| s as usize) else {
            break;
        };
        let mut typ = [0u8; 4];
        typ.copy_from_slice(&d[i + 4..i + 8]);
        let mut hdr = 8;
        if size == 1 {
            let Some(s) = d.get(i + 8..i + 16) else {
                break;
            };
            size = u64::from_be_bytes(s.try_into().unwrap_or_default()) as usize;
            hdr = 16;
        } else if size == 0 {
            size = end - i;
        }
        if size < hdr || i.saturating_add(size) > end {
            break;
        }
        out.push((typ, i + hdr, i + size));
        i += size;
    }
    out
}

fn isobmff(d: &[u8]) -> Option<Info> {
    let top = boxes(d, 0, d.len());
    let (_, fs, fe) = *top.first().filter(|b| &b.0 == b"ftyp")?;
    let mut brands: Vec<&[u8]> = vec![d.get(fs..fs + 4)?];
    let mut j = fs + 8;
    while j + 4 <= fe {
        brands.push(&d[j..j + 4]);
        j += 4;
    }
    let kind = if brands.iter().any(|b| *b == b"avif" || *b == b"avis") {
        AVIF
    } else if brands.iter().any(|b| {
        matches!(
            *b,
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"hevm" | b"hevs" | b"mif1"
        )
    }) {
        HEIF
    } else {
        return None;
    };
    let (_, ms, me) = *top.iter().find(|b| &b.0 == b"meta")?;
    // meta is a full box: version and flags first
    let meta = boxes(d, ms + 4, me);
    let mut primary = 1u64;
    let mut props: Vec<([u8; 4], usize, usize)> = Vec::new();
    let mut assoc: Vec<(u64, Vec<usize>)> = Vec::new();
    let mut aux_alpha: Vec<u64> = Vec::new();
    let mut auxl: Vec<(u64, Vec<u64>)> = Vec::new();
    for (t, s, e) in &meta {
        match t {
            b"pitm" => {
                primary = if d[*s] == 0 {
                    be16(d, s + 4)?
                } else {
                    be32(d, s + 4)?
                };
            }
            b"iprp" => {
                for (t2, s2, e2) in boxes(d, *s, *e) {
                    if &t2 == b"ipco" {
                        props = boxes(d, s2, e2);
                    } else if &t2 == b"ipma" {
                        let ver = d[s2];
                        let flags = be32(d, s2)? & 0xffffff;
                        let n = be32(d, s2 + 4)?;
                        let mut p = s2 + 8;
                        for _ in 0..n {
                            if p >= e2 {
                                break;
                            }
                            let id = if ver < 1 {
                                let v = be16(d, p)?;
                                p += 2;
                                v
                            } else {
                                let v = be32(d, p)?;
                                p += 4;
                                v
                            };
                            let cnt = *d.get(p)? as usize;
                            p += 1;
                            let mut list = Vec::new();
                            for _ in 0..cnt {
                                let idx = if flags & 1 != 0 {
                                    let v = be16(d, p)? & 0x7fff;
                                    p += 2;
                                    v
                                } else {
                                    let v = *d.get(p)? as u64 & 0x7f;
                                    p += 1;
                                    v
                                };
                                list.push(idx as usize);
                            }
                            assoc.push((id, list));
                        }
                    }
                }
            }
            b"iref" => {
                let ver = d[*s];
                for (t2, s2, e2) in boxes(d, s + 4, *e) {
                    if &t2 != b"auxl" {
                        continue;
                    }
                    let rd = |p: usize| if ver == 0 { be16(d, p) } else { be32(d, p) };
                    let w = if ver == 0 { 2 } else { 4 };
                    let from = rd(s2)?;
                    let cnt = be16(d, s2 + w)? as usize;
                    let mut to = Vec::new();
                    for k in 0..cnt {
                        let p = s2 + w + 2 + k * w;
                        if p + w > e2 {
                            break;
                        }
                        to.push(rd(p)?);
                    }
                    auxl.push((from, to));
                }
            }
            _ => {}
        }
    }
    let props_of = |id: u64| -> Vec<&([u8; 4], usize, usize)> {
        assoc
            .iter()
            .filter(|(i, _)| *i == id)
            .flat_map(|(_, l)| l.iter())
            .filter_map(|&k| if k == 0 { None } else { props.get(k - 1) })
            .collect()
    };
    let mut info = Info {
        kind,
        ..Info::default()
    };
    let mut have_size = false;
    for (t, s, _) in props_of(primary) {
        match t {
            b"ispe" => {
                info.width = be32(d, s + 4)?;
                info.height = be32(d, s + 8)?;
                have_size = true;
            }
            b"pixi" => {
                let ch = *d.get(s + 4)? as u64;
                info.channels = Some(ch);
                info.bits = Some(*d.get(s + 5)? as u64);
            }
            _ => {}
        }
    }
    if !have_size {
        return None;
    }
    // an alpha plane (an auxiliary image of the primary one) adds a channel
    for (from, to) in &auxl {
        if !to.contains(&primary) {
            continue;
        }
        let is_alpha = props_of(*from).iter().any(|(t, s, e)| {
            t == b"auxC" && d[s + 4..*e].starts_with(b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha")
        });
        if is_alpha {
            aux_alpha.push(*from);
        }
    }
    if !aux_alpha.is_empty()
        && let Some(c) = info.channels.as_mut()
    {
        *c += 1;
    }
    Some(info)
}

/// A WBMP multi-byte integer.
fn mbint(d: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    loop {
        let b = *d.get(*i)?;
        *i += 1;
        v = (v << 7) | (b & 0x7f) as u64;
        if v > u32::MAX as u64 {
            return None;
        }
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
}

fn wbmp(d: &[u8]) -> Option<Info> {
    let mut i = 0;
    if mbint(d, &mut i)? != 0 {
        return None;
    }
    // the fixed header byte, then its extension headers
    let mut fix = *d.get(i)?;
    i += 1;
    while fix & 0x80 != 0 {
        fix = *d.get(i)?;
        i += 1;
    }
    let w = mbint(d, &mut i)?;
    let h = mbint(d, &mut i)?;
    if w == 0 || h == 0 || w > 2048 || h > 2048 {
        return None;
    }
    Some(Info {
        width: w,
        height: h,
        kind: WBMP,
        ..Info::default()
    })
}

fn xbm(d: &[u8]) -> Option<Info> {
    let (mut w, mut h) = (None, None);
    for line in d.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(rest) = line.strip_prefix(b"#define") else {
            continue;
        };
        let mut parts = rest
            .split(|c| *c == b' ' || *c == b'\t')
            .filter(|p| !p.is_empty());
        let (Some(name), Some(val)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(n) = std::str::from_utf8(val)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        else {
            continue;
        };
        if name.ends_with(b"_width") {
            w.get_or_insert(n);
        } else if name.ends_with(b"_height") {
            h.get_or_insert(n);
        }
        if w.is_some() && h.is_some() {
            break;
        }
    }
    let (w, h) = (w?, h?);
    if w == 0 || h == 0 {
        return None;
    }
    Some(Info {
        width: w,
        height: h,
        kind: XBM,
        ..Info::default()
    })
}

/// iptcparse(): `(record, dataset, value)` in order, None when the data
/// holds no tag or a tag runs past the end.
pub fn iptc_parse(d: &[u8]) -> Option<Vec<(u8, u8, Vec<u8>)>> {
    let mut i = d.iter().position(|&c| c == 0x1C)?;
    let mut out = Vec::new();
    while i + 5 <= d.len() && d[i] == 0x1C {
        let rec = d[i + 1];
        let ds = d[i + 2];
        let len = u16::from_be_bytes([d[i + 3], d[i + 4]]) as usize;
        if len & 0x8000 != 0 {
            return None;
        }
        i += 5;
        if i + len > d.len() {
            return None;
        }
        out.push((rec, ds, d[i..i + len].to_vec()));
        i += len;
    }
    if out.is_empty() { None } else { Some(out) }
}

/// iptcembed(): the JPEG `jpg` with its APP13 segment replaced by one
/// holding `iptc` (inserted after APP0); None when it is not a JPEG.
pub fn iptc_embed(iptc: &[u8], jpg: &[u8]) -> Option<Vec<u8>> {
    if !jpg.starts_with(b"\xFF\xD8") {
        return None;
    }
    let mut out = b"\xFF\xD8".to_vec();
    let mut i = 2;
    let mut pad = iptc.to_vec();
    if pad.len() % 2 == 1 {
        pad.push(0);
    }
    loop {
        // bytes before the next marker are kept as they are
        while i < jpg.len() && jpg[i] != 0xFF {
            out.push(jpg[i]);
            i += 1;
        }
        while i < jpg.len() && jpg[i] == 0xFF {
            i += 1;
        }
        let Some(&m) = jpg.get(i) else {
            return Some(out);
        };
        i += 1;
        let seglen = |i: usize| {
            jpg.get(i..i + 2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
                .unwrap_or(0)
        };
        match m {
            0xDA | 0xD9 => {
                out.extend_from_slice(&[0xFF, m]);
                out.extend_from_slice(&jpg[i..]);
                return Some(out);
            }
            0xED => {
                // the old IPTC block is dropped
                i += seglen(i).max(2);
            }
            0xE0 => {
                let l = seglen(i).max(2);
                let end = (i + l).min(jpg.len());
                out.extend_from_slice(&[0xFF, 0xE0]);
                out.extend_from_slice(&jpg[i..end]);
                i = end;
                let len = 2 + 14 + 4 + 2 + 2 + 4 + pad.len();
                out.extend_from_slice(&[0xFF, 0xED]);
                out.extend_from_slice(&(len as u16).to_be_bytes());
                out.extend_from_slice(b"Photoshop 3.0\0");
                out.extend_from_slice(b"8BIM\x04\x04\0\0");
                out.extend_from_slice(&(pad.len() as u32).to_be_bytes());
                out.extend_from_slice(&pad);
            }
            0x01 | 0xD0..=0xD8 => out.extend_from_slice(&[0xFF, m]),
            _ => {
                let l = seglen(i).max(2);
                let end = (i + l).min(jpg.len());
                out.extend_from_slice(&[0xFF, m]);
                out.extend_from_slice(&jpg[i..end]);
                i = end;
            }
        }
    }
}
