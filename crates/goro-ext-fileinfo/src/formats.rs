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

fn found(
    desc: impl Into<Vec<u8>>,
    mime: Option<&str>,
    ext: Option<&str>,
    apple: Option<&str>,
) -> Found {
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
fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Tests file(1) runs before the magic rules whatever the database:
/// SIMH tape images, JSON, CSV and compound documents.
pub fn pre_tests(b: &[u8], ti: &TextInfo, out: &mut Vec<Found>) {
    if simh(b) {
        out.push(found(
            "SIMH tape data",
            Some("application/SIMH-tape-data"),
            None,
            None,
        ));
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
                out.push(found(
                    "JSON text data",
                    Some("application/json"),
                    None,
                    None,
                ));
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
        s += if (148..156).contains(&i) {
            32
        } else {
            c as u64
        };
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

type Check = fn(&[u8]) -> Option<Found>;

/// Native checks of the default database, before its magic rules.
pub fn native(b: &[u8], _ti: &TextInfo, out: &mut Vec<Found>) {
    let checks: &[Check] = &[jpeg, id3, pdf, zip, gpkg, openpgp, ext_fs, bcachefs, dsdiff];
    for c in checks {
        if let Some(f) = c(b) {
            out.push(f);
            return;
        }
    }
}

/// Native checks of the default database for formats without a magic
/// number, after its magic rules.
pub fn native_late(b: &[u8], out: &mut Vec<Found>) {
    if let Some(f) = tga(b) {
        out.push(f);
    }
}

fn hex_uuid(u: &[u8]) -> String {
    let h: Vec<String> = u.iter().map(|c| format!("{:02x}", c)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        h[0..4].concat(),
        h[4..6].concat(),
        h[6..8].concat(),
        h[8..10].concat(),
        h[10..16].concat()
    )
}

// ------------------------------------------------------------- OpenPGP

/// OpenPGP transferable keys (RFC 4880 4.2 packet headers, 5.5.2 key
/// material).
fn openpgp(b: &[u8]) -> Option<Found> {
    // (tag, body start, body length) of the packet at `p`.
    let packet = |p: usize| -> Option<(u8, usize, usize)> {
        let h = *b.get(p)?;
        if h & 0x80 == 0 {
            return None;
        }
        if h & 0x40 != 0 {
            let tag = h & 0x3f;
            let l0 = *b.get(p + 1)? as usize;
            if l0 < 192 {
                Some((tag, p + 2, l0))
            } else if l0 < 224 {
                let l1 = *b.get(p + 2)? as usize;
                Some((tag, p + 3, ((l0 - 192) << 8) + l1 + 192))
            } else if l0 == 255 {
                Some((tag, p + 6, be32(b, p + 2)? as usize))
            } else {
                None
            }
        } else {
            let tag = (h >> 2) & 0xf;
            match h & 3 {
                0 => Some((tag, p + 2, *b.get(p + 1)? as usize)),
                1 => Some((tag, p + 3, be16(b, p + 1)? as usize)),
                2 => Some((tag, p + 5, be32(b, p + 1)? as usize)),
                _ => None,
            }
        }
    };
    let (tag, st, len) = packet(0)?;
    let kind = match tag {
        6 => "Public",
        5 => "Secret",
        _ => return None,
    };
    let ver = *b.get(st)?;
    if !(2..=5).contains(&ver) {
        return None;
    }
    let created = be32(b, st + 1)? as i64;
    let algo_at = if ver < 4 { st + 7 } else { st + 5 };
    let algo = *b.get(algo_at)?;
    let bits = be16(b, algo_at + 1);
    let a = match algo {
        1 => format!("RSA (Encrypt or Sign, {} bits)", bits?),
        2 => format!("RSA (Encrypt-Only, {} bits)", bits?),
        3 => format!("RSA (Sign-Only, {} bits)", bits?),
        16 => format!("ElGamal (Encrypt-Only, {} bits)", bits?),
        17 => format!("DSA ({} bits)", bits?),
        18 => "ECDH".to_string(),
        19 => "ECDSA".to_string(),
        22 => "EdDSA".to_string(),
        _ => return None,
    };
    let mut d = format!(
        "OpenPGP {} Key Version {}, Created {}, {}",
        kind,
        ver,
        crate::eval::fmt_time(created, false),
        a
    );
    let mut p = st + len;
    for _ in 0..2 {
        let Some((t, s, l)) = packet(p) else { break };
        let n = match t {
            2 => "Signature",
            7 => "Secret Subkey",
            12 => "Trust",
            13 => "User ID",
            14 => "Public Subkey",
            17 => "User Attribute",
            _ => break,
        };
        d.push_str("; ");
        d.push_str(n);
        p = s + l;
    }
    d.push_str("; OpenPGP Certificate");
    Some(found(
        d,
        Some("application/pgp-keys"),
        Some("asc/gpg/pgp"),
        None,
    ))
}

// ------------------------------------------------------------ ext2/3/4

/// The ext2/ext3/ext4 superblock at offset 1024 (s_magic 0xEF53; feature
/// flags as documented in the kernel's ext4 "Disk Layout" documentation).
fn ext_fs(b: &[u8]) -> Option<Found> {
    let sb = 1024;
    if le16(b, sb + 56)? != 0xef53 {
        return None;
    }
    let rev = le32(b, sb + 76)?;
    let minor = le16(b, sb + 62)?;
    let compat = le32(b, sb + 92)?;
    let incompat = le32(b, sb + 96)?;
    let ro = le32(b, sb + 100)?;
    let kind = if incompat & 0x0040 != 0
        || incompat & 0x0200 != 0
        || ro & 0x0008 != 0
        || incompat & 0x0080 != 0
    {
        "ext4"
    } else if compat & 0x0004 != 0 {
        "ext3"
    } else {
        "ext2"
    };
    let mut d = format!(
        "Linux rev {}.{} {} filesystem data, UUID={}",
        rev,
        minor,
        kind,
        hex_uuid(b.get(sb + 104..sb + 120)?)
    );
    let name: Vec<u8> = b
        .get(sb + 120..sb + 136)?
        .iter()
        .copied()
        .take_while(|&c| c != 0)
        .collect();
    if !name.is_empty() {
        d.push_str(&format!(
            ", volume name \"{}\"",
            String::from_utf8_lossy(&name)
        ));
    }
    if incompat & 0x0004 != 0 {
        d.push_str(" (needs journal recovery)");
    }
    if le16(b, sb + 58)? & 0x0002 != 0 {
        d.push_str(" (errors)");
    }
    if incompat & 0x0001 != 0 {
        d.push_str(" (compressed)");
    }
    if incompat & 0x0040 != 0 {
        d.push_str(" (extents)");
    }
    if incompat & 0x0080 != 0 {
        d.push_str(" (64bit)");
    }
    if ro & 0x0002 != 0 {
        d.push_str(" (large files)");
    }
    if ro & 0x0008 != 0 {
        d.push_str(" (huge files)");
    }
    Some(found(d, None, None, None))
}

// ------------------------------------------------------------- bcachefs

/// bcachefs superblock at 4 KiB: its magic UUID c68573f6-4e1a-45ca-8265-
/// f57f48ba6d81, versions, user UUID, label, device index and count, and
/// the first member's UUID from the members field after the 512-byte
/// layout.
fn bcachefs(b: &[u8]) -> Option<Found> {
    const MAGIC: [u8; 16] = [
        0xc6, 0x85, 0x73, 0xf6, 0x4e, 0x1a, 0x45, 0xca, 0x82, 0x65, 0xf5, 0x7f, 0x48, 0xba, 0x6d,
        0x81,
    ];
    let sb = 4096;
    if b.get(sb + 24..sb + 40)? != MAGIC {
        return None;
    }
    let ver = le16(b, sb + 16)?;
    let vmin = le16(b, sb + 18)?;
    let label: Vec<u8> = b
        .get(sb + 72..sb + 104)?
        .iter()
        .copied()
        .take_while(|&c| c != 0)
        .collect();
    let dev = *b.get(sb + 122)?;
    let nr = *b.get(sb + 123)?;
    let mut d = format!(
        "bcachefs, UUID={}, label \"{}\", version {}, min version {}, device {}",
        hex_uuid(b.get(sb + 56..sb + 72)?),
        String::from_utf8_lossy(&label),
        ver,
        vmin,
        dev
    );
    // Fields follow the fixed part and the layout (sb + 0x2f0).
    let mut p = sb + 0x2f0;
    for _ in 0..16 {
        let (Some(u64s), Some(ty)) = (le32(b, p), le32(b, p + 4)) else {
            break;
        };
        if u64s == 0 {
            break;
        }
        if ty == 1 {
            if let Some(u) = b.get(p + 8..p + 8 + 16) {
                d.push_str(&format!("/UUID={}", hex_uuid(u)));
            }
            break;
        }
        p += 8 * u64s as usize;
    }
    d.push_str(&format!(", {} devices", nr));
    Some(found(d, None, None, None))
}

// --------------------------------------------------------------- DSDIFF

/// DSDIFF (Philips "DSDIFF File Format Specification" 1.5): FRM8 form of
/// type "DSD ", the PROP/SND chunk's FS, CHNL and CMPR chunks, and an
/// ID3 chunk.
fn dsdiff(b: &[u8]) -> Option<Found> {
    if !b.starts_with(b"FRM8") || b.get(12..16)? != b"DSD " {
        return None;
    }
    let mut d = String::from("DSDIFF audio bitstream data, 1 bit");
    let chunk = |p: usize| -> Option<(&[u8], usize, usize)> {
        let id = b.get(p..p + 4)?;
        let sz = u64::from_be_bytes(b.get(p + 4..p + 12)?.try_into().ok()?) as usize;
        Some((id, p + 12, sz))
    };
    let mut p = 16;
    let mut id3: Option<usize> = None;
    let mut guard = 0;
    while let Some((id, st, sz)) = chunk(p) {
        guard += 1;
        if guard > 64 {
            break;
        }
        if id == b"PROP" && b.get(st..st + 4) == Some(b"SND ") {
            let mut q = st + 4;
            let end = st + sz;
            let (mut rate, mut chans, mut comp) = (None, None, None);
            while q < end {
                let Some((cid, cst, csz)) = chunk(q) else {
                    break;
                };
                match cid {
                    b"FS  " => rate = be32(b, cst),
                    b"CHNL" => chans = be16(b, cst),
                    b"CMPR" => comp = b.get(cst..cst + 4),
                    _ => {}
                }
                q = cst + csz + (csz & 1);
            }
            match chans {
                Some(1) => d.push_str(", mono"),
                Some(2) => d.push_str(", stereo"),
                Some(n) => d.push_str(&format!(", {} channels", n)),
                None => {}
            }
            if let Some(r) = rate {
                let name = match r {
                    2822400 => Some("DSD 64"),
                    5644800 => Some("DSD 128"),
                    11289600 => Some("DSD 256"),
                    22579200 => Some("DSD 512"),
                    _ => None,
                };
                match name {
                    Some(n) => d.push_str(&format!(", \"{}\" {} Hz", n, r)),
                    None => d.push_str(&format!(", {} Hz", r)),
                }
            }
            match comp {
                Some(b"DSD ") => d.push_str(", no compression"),
                Some(b"DST ") => d.push_str(", DST compression"),
                _ => {}
            }
        }
        if id == b"ID3 " {
            id3 = Some(p);
        }
        p = st + sz + (sz & 1);
    }
    if let Some(i) = id3 {
        // The two bytes after the chunk ID (observed on PHP 8.5).
        if let (Some(&maj), Some(&min)) = (b.get(i + 4), b.get(i + 5)) {
            d.push_str(&format!(", ID3 version 2.{}.{}", maj, min));
        }
    }
    Some(found(d, Some("audio/x-dff"), None, None))
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
    [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 0,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ],
    // MPEG-2/2.5 layer I, layers II and III
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256, 0,
    ],
    [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ],
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
    let srs = if sr.is_multiple_of(1000) {
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
        if b[j..].starts_with(b"/Pages")
            && !b[j + 6..]
                .first()
                .is_some_and(|c| c.is_ascii_alphanumeric())
        {
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
    if cmap > 1
        || !matches!(ty, 1 | 2 | 3 | 9 | 10 | 11)
        || !matches!(depth, 1 | 8 | 15 | 16 | 24 | 32)
    {
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
