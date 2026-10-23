//! Image properties derived from profiles (EXIF tags as `exif:*`), the
//! JPEG quality estimate, and the TXT (pixel enumeration) format.

use crate::color::fmt_g;
use crate::image::Image;
use crate::types::{Colorspace, OrientationType};

/// Seconds since the epoch as a UTC civil date and time.
pub fn civil_utc(secs: i64) -> (i64, u8, u8, u8, u8, u8) {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // days to civil (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    (
        y,
        m as u8,
        d as u8,
        (rem / 3600) as u8,
        (rem / 60 % 60) as u8,
        (rem % 60) as u8,
    )
}

/// Now, in seconds since the epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A date property value, as ImageMagick writes them ("2026-10-23T17:21:01+00:00").
pub fn iso_date(secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = civil_utc(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}+00:00")
}

/// The date:* properties ImageMagick gives what it reads: the file's
/// change and modification times (kept when the file named its own),
/// and the time of reading.
pub fn set_read_dates(img: &mut Image, file: Option<&std::fs::Metadata>) {
    let now = now_secs();
    let (create, modify) = match file {
        Some(md) => {
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(now, |d| d.as_secs() as i64);
            #[cfg(unix)]
            let ctime = {
                use std::os::unix::fs::MetadataExt;
                md.ctime()
            };
            #[cfg(not(unix))]
            let ctime = mtime;
            (ctime, mtime)
        }
        None => (now, now),
    };
    if img.properties.get("date:create").is_none() {
        img.properties.set("date:create", &iso_date(create));
    }
    if img.properties.get("date:modify").is_none() {
        img.properties.set("date:modify", &iso_date(modify));
    }
    img.properties.set("date:timestamp", &iso_date(now));
}

/// The chromaticity and gamma SetImageColorspace() leaves: linear and
/// gray spaces have no primaries (and linear ones a gamma of 1); sRGB
/// gets its own back.
pub fn colorspace_attributes(img: &mut Image, cs: Colorspace) {
    match cs {
        Colorspace::Gray => img.chromaticity = Default::default(),
        Colorspace::RGB
        | Colorspace::scRGB
        | Colorspace::XyY
        | Colorspace::XYZ
        | Colorspace::LinearGray => {
            img.chromaticity = Default::default();
            img.gamma = 1.0;
        }
        Colorspace::SRGB => {
            img.chromaticity = crate::image::Chromaticity::srgb();
            img.gamma = 1.0 / 2.2;
        }
        _ => {}
    }
}

/// EXIF tag names (EXIF 2.32 / TIFF 6.0).
fn exif_tag_name(tag: u16) -> Option<&'static str> {
    Some(match tag {
        0x00fe => "NewSubfileType",
        0x0100 => "ImageWidth",
        0x0101 => "ImageLength",
        0x0102 => "BitsPerSample",
        0x0103 => "Compression",
        0x0106 => "PhotometricInterpretation",
        0x010e => "ImageDescription",
        0x010f => "Make",
        0x0110 => "Model",
        0x0111 => "StripOffsets",
        0x0112 => "Orientation",
        0x0115 => "SamplesPerPixel",
        0x0116 => "RowsPerStrip",
        0x0117 => "StripByteCounts",
        0x011a => "XResolution",
        0x011b => "YResolution",
        0x011c => "PlanarConfiguration",
        0x0128 => "ResolutionUnit",
        0x012d => "TransferFunction",
        0x0131 => "Software",
        0x0132 => "DateTime",
        0x013b => "Artist",
        0x013e => "WhitePoint",
        0x013f => "PrimaryChromaticities",
        0x0201 => "JPEGInterchangeFormat",
        0x0202 => "JPEGInterchangeFormatLength",
        0x0211 => "YCbCrCoefficients",
        0x0212 => "YCbCrSubSampling",
        0x0213 => "YCbCrPositioning",
        0x0214 => "ReferenceBlackWhite",
        0x8298 => "Copyright",
        0x829a => "ExposureTime",
        0x829d => "FNumber",
        0x8822 => "ExposureProgram",
        0x8824 => "SpectralSensitivity",
        0x8827 => "PhotographicSensitivity",
        0x8830 => "SensitivityType",
        0x9000 => "ExifVersion",
        0x9003 => "DateTimeOriginal",
        0x9004 => "DateTimeDigitized",
        0x9010 => "OffsetTime",
        0x9011 => "OffsetTimeOriginal",
        0x9012 => "OffsetTimeDigitized",
        0x9101 => "ComponentsConfiguration",
        0x9102 => "CompressedBitsPerPixel",
        0x9201 => "ShutterSpeedValue",
        0x9202 => "ApertureValue",
        0x9203 => "BrightnessValue",
        0x9204 => "ExposureBiasValue",
        0x9205 => "MaxApertureValue",
        0x9206 => "SubjectDistance",
        0x9207 => "MeteringMode",
        0x9208 => "LightSource",
        0x9209 => "Flash",
        0x920a => "FocalLength",
        0x9214 => "SubjectArea",
        0x927c => "MakerNote",
        0x9286 => "UserComment",
        0x9290 => "SubSecTime",
        0x9291 => "SubSecTimeOriginal",
        0x9292 => "SubSecTimeDigitized",
        0xa000 => "FlashPixVersion",
        0xa001 => "ColorSpace",
        0xa002 => "PixelXDimension",
        0xa003 => "PixelYDimension",
        0xa004 => "RelatedSoundFile",
        0xa20b => "FlashEnergy",
        0xa20e => "FocalPlaneXResolution",
        0xa20f => "FocalPlaneYResolution",
        0xa210 => "FocalPlaneResolutionUnit",
        0xa214 => "SubjectLocation",
        0xa215 => "ExposureIndex",
        0xa217 => "SensingMethod",
        0xa300 => "FileSource",
        0xa301 => "SceneType",
        0xa302 => "CFAPattern",
        0xa401 => "CustomRendered",
        0xa402 => "ExposureMode",
        0xa403 => "WhiteBalance",
        0xa404 => "DigitalZoomRatio",
        0xa405 => "FocalLengthIn35mmFilm",
        0xa406 => "SceneCaptureType",
        0xa407 => "GainControl",
        0xa408 => "Contrast",
        0xa409 => "Saturation",
        0xa40a => "Sharpness",
        0xa40c => "SubjectDistanceRange",
        0xa420 => "ImageUniqueID",
        0xa430 => "CameraOwnerName",
        0xa431 => "BodySerialNumber",
        0xa432 => "LensSpecification",
        0xa433 => "LensMake",
        0xa434 => "LensModel",
        0xa435 => "LensSerialNumber",
        _ => return None,
    })
}

fn gps_tag_name(tag: u16) -> Option<&'static str> {
    Some(match tag {
        0x0000 => "GPSVersionID",
        0x0001 => "GPSLatitudeRef",
        0x0002 => "GPSLatitude",
        0x0003 => "GPSLongitudeRef",
        0x0004 => "GPSLongitude",
        0x0005 => "GPSAltitudeRef",
        0x0006 => "GPSAltitude",
        0x0007 => "GPSTimeStamp",
        0x0008 => "GPSSatellites",
        0x0009 => "GPSStatus",
        0x000a => "GPSMeasureMode",
        0x000b => "GPSDOP",
        0x000c => "GPSSpeedRef",
        0x000d => "GPSSpeed",
        0x0010 => "GPSImgDirectionRef",
        0x0011 => "GPSImgDirection",
        0x0012 => "GPSMapDatum",
        0x001d => "GPSDateStamp",
        _ => return None,
    })
}

struct Tiff<'a> {
    d: &'a [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, o: usize) -> Option<u16> {
        let b = self.d.get(o..o + 2)?;
        Some(if self.le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let b = self.d.get(o..o + 4)?;
        Some(if self.le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    }
}

/// Parse an EXIF block (with or without the "Exif\0\0" header) into
/// (tag name, value) pairs.
pub fn parse_exif(data: &[u8]) -> Vec<(String, String)> {
    let d = data.strip_prefix(b"Exif\0\0").unwrap_or(data);
    let le = match d.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Vec::new(),
    };
    let t = Tiff { d, le };
    let mut out = Vec::new();
    let Some(ifd0) = t.u32(4) else {
        return out;
    };
    let mut seen = Vec::new();
    walk_ifd(&t, ifd0 as usize, false, &mut out, &mut seen, 0);
    out
}

fn walk_ifd(
    t: &Tiff,
    off: usize,
    gps: bool,
    out: &mut Vec<(String, String)>,
    seen: &mut Vec<usize>,
    depth: u32,
) {
    if depth > 4 || seen.contains(&off) {
        return;
    }
    seen.push(off);
    let Some(n) = t.u16(off) else {
        return;
    };
    for i in 0..n as usize {
        let e = off + 2 + i * 12;
        let (Some(tag), Some(ty), Some(count)) = (t.u16(e), t.u16(e + 2), t.u32(e + 4)) else {
            return;
        };
        let size = match ty {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => continue,
        };
        let total = size * count as usize;
        if total > 1 << 20 {
            continue;
        }
        let voff = if total <= 4 {
            e + 8
        } else {
            match t.u32(e + 8) {
                Some(v) => v as usize,
                None => continue,
            }
        };
        if voff + total > t.d.len() {
            continue;
        }
        if !gps && (tag == 0x8769 || tag == 0xa005) {
            if let Some(sub) = t.u32(voff) {
                if tag == 0x8769 {
                    walk_ifd(t, sub as usize, false, out, seen, depth + 1);
                }
            }
            continue;
        }
        if !gps && tag == 0x8825 {
            if let Some(sub) = t.u32(voff) {
                walk_ifd(t, sub as usize, true, out, seen, depth + 1);
            }
            continue;
        }
        let name = if gps {
            gps_tag_name(tag)
        } else {
            exif_tag_name(tag)
        };
        let name = match name {
            Some(n) => n.to_string(),
            None => format!("0x{:04x}", tag),
        };
        let vals: Vec<String> = match ty {
            2 => {
                let s = &t.d[voff..voff + total];
                let s = match s.iter().position(|&c| c == 0) {
                    Some(p) => &s[..p],
                    None => s,
                };
                vec![String::from_utf8_lossy(s).trim_end().to_string()]
            }
            1 | 7 => {
                let s = &t.d[voff..voff + total];
                if ty == 7 && (tag == 0x9000 || tag == 0xa000) {
                    vec![String::from_utf8_lossy(s).to_string()]
                } else if ty == 7 && total > 16 {
                    vec![
                        String::from_utf8_lossy(s)
                            .trim_end_matches('\0')
                            .to_string(),
                    ]
                } else {
                    s.iter().map(|b| b.to_string()).collect()
                }
            }
            6 => t.d[voff..voff + total]
                .iter()
                .map(|&b| (b as i8).to_string())
                .collect(),
            3 => (0..count as usize)
                .filter_map(|i| t.u16(voff + i * 2).map(|v| v.to_string()))
                .collect(),
            8 => (0..count as usize)
                .filter_map(|i| t.u16(voff + i * 2).map(|v| (v as i16).to_string()))
                .collect(),
            4 => (0..count as usize)
                .filter_map(|i| t.u32(voff + i * 4).map(|v| v.to_string()))
                .collect(),
            9 => (0..count as usize)
                .filter_map(|i| t.u32(voff + i * 4).map(|v| (v as i32).to_string()))
                .collect(),
            5 => (0..count as usize)
                .filter_map(|i| {
                    Some(format!(
                        "{}/{}",
                        t.u32(voff + i * 8)?,
                        t.u32(voff + i * 8 + 4)?
                    ))
                })
                .collect(),
            10 => (0..count as usize)
                .filter_map(|i| {
                    Some(format!(
                        "{}/{}",
                        t.u32(voff + i * 8)? as i32,
                        t.u32(voff + i * 8 + 4)? as i32
                    ))
                })
                .collect(),
            11 => (0..count as usize)
                .filter_map(|i| t.u32(voff + i * 4).map(|v| fmt_g(f32::from_bits(v) as f64)))
                .collect(),
            _ => Vec::new(),
        };
        if tag == 0x927c {
            continue;
        }
        out.push((name, vals.join(", ")));
    }
}

/// Derive properties from the image's profiles (as ImageMagick does when
/// it reads an image): the EXIF tags, the orientation.
pub fn parse_profiles(img: &mut Image) {
    if let Some(exif) = img.profile("exif").map(|e| e.to_vec()) {
        for (k, v) in parse_exif(&exif) {
            if k == "Orientation" {
                if let Ok(o) = v.parse::<i32>() {
                    if (1..=8).contains(&o) {
                        img.orientation = OrientationType(o);
                    }
                }
            }
            let key = format!("exif:{}", k);
            if img.properties.get(&key).is_none() {
                img.properties.set(&key, &v);
            }
        }
    }
}

/// The IJG reference luminance quantisation table (ITU-T T.81 K.1), in
/// natural order.
const STD_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Estimate the quality a JPEG was saved with, by inverting the IJG
/// quality scaling of the standard luminance table.
pub fn estimate_jpeg_quality(data: &[u8]) -> Option<u32> {
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xff {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if marker == 0xda {
            return None;
        }
        if marker == 0xdb {
            let seg = data.get(i + 4..i + 2 + len)?;
            let pq = seg[0] >> 4;
            let tq = seg[0] & 15;
            if tq == 0 {
                let mut sum = 0f64;
                for k in 0..64 {
                    let v = if pq == 0 {
                        *seg.get(1 + k)? as f64
                    } else {
                        u16::from_be_bytes([*seg.get(1 + 2 * k)?, *seg.get(2 + 2 * k)?]) as f64
                    };
                    sum += v * 100.0 / STD_LUMA[ZIGZAG[k]] as f64;
                }
                let s = sum / 64.0;
                let q = if s <= 0.0 {
                    100.0
                } else if s <= 100.0 {
                    (200.0 - s) / 2.0
                } else {
                    5000.0 / s
                };
                return Some(q.round().clamp(1.0, 100.0) as u32);
            }
        }
        i += 2 + len;
    }
    None
}

/// The TXT format: "# ImageMagick pixel enumeration" and one line per pixel.
pub fn encode_txt(img: &Image) -> Vec<u8> {
    let cs = if img.colorspace == Colorspace::Gray {
        "gray"
    } else if img.colorspace == Colorspace::CMYK {
        "cmyk"
    } else {
        "srgb"
    };
    let cs = if img.alpha {
        format!("{}a", cs)
    } else {
        cs.to_string()
    };
    let mut s = format!(
        "# ImageMagick pixel enumeration: {},{},0,65535,{}\n",
        img.columns, img.rows, cs
    );
    for y in 0..img.rows {
        for x in 0..img.columns {
            let p = img.px(x, y);
            let c = img.pixel_info(x, y);
            let vals: Vec<String> = if img.colorspace == Colorspace::Gray {
                let mut v = vec![p[0].to_string()];
                if img.alpha {
                    v.push(p[3].to_string());
                }
                v
            } else {
                let mut v = vec![p[0].to_string(), p[1].to_string(), p[2].to_string()];
                if img.alpha {
                    v.push(p[3].to_string());
                }
                v
            };
            let hex: String = if img.alpha {
                format!("#{:04X}{:04X}{:04X}{:04X}", p[0], p[1], p[2], p[3])
            } else {
                format!("#{:04X}{:04X}{:04X}", p[0], p[1], p[2])
            };
            s.push_str(&format!(
                "{},{}: ({})  {}  {}\n",
                x,
                y,
                vals.join(","),
                hex,
                c.to_color_string()
            ));
        }
    }
    s.into_bytes()
}
