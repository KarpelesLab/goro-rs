//! goro's default rules: magic(5) rules and native checks written from the
//! format specifications of the file types it recognises (PNG, GIF89a,
//! JFIF/JPEG ITU T.81, BMP, TGA, ZIP APPNOTE, ISO/IEC 14496-12 ISO base
//! media, EBML/Matroska, WOFF, XZ, Zstandard RFC 8878, ID3v2 and MPEG-1/2
//! audio ISO/IEC 11172-3, PDF ISO 32000, POSIX ustar, SIMH magtape, ...).
//! The description strings are the ones PHP 8.5's finfo prints for these
//! types (expected output of ext/fileinfo/tests and observation).

use crate::eval::Found;
use crate::text::{Enc, TextInfo};

pub const BUILTIN_MAGIC: &str = include_str!("builtin.magic");

fn found(desc: impl Into<Vec<u8>>, mime: Option<&str>, ext: Option<&str>, apple: Option<&str>) -> Found {
    Found {
        desc: desc.into(),
        mime: mime.map(|s| s.as_bytes().to_vec()),
        ext: ext.map(|s| s.as_bytes().to_vec()),
        apple: apple.map(|s| s.as_bytes().to_vec()),
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Tests file(1) runs before the magic rules whatever the database:
/// SIMH tape images, JSON, CSV and compound documents.
pub fn pre_tests(b: &[u8], ti: &TextInfo, out: &mut Vec<Found>) {
    if simh(b) {
        out.push(found("SIMH tape data", Some("application/SIMH-tape-data"), None, None));
        return;
    }
    if ti.is_text() && matches!(ti.enc, Enc::Ascii | Enc::Utf8 | Enc::Utf8Bom) {
        let body = b.strip_prefix(b"\xef\xbb\xbf").unwrap_or(b);
        if let Some(n) = crate::text::json_values(body) {
            if n > 1 {
                out.push(found(
                    "New Line Delimited JSON text data",
                    Some("application/x-ndjson"),
                    None,
                    None,
                ));
            } else {
                out.push(found("JSON text data", Some("application/json"), None, None));
            }
            return;
        }
        if crate::text::is_csv(body) {
            let d = format!("CSV {}", ti.describe());
            out.push(found(d, Some("text/csv"), None, None));
            return;
        }
    }
    if let Some(f) = crate::cdf::detect(b) {
        out.push(f);
    }
}

/// Tests file(1) runs after the magic rules: tar archives.
pub fn post_tests(b: &[u8], out: &mut Vec<Found>) {
    if let Some(f) = tar(b) {
        out.push(f);
    }
}

/// SIMH magtape (SIMH "Magtape Representation and Handling"): records of
/// a 32-bit little-endian word (flags in the top byte, the length in the
/// low 24 bits), the data padded to an even size and the same word again;
/// zero words are tape marks, all-ones the end of medium. At least one
/// whole record is needed; a record cut by the end of the buffer ends the
/// walk (observed on PHP 8.5).
fn simh(b: &[u8]) -> bool {
    let mut p = 0usize;
    let mut records = 0;
    let mut steps = 0;
    while p + 4 <= b.len() && steps < 256 {
        steps += 1;
        let w = le32(b, p).unwrap_or(0);
        if w == 0 {
            p += 4;
            continue;
        }
        if w == 0xffff_ffff {
            break;
        }
        let len = (w & 0x00ff_ffff) as usize;
        let end = p + 4 + len + (len & 1);
        if end + 4 > b.len() {
            break;
        }
        if le32(b, end) != Some(w) {
            return false;
        }
        records += 1;
        p = end + 4;
    }
    records > 0
}

fn oct(s: &[u8]) -> Option<u64> {
    let s: Vec<u8> = s
        .iter()
        .copied()
        .skip_while(|c| *c == b' ')
        .take_while(|c| (b'0'..=b'7').contains(c))
        .collect();
    if s.is_empty() {
        return None;
    }
    let mut v = 0u64;
    for c in s {
        v = v * 8 + (c - b'0') as u64;
    }
    Some(v)
}

/// POSIX.1 ustar headers (and pre-POSIX tar with a valid checksum).
fn tar(b: &[u8]) -> Option<Found> {
    let h = b.get(..512)?;
    let sum = oct(&h[148..156])?;
    let mut s = 0u64;
    for (i, &c) in h.iter().enumerate() {
        s += if (148..156).contains(&i) { 32 } else { c as u64 };
    }
    if s != sum {
        return None;
    }
    let (d, mime) = if &h[257..265] == b"ustar  \0" {
        ("POSIX tar archive (GNU)", "application/x-tar")
    } else if &h[257..263] == b"ustar\0" {
        ("POSIX tar archive", "application/x-tar")
    } else {
        ("tar archive", "application/x-tar")
    };
    Some(found(d, Some(mime), Some("tar"), None))
}

/// Native checks of the default database, before its magic rules.
pub fn native(b: &[u8], _ti: &TextInfo, out: &mut Vec<Found>) {
    let checks: &[fn(&[u8]) -> Option<Found>] = &[jpeg, id3, pdf, zip, tga, gpkg];
    for c in checks {
        if let Some(f) = c(b) {
            out.push(f);
            return;
        }
    }
}

/// Native text checks of the default database (after its text rules).
pub fn native_text(_b: &[u8], _ti: &TextInfo, _out: &mut Vec<Found>) {}

// ----------------------------------------------------------------- JPEG

/// JPEG (ITU T.81) with JFIF APP0 (JFIF 1.02 specification).
fn jpeg(b: &[u8]) -> Option<Found> {
    if !b.starts_with(b"\xff\xd8\xff") {
        return None;
    }
    let mut d = String::from("JPEG image data");
    let mut p = 2usize;
    if b.get(3) == Some(&0xe0) && b.get(6..11) == Some(b"JFIF\0") {
        let seg = be16(b, 4).unwrap_or(0);
        if let (Some(&maj), Some(&min)) = (b.get(11), b.get(12)) {
            d.push_str(&format!(", JFIF standard {}.{:02}", maj, min));
        }
        match b.get(13) {
            Some(0) => d.push_str(", aspect ratio"),
            Some(1) => d.push_str(", resolution (DPI)"),
            Some(2) => d.push_str(", resolution (DPCM)"),
            _ => {}
        }
        if let (Some(x), Some(y)) = (be16(b, 14), be16(b, 16)) {
            d.push_str(&format!(", density {}x{}", x, y));
        }
        d.push_str(&format!(", segment length {}", seg));
        if let (Some(&tx), Some(&ty)) = (b.get(18), b.get(19))
            && tx > 0
            && ty > 0
        {
            d.push_str(&format!(", thumbnail {}x{}", tx, ty));
        }
        p = 4 + seg as usize;
    } else if b.get(3) == Some(&0xe1) && b.get(6..12) == Some(b"Exif\0\0") {
        let seg = be16(b, 4).unwrap_or(0);
        d.push_str(", Exif standard");
        p = 4 + seg as usize;
    }
    // Following segments: comments and the frame header.
    let mut guard = 0;
    while p + 4 <= b.len() && guard < 64 {
        guard += 1;
        if b[p] != 0xff {
            break;
        }
        let m = b[p + 1];
        if m == 0xff {
            p += 1;
            continue;
        }
        let len = be16(b, p + 2).unwrap_or(0) as usize;
        match m {
            0xfe => {
                let st = p + 4;
                let en = (p + 2 + len).min(b.len()).max(st);
                let c = &b[st..en];
                let c: Vec<u8> = c.iter().copied().take_while(|&x| x != 0).collect();
                d.push_str(", comment: \"");
                d.push_str(&String::from_utf8_lossy(&c));
                d.push('"');
            }
            0xc0..=0xcf if !matches!(m, 0xc4 | 0xc8 | 0xcc) => {
                let kind = match m {
                    0xc0 => "baseline",
                    0xc1 => "extended sequential",
                    0xc2 => "progressive",
                    0xc3 => "lossless",
                    _ => "hierarchical",
                };
                if let (Some(&prec), Some(h), Some(w), Some(&nc)) =
                    (b.get(p + 4), be16(b, p + 5), be16(b, p + 7), b.get(p + 9))
                {
                    d.push_str(&format!(
                        ", {}, precision {}, {}x{}, components {}",
                        kind, prec, w, h, nc
                    ));
                }
                break;
            }
            0xda | 0xd9 => break,
            _ => {}
        }
        p += 2 + len;
    }
    Some(found(
        d,
        Some("image/jpeg"),
        Some("jpeg/jpg/jpe/jfif"),
        Some("8BIMJPEG"),
    ))
}

// ------------------------------------------------------------ ID3 / MP3

const BITRATES: [[u32; 16]; 5] = [
    // MPEG-1 layer I, II, III
    [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 0],
    [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0],
    [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0],
    // MPEG-2/2.5 layer I, layers II and III
    [0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256, 0],
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0],
];

/// An MPEG audio frame header (ISO/IEC 11172-3 2.4.1.3, 13818-3).
pub fn mpeg_audio(h: &[u8]) -> Option<String> {
    if h.len() < 4 || h[0] != 0xff || h[1] & 0xe0 != 0xe0 {
        return None;
    }
    let ver = (h[1] >> 3) & 3;
    let layer = (h[1] >> 1) & 3;
    if ver == 1 || layer == 0 {
        return None;
    }
    let bri = (h[2] >> 4) as usize;
    let sri = ((h[2] >> 2) & 3) as usize;
    if bri == 15 || sri == 3 {
        return None;
    }
    let lname = match layer {
        3 => "I",
        2 => "II",
        _ => "III",
    };
    let vname = match ver {
        3 => "v1",
        2 => "v2",
        _ => "v2.5",
    };
    let table = if ver == 3 {
        3 - layer as usize
    } else if layer == 3 {
        3
    } else {
        4
    };
    let rates: [u32; 3] = match ver {
        3 => [44100, 48000, 32000],
        2 => [22050, 24000, 16000],
        _ => [11025, 12000, 8000],
    };
    let sr = rates[sri];
    let srs = if sr % 1000 == 0 {
        format!("{} kHz", sr / 1000)
    } else {
        format!("{} kHz", sr as f64 / 1000.0)
    };
    let mode = match h[3] >> 6 {
        0 => "Stereo",
        1 => "JntStereo",
        2 => "2x Monaural",
        _ => "Monaural",
    };
    let mut s = format!("MPEG ADTS, layer {}, {}", lname, vname);
    if bri != 0 {
        s.push_str(&format!(", {} kbps", BITRATES[table][bri]));
    }
    s.push_str(&format!(", {}, {}", srs, mode));
    Some(s)
}

/// ID3v2 tags (id3.org ID3v2.3/2.4 structure) and bare MPEG audio.
fn id3(b: &[u8]) -> Option<Found> {
    if !b.starts_with(b"ID3") || b.len() < 10 {
        return None;
    }
    let maj = b[3];
    let min = b[4];
    if maj == 0xff || min == 0xff {
        return None;
    }
    let mut d = format!("Audio file with ID3 version 2.{}.{}", maj, min);
    if b[5] & 0x40 != 0 {
        d.push_str(", extended header");
    }
    let size = ((b[6] as usize & 0x7f) << 21)
        | ((b[7] as usize & 0x7f) << 14)
        | ((b[8] as usize & 0x7f) << 7)
        | (b[9] as usize & 0x7f);
    let at = 10 + size;
    let mut mime = "application/octet-stream";
    if let Some(m) = b.get(at..).and_then(mpeg_audio) {
        d.push_str(", contains: ");
        d.push_str(&m);
        mime = "audio/mpeg";
    }
    Some(found(d, Some(mime), None, None))
}

// ------------------------------------------------------------------ PDF

/// PDF (ISO 32000-1 7.5.2 header, 7.7.3.2 page tree /Count).
fn pdf(b: &[u8]) -> Option<Found> {
    if !b.starts_with(b"%PDF-") {
        return None;
    }
    let v: Vec<u8> = b[5..]
        .iter()
        .copied()
        .take_while(|c| c.is_ascii_digit() || *c == b'.')
        .collect();
    let mut d = format!("PDF document, version {}", String::from_utf8_lossy(&v));
    // The root page tree's /Count.
    let mut i = 0;
    while let Some(p) = find(&b[i..], b"/Type") {
        let st = i + p + 5;
        let mut j = st;
        while j < b.len() && matches!(b[j], b' ' | b'\r' | b'\n' | b'\t') {
            j += 1;
        }
        if b[j..].starts_with(b"/Pages") && !b[j + 6..].first().is_some_and(|c| c.is_ascii_alphanumeric()) {
            // Search the dictionary around it for /Count.
            let lo = (i + p).saturating_sub(512);
            let hi = (st + 512).min(b.len());
            if let Some(c) = find(&b[lo..hi], b"/Count") {
                let k = lo + c + 6;
                let n: String = b[k..]
                    .iter()
                    .skip_while(|c| c.is_ascii_whitespace())
                    .take_while(|c| c.is_ascii_digit())
                    .map(|&c| c as char)
                    .collect();
                if !n.is_empty() {
                    d.push_str(&format!(", {} page(s)", n));
                }
            }
            break;
        }
        i = st;
    }
    Some(found(d, Some("application/pdf"), Some("pdf"), None))
}

pub fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    h.windows(n.len()).position(|w| w == n)
}

// ------------------------------------------------------------------ ZIP

/// ZIP (PKWARE APPNOTE.TXT 4.3.7 local file header), with the
/// OpenDocument/EPUB "mimetype" first entry and Office Open XML parts.
fn zip(b: &[u8]) -> Option<Found> {
    if !b.starts_with(b"PK\x03\x04") || b.len() < 30 {
        return None;
    }
    // The low byte is the version, the high byte the host system.
    let need = b[4] as u16;
    let method = le16(b, 8)?;
    let csize = le32(b, 18)? as usize;
    let nlen = le16(b, 26)? as usize;
    let xlen = le16(b, 28)? as usize;
    let name = b.get(30..30 + nlen)?;
    if name == b"mimetype" && method == 0 {
        let st = 30 + nlen + xlen;
        let m = b.get(st..st + csize)?;
        let ms = String::from_utf8_lossy(m).into_owned();
        let known: Option<(&str, &str)> = match ms.as_str() {
            "application/vnd.oasis.opendocument.text" => Some(("OpenDocument Text", "odt")),
            "application/vnd.oasis.opendocument.spreadsheet" => {
                Some(("OpenDocument Spreadsheet", "ods"))
            }
            "application/vnd.oasis.opendocument.presentation" => {
                Some(("OpenDocument Presentation", "odp"))
            }
            "application/vnd.oasis.opendocument.graphics" => Some(("OpenDocument Drawing", "odg")),
            "application/epub+zip" => Some(("EPUB document", "epub")),
            _ => None,
        };
        return Some(match known {
            Some((d, e)) => found(d, Some(&ms), Some(e), None),
            None => found(
                format!("Zip data (MIME type \"{}\"?)", ms),
                Some(&ms),
                None,
                None,
            ),
        });
    }
    // Office Open XML (ECMA-376 Part 2 package parts).
    let mut names: Vec<&[u8]> = Vec::new();
    let mut p = 0;
    while let Some(k) = find(&b[p..], b"PK\x03\x04") {
        let h = p + k;
        if let (Some(nl), Some(_)) = (le16(b, h + 26), le16(b, h + 28))
            && let Some(n) = b.get(h + 30..h + 30 + nl as usize)
        {
            names.push(n);
        }
        p = h + 4;
        if names.len() > 1000 {
            break;
        }
    }
    let ooxml = names
        .iter()
        .any(|n| *n == b"[Content_Types].xml" || *n == b"_rels/.rels");
    if ooxml {
        let any = |pre: &[u8]| names.iter().any(|n| n.starts_with(pre));
        if any(b"word/") {
            return Some(found(
                "Microsoft Word 2007+",
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
                Some("docx"),
                None,
            ));
        }
        if any(b"xl/") {
            return Some(found(
                "Microsoft Excel 2007+",
                Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
                Some("xlsx"),
                None,
            ));
        }
        if any(b"ppt/") {
            return Some(found(
                "Microsoft PowerPoint 2007+",
                Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
                Some("pptx"),
                None,
            ));
        }
    }
    let meth = match method {
        0 => "store".to_string(),
        8 => "deflate".to_string(),
        9 => "deflate64".to_string(),
        12 => "bzip2".to_string(),
        14 => "lzma".to_string(),
        93 => "zstd".to_string(),
        95 => "xz".to_string(),
        98 => "ppmd".to_string(),
        n => format!("{}", n),
    };
    Some(found(
        format!(
            "Zip archive data, at least v{}.{} to extract, compression method={}",
            need / 10,
            need % 10,
            meth
        ),
        Some("application/zip"),
        None,
        None,
    ))
}

// ------------------------------------------------------------------ TGA

/// Truevision TGA (TGA File Format Specification 2.0 header).
fn tga(b: &[u8]) -> Option<Found> {
    if b.len() < 18 {
        return None;
    }
    let cmap = b[1];
    let ty = b[2];
    let depth = b[16];
    if cmap > 1 || !matches!(ty, 1 | 2 | 3 | 9 | 10 | 11) || !matches!(depth, 1 | 8 | 15 | 16 | 24 | 32) {
        return None;
    }
    if cmap == 0 && b[3..8].iter().any(|&c| c != 0) {
        return None;
    }
    if b[17] & 0xc0 != 0 {
        return None;
    }
    let w = le16(b, 12)?;
    let h = le16(b, 14)?;
    if w == 0 || h == 0 {
        return None;
    }
    let kind = match ty {
        1 => "Map",
        2 => "RGB",
        3 => "Mono",
        9 => "Map - RLE",
        10 => "RGB - RLE",
        _ => "Mono - RLE",
    };
    let mut d = format!("Targa image data - {} {} x {} x {}", kind, w, h, depth);
    let (x, y) = (le16(b, 8)?, le16(b, 10)?);
    if x != 0 || y != 0 {
        d.push_str(&format!(" +{} +{}", x, y));
    }
    Some(found(
        d,
        Some("image/x-tga"),
        Some("tga/tpic/icb/vda/vst"),
        Some("????TPIC"),
    ))
}

// ----------------------------------------------------------------- GPKG

/// Gentoo GLEP 78 binary packages: a ustar archive whose first member is
/// "<name>/gpkg-1", followed by "<name>/metadata.tar.<compression>".
fn gpkg(b: &[u8]) -> Option<Found> {
    let h = b.get(..512)?;
    if &h[257..263] != b"ustar\0" {
        return None;
    }
    let name: Vec<u8> = h[..100].iter().copied().take_while(|&c| c != 0).collect();
    let pkg = name.strip_suffix(b"/gpkg-1")?;
    let mut d = format!(
        "Gentoo GLEP 78 (GPKG) binary package for \"{}\"",
        String::from_utf8_lossy(pkg)
    );
    // The next member follows the gpkg-1 file's data.
    let size = oct(&h[124..136]).unwrap_or(0) as usize;
    let next = 512 + size.div_ceil(512) * 512;
    if let Some(nh) = b.get(next..next + 100) {
        let n: Vec<u8> = nh.iter().copied().take_while(|&c| c != 0).collect();
        let comp = [
            (".tar.zst", "zstd"),
            (".tar.gz", "gzip"),
            (".tar.bz2", "bzip2"),
            (".tar.xz", "xz"),
            (".tar.lz4", "lz4"),
            (".tar.lz", "lzip"),
            (".tar.lzo", "lzop"),
        ];
        for (sfx, c) in comp {
            if n.ends_with(sfx.as_bytes()) {
                d.push_str(&format!(" using {} compression", c));
                break;
            }
        }
    }
    Some(found(d, Some("application/x-tar"), None, None))
}
