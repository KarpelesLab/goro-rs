//! `exif_read_data()`, `exif_thumbnail()`: Exif metadata of JPEG (APP1
//! "Exif" segments), TIFF and HEIF files.
//!
//! The structures come from the TIFF 6.0 and Exif 2.3 (CIPA DC-008)
//! specifications (byte order header, IFDs of 12-byte entries, the Exif,
//! GPS and Interoperability IFD pointers, IFD1 holding the thumbnail),
//! ITU T.81 for the JPEG markers and ISO/IEC 23008-12 for HEIF. The shape
//! of the result (sections, COMPUTED values, value conversions) and the
//! diagnostics follow the expected output of php-src's ext/exif tests and
//! observations of PHP 8.5 on crafted files.

use crate::Diag;
use crate::imageinfo::{
    self, IMAGETYPE_HEIF, IMAGETYPE_JPEG, IMAGETYPE_TIFF_II, IMAGETYPE_TIFF_MM,
};
use crate::makernote;
use crate::tags::{self, Table};

/// A PHP value of the result.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Int(i64),
    Float(f64),
    Str(Vec<u8>),
    /// A list (keys 0..n).
    List(Vec<Node>),
    /// An array with string keys, in order.
    Map(Vec<(Vec<u8>, Node)>),
}

impl Node {
    fn s(v: impl Into<Vec<u8>>) -> Node {
        Node::Str(v.into())
    }
}

/// Inserts like a PHP array assignment: an existing key keeps its place.
fn map_set(m: &mut Vec<(Vec<u8>, Node)>, k: Vec<u8>, v: Node) {
    if let Some(e) = m.iter_mut().find(|e| e.0 == k) {
        e.1 = v;
    } else {
        m.push((k, v));
    }
}

/// Character set conversion for UNICODE/JIS comments and the Windows XP
/// tags (the engine provides it through mbstring's converters).
pub trait Charset {
    /// `data` from encoding `from` to `to`; None when either is unknown.
    fn convert(&self, data: &[u8], to: &[u8], from: &[u8]) -> Option<Vec<u8>>;
}

/// The exif.* ini settings (PHP manual: Exif runtime configuration).
#[derive(Debug, Clone)]
pub struct Ini {
    pub encode_unicode: Vec<u8>,
    pub decode_unicode_motorola: Vec<u8>,
    pub decode_unicode_intel: Vec<u8>,
    pub encode_jis: Vec<u8>,
    pub decode_jis_motorola: Vec<u8>,
    pub decode_jis_intel: Vec<u8>,
}

impl Default for Ini {
    fn default() -> Ini {
        Ini {
            encode_unicode: b"ISO-8859-15".to_vec(),
            decode_unicode_motorola: b"UCS-2BE".to_vec(),
            decode_unicode_intel: b"UCS-2LE".to_vec(),
            encode_jis: Vec::new(),
            decode_jis_motorola: b"JIS".to_vec(),
            decode_jis_intel: b"JIS".to_vec(),
        }
    }
}

/// The file being read.
#[derive(Debug, Clone, Default)]
pub struct FileMeta {
    /// Base name ("FileName"); None for streams without a path.
    pub name: Option<Vec<u8>>,
    /// Modification time ("FileDateTime"), 0 when unknown.
    pub mtime: i64,
}

// Sections, in the order PHP lists them (observed in "SectionsFound").
pub const S_FILE: usize = 0;
pub const S_COMPUTED: usize = 1;
pub const S_ANY_TAG: usize = 2;
pub const S_IFD0: usize = 3;
pub const S_THUMBNAIL: usize = 4;
pub const S_COMMENT: usize = 5;
pub const S_APP0: usize = 6;
pub const S_EXIF: usize = 7;
pub const S_FPIX: usize = 8;
pub const S_GPS: usize = 9;
pub const S_INTEROP: usize = 10;
pub const S_APP12: usize = 11;
pub const S_WINXP: usize = 12;
pub const S_MAKERNOTE: usize = 13;
const NSECT: usize = 14;

pub const SECTION_NAMES: [&str; NSECT] = [
    "FILE",
    "COMPUTED",
    "ANY_TAG",
    "IFD0",
    "THUMBNAIL",
    "COMMENT",
    "APP0",
    "EXIF",
    "FPIX",
    "GPS",
    "INTEROP",
    "APP12",
    "WINXP",
    "MAKERNOTE",
];

/// Byte sizes of the TIFF field types 1..12 (TIFF 6.0 section 2).
const FMT_SIZE: [usize; 13] = [0, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 4, 8];

const TAG_IMAGEWIDTH: u16 = 0x0100;
const TAG_IMAGEHEIGHT: u16 = 0x0101;
const TAG_PHOTOMETRIC: u16 = 0x0106;
const TAG_MAKE: u16 = 0x010F;
const TAG_MODEL: u16 = 0x0110;
const TAG_STRIP_OFFSETS: u16 = 0x0111;
const TAG_STRIP_BYTE_COUNTS: u16 = 0x0117;
const TAG_SUB_IFD: u16 = 0x014A;
const TAG_JPEG_IF_OFFSET: u16 = 0x0201;
const TAG_JPEG_IF_LENGTH: u16 = 0x0202;
const TAG_COPYRIGHT: u16 = 0x8298;
const TAG_EXPOSURETIME: u16 = 0x829A;
const TAG_FNUMBER: u16 = 0x829D;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_GPS_IFD: u16 = 0x8825;
const TAG_SHUTTERSPEED: u16 = 0x9201;
const TAG_APERTURE: u16 = 0x9202;
const TAG_MAXAPERTURE: u16 = 0x9205;
const TAG_SUBJECT_DISTANCE: u16 = 0x9206;
const TAG_MAKER_NOTE: u16 = 0x927C;
const TAG_USERCOMMENT: u16 = 0x9286;
const TAG_XP_TITLE: u16 = 0x9C9B;
const TAG_XP_SUBJECT: u16 = 0x9C9F;
const TAG_EXIF_IMAGEWIDTH: u16 = 0xA002;
const TAG_INTEROP_IFD: u16 = 0xA005;
const TAG_FOCALPLANE_X_RES: u16 = 0xA20E;
const TAG_FOCALPLANE_RES_UNIT: u16 = 0xA210;
const TAG_FOCALPLANE_X_RES_OLD: u16 = 0x920E;
const TAG_FOCALPLANE_RES_UNIT_OLD: u16 = 0x9210;

/// The most tags one file may hold (observed on PHP 8.5: 1000, then
/// "corrupt EXIF header: maximum IFD tag count reached").
const MAX_TAGS: usize = 1000;
/// The deepest IFD nesting followed.
const MAX_NESTING: usize = 150;
/// Diagnostics shown before "Further exif parsing errors have been
/// suppressed" (observed on PHP 8.5: ten).
const MAX_ERRORS: usize = 10;

#[derive(Default, Clone)]
struct Thumb {
    filetype: i64,
    offset: u32,
    size: u32,
    offset_set: bool,
    size_set: bool,
    data: Option<Vec<u8>>,
    width: u32,
    height: u32,
}

/// The state of one read.
struct Info<'a> {
    ini: &'a Ini,
    cs: &'a dyn Charset,
    file: &'a [u8],
    file_size: u32,
    file_type: i64,
    motorola: bool,
    /// Whether a TIFF header was found (ByteOrderMotorola is reported).
    have_order: bool,
    /// TIFF file (as opposed to Exif inside JPEG or HEIF).
    tiff_mode: bool,
    read_thumbnail: bool,
    width: u32,
    height: u32,
    is_color: bool,
    found: u32,
    sections: Vec<Vec<(Vec<u8>, Node)>>,
    comments: Vec<Node>,
    thumb: Thumb,
    user_comment: Option<(Option<Vec<u8>>, Option<&'static str>)>,
    copyright: Option<Vec<u8>>,
    copyright_photographer: Option<Vec<u8>>,
    copyright_editor: Option<Vec<u8>>,
    aperture_fnumber: f64,
    aperture_value: f64,
    max_aperture: f64,
    exposure_time: Option<f64>,
    shutter_speed: Option<f64>,
    distance: f64,
    exif_image_width: u32,
    focal_plane_x_res: f64,
    focal_plane_unit: f64,
    ccd_width: f64,
    make: Option<Vec<u8>>,
    model: Option<Vec<u8>>,
    diags: Vec<Diag>,
    errors: usize,
    suppressed: bool,
    tag_count: usize,
    tag_limit_warned: bool,
    nesting: usize,
}

/// Why the read stopped.
#[derive(Debug, Clone, Copy)]
struct Fail {
    /// Reported as "File structure corrupted" when it ends IFD0.
    hard: bool,
}

/// A structural error: the file is corrupt.
const HARD: Fail = Fail { hard: true };
/// A bad value: the IFDs being read are abandoned, the file is not.
const SOFT: Fail = Fail { hard: false };

type Res<T> = Result<T, Fail>;

impl<'a> Info<'a> {
    fn new(file: &'a [u8], ini: &'a Ini, cs: &'a dyn Charset) -> Info<'a> {
        Info {
            ini,
            cs,
            file,
            file_size: file.len().min(u32::MAX as usize) as u32,
            file_type: 0,
            motorola: false,
            have_order: false,
            tiff_mode: false,
            read_thumbnail: false,
            width: 0,
            height: 0,
            is_color: false,
            found: 0,
            sections: vec![Vec::new(); NSECT],
            comments: Vec::new(),
            thumb: Thumb::default(),
            user_comment: None,
            copyright: None,
            copyright_photographer: None,
            copyright_editor: None,
            aperture_fnumber: 0.0,
            aperture_value: 0.0,
            max_aperture: 0.0,
            exposure_time: None,
            shutter_speed: None,
            distance: 0.0,
            exif_image_width: 0,
            focal_plane_x_res: 0.0,
            focal_plane_unit: 0.0,
            ccd_width: 0.0,
            make: None,
            model: None,
            diags: Vec::new(),
            errors: 0,
            suppressed: false,
            tag_count: 0,
            tag_limit_warned: false,
            nesting: 0,
        }
    }

    // ------------------------------------------------------------ errors

    fn report(&mut self, notice: bool, msg: String) {
        if self.suppressed {
            return;
        }
        self.errors += 1;
        if self.errors > MAX_ERRORS {
            self.suppressed = true;
            let m = "Further exif parsing errors have been suppressed".to_string();
            self.diags.push(if notice {
                Diag::NoticeBare(m)
            } else {
                Diag::WarningBare(m)
            });
            return;
        }
        self.diags.push(if notice {
            Diag::Notice(msg)
        } else {
            Diag::Warning(msg)
        });
    }
    fn warn(&mut self, msg: impl Into<String>) {
        self.report(false, msg.into());
    }
    fn notice(&mut self, msg: impl Into<String>) {
        self.report(true, msg.into());
    }

    // ------------------------------------------------------------ bytes

    fn get16(&self, b: &[u8]) -> u16 {
        if self.motorola {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        }
    }
    fn get32(&self, b: &[u8]) -> u32 {
        if self.motorola {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        }
    }
    /// `len` bytes at absolute offset `at`, None past the end.
    fn bytes(&self, at: usize, len: usize) -> Option<&'a [u8]> {
        let end = at.checked_add(len)?;
        self.file.get(at..end)
    }

    // ------------------------------------------------------------ tags

    fn add_section_tag(&mut self, section: usize, key: Vec<u8>, v: Node) {
        self.found |= 1 << section;
        self.found |= 1 << S_ANY_TAG;
        map_set(&mut self.sections[section], key, v);
    }

    /// A value as PHP converts it (observed on PHP 8.5): bytes and
    /// undefined data as strings, ASCII up to its first NUL, LONG read as
    /// signed, rationals as "n/d", FLOAT and DOUBLE read in the machine's
    /// (little-endian) order with the first value repeated.
    fn convert(&self, fmt: u16, count: u32, v: &[u8]) -> Node {
        if count == 0 {
            return Node::Null;
        }
        match fmt {
            1 | 6 | 7 => Node::Str(v.to_vec()),
            2 => {
                let n = v.iter().position(|&c| c == 0).unwrap_or(v.len());
                if n == 0 {
                    Node::Null
                } else {
                    Node::Str(v[..n].to_vec())
                }
            }
            _ => {
                let size = FMT_SIZE[fmt as usize];
                let n = (v.len() / size).min(count as usize);
                let one = |i: usize| -> Node {
                    let b = &v[i * size..];
                    match fmt {
                        3 => Node::Int(self.get16(b) as i64),
                        8 => Node::Int(self.get16(b) as i16 as i64),
                        4 | 9 => Node::Int(self.get32(b) as i32 as i64),
                        5 => Node::s(format!("{}/{}", self.get32(b), self.get32(&b[4..]))),
                        10 => Node::s(format!(
                            "{}/{}",
                            self.get32(b) as i32,
                            self.get32(&b[4..]) as i32
                        )),
                        11 => Node::Float(f32::from_le_bytes([v[0], v[1], v[2], v[3]]) as f64),
                        12 => Node::Float(f64::from_le_bytes(v[..8].try_into().unwrap())),
                        _ => Node::Null,
                    }
                };
                if n == 0 {
                    Node::Null
                } else if count == 1 {
                    one(0)
                } else {
                    Node::List((0..n).map(one).collect())
                }
            }
        }
    }

    /// The numeric value of a tag (first component) as a double.
    fn number(&self, fmt: u16, v: &[u8]) -> f64 {
        if v.len() < FMT_SIZE[fmt as usize].max(1) {
            return 0.0;
        }
        match fmt {
            1 => v[0] as f64,
            6 => v[0] as i8 as f64,
            3 => self.get16(v) as f64,
            8 => self.get16(v) as i16 as f64,
            4 => self.get32(v) as f64,
            9 => self.get32(v) as i32 as f64,
            // Rationals, signed ones too, are read unsigned into a single
            // precision float (observed on PHP 8.5: 0xFFFFFFFF/1 gives
            // "4294967296.00m").
            5 | 10 => {
                let d = self.get32(&v[4..]);
                if d == 0 {
                    0.0
                } else {
                    (self.get32(v) as f64 / d as f64) as f32 as f64
                }
            }
            11 => f32::from_le_bytes([v[0], v[1], v[2], v[3]]) as f64,
            12 => f64::from_le_bytes(v[..8].try_into().unwrap()),
            _ => 0.0,
        }
    }

    fn uint(&self, fmt: u16, v: &[u8]) -> u32 {
        if v.len() < FMT_SIZE[fmt as usize].max(1) {
            return 0;
        }
        match fmt {
            1 | 6 | 2 | 7 => v[0] as u32,
            3 | 8 => self.get16(v) as u32,
            4 | 9 => self.get32(v),
            5 | 10 => {
                let d = self.get32(&v[4..]);
                self.get32(v).checked_div(d).unwrap_or(0)
            }
            11 | 12 => {
                // Out of range floats count as 0 (test float_cast_overflow.phpt).
                let f = self.number(fmt, v);
                if f.is_nan() || !(0.0..4294967296.0).contains(&f) {
                    0
                } else {
                    f as u32
                }
            }
            _ => 0,
        }
    }

    /// One IFD entry at absolute offset `e`. `base` is where offsets
    /// count from; `limit` bounds them (the file size, observed on PHP
    /// 8.5: "Illegal pointer offset(... > x<FileSize>)").
    #[allow(clippy::too_many_arguments)]
    fn process_tag(&mut self, e: usize, base: usize, section: usize, table: Table) -> Res<()> {
        let Some(ent) = self.bytes(e, 12) else {
            return Err(HARD);
        };
        let tag = self.get16(ent);
        let mut fmt = self.get16(&ent[2..]);
        let count = self.get32(&ent[4..]);
        if fmt == 0 || fmt > 12 {
            self.warn(format!(
                "Process tag(x{tag:04X}={}): Illegal format code 0x{fmt:04X}, suppose BYTE",
                tags::diag_name(table, tag)
            ));
            fmt = 1;
            // Past the error limit IFD0 is given up, sub IFDs go on
            // (observed on PHP 8.5, tests bug76557.phpt, bug71534.phpt).
            if self.suppressed && section == S_IFD0 {
                return Err(HARD);
            }
        }
        // TIFF files: the IFD pointers are followed from the entry's value
        // field whatever their format (observed on PHP 8.5, tests
        // duplicate_copyright_tag_leak.phpt, nesting_level_oom.phpt).
        if self.tiff_mode && matches!(table, Table::Ifd) {
            let sub = match tag {
                TAG_EXIF_IFD => Some((S_EXIF, Table::Ifd)),
                TAG_GPS_IFD => Some((S_GPS, Table::Gps)),
                TAG_INTEROP_IFD => Some((S_INTEROP, Table::Interop)),
                TAG_SUB_IFD => Some((S_THUMBNAIL, Table::Ifd)),
                _ => None,
            };
            if let Some((sec, tab)) = sub {
                let off = self.get32(&ent[8..]);
                if off as u64 + 2 > self.file_size as u64 {
                    self.warn(format!(
                        "Error in TIFF: filesize(x{:04X}) less than start of IFD dir(x{:04X})",
                        self.file_size,
                        off as u64 + 2
                    ));
                    return Ok(());
                }
                self.found |= (1 << sec) | (1 << S_ANY_TAG);
                let _ = self.process_ifd(base + off as usize, base, sec, tab);
                return Ok(());
            }
        }
        let size = FMT_SIZE[fmt as usize] as u64;
        let byte_count = count as u64 * size;
        if byte_count > i32::MAX as u64 {
            self.warn(format!(
                "Process tag(x{tag:04X}={}): Illegal byte_count",
                tags::diag_name(table, tag)
            ));
            return Err(SOFT);
        }
        let byte_count = byte_count as usize;
        let value: &'a [u8] = if byte_count > 4 {
            let off = self.get32(&ent[8..]);
            let sum = off.wrapping_add(byte_count as u32);
            if (sum as u64) < off as u64 || sum > self.file_size {
                self.warn(format!(
                    "Process tag(x{tag:04X}={}): Illegal pointer offset(x{off:04X} + x{byte_count:04X} = x{sum:04X} > x{:04X})",
                    tags::diag_name(table, tag),
                    self.file_size
                ));
                return Err(SOFT);
            }
            match self.bytes(base + off as usize, byte_count) {
                Some(v) => v,
                None => {
                    self.warn("Unexpected end of file reached");
                    return Err(SOFT);
                }
            }
        } else {
            &ent[8..8 + byte_count]
        };

        if self.tag_count >= MAX_TAGS {
            if !self.tag_limit_warned {
                self.tag_limit_warned = true;
                self.warn("corrupt EXIF header: maximum IFD tag count reached");
            }
            return Err(SOFT);
        }
        self.tag_count += 1;

        // TIFF image size: LONG or else a 16-bit value, in any IFD
        // (observed on PHP 8.5, tests bug72627.phpt, float_cast_overflow.phpt).
        if self.tiff_mode && count > 0 && matches!(table, Table::Ifd) {
            let v = || {
                let raw = &ent[8..12];
                if fmt == 4 {
                    self.get32(raw)
                } else {
                    self.get16(raw) as u32
                }
            };
            if tag == TAG_IMAGEWIDTH && self.width == 0 {
                self.width = v();
            } else if tag == TAG_IMAGEHEIGHT && self.height == 0 {
                self.height = v();
            }
        }
        // Thumbnail location (IFD1).
        if section == S_THUMBNAIL {
            match tag {
                TAG_IMAGEWIDTH | TAG_IMAGEHEIGHT if count == 0 => {
                    self.empty_tag(tag, table);
                    return Err(HARD);
                }
                // In TIFF files only; a JPEG file's thumbnail size comes from
                // its SOF (observed on PHP 8.5, test bug77540.phpt).
                TAG_IMAGEWIDTH if self.tiff_mode || matches!(fmt, 3 | 4) => {
                    self.thumb.width = self.uint(fmt, value)
                }
                TAG_IMAGEHEIGHT if self.tiff_mode || matches!(fmt, 3 | 4) => {
                    self.thumb.height = self.uint(fmt, value)
                }
                TAG_STRIP_OFFSETS | TAG_JPEG_IF_OFFSET => {
                    if count == 0 {
                        self.empty_tag(tag, table);
                        return Err(HARD);
                    }
                    self.thumb.offset = self.uint(fmt, value);
                    self.thumb.offset_set = true;
                }
                TAG_STRIP_BYTE_COUNTS | TAG_JPEG_IF_LENGTH => {
                    if count == 0 {
                        self.empty_tag(tag, table);
                        return Err(HARD);
                    }
                    if tag == TAG_STRIP_BYTE_COUNTS && self.thumb.filetype == 0 {
                        self.thumb.filetype = if self.motorola {
                            IMAGETYPE_TIFF_MM
                        } else {
                            IMAGETYPE_TIFF_II
                        };
                    } else if tag == TAG_JPEG_IF_LENGTH {
                        self.thumb.filetype = IMAGETYPE_JPEG;
                    }
                    self.thumb.size = self.uint(fmt, value);
                    self.thumb.size_set = true;
                }
                _ => {}
            }
        }

        if matches!(table, Table::Ifd) && section != S_THUMBNAIL {
            match tag {
                TAG_IMAGEWIDTH | TAG_IMAGEHEIGHT | TAG_JPEG_IF_LENGTH | TAG_JPEG_IF_OFFSET
                    if count == 0 =>
                {
                    self.empty_tag(tag, table);
                    return Err(HARD);
                }
                TAG_PHOTOMETRIC if self.tiff_mode => {
                    self.is_color = self.uint(fmt, value) >= 2;
                }
                TAG_COPYRIGHT => self.copyright_tag(value),
                TAG_USERCOMMENT => {
                    let uc = self.user_comment_value(value);
                    let empty_jis = uc.1 == Some("JIS") && uc.0.as_deref() == Some(b"\0");
                    self.user_comment = if empty_jis { None } else { Some(uc) };
                }
                TAG_FNUMBER => self.aperture_fnumber = self.number(fmt, value),
                TAG_APERTURE => self.aperture_value = self.number(fmt, value),
                TAG_MAXAPERTURE => self.max_aperture = self.number(fmt, value),
                TAG_EXPOSURETIME => self.exposure_time = Some(self.number(fmt, value)),
                TAG_SHUTTERSPEED => self.shutter_speed = Some(self.number(fmt, value)),
                TAG_SUBJECT_DISTANCE => self.distance = self.number(fmt, value),
                TAG_EXIF_IMAGEWIDTH => self.exif_image_width = self.uint(fmt, value),
                TAG_FOCALPLANE_X_RES | TAG_FOCALPLANE_X_RES_OLD => {
                    self.focal_plane_x_res = self.number(fmt, value)
                }
                TAG_FOCALPLANE_RES_UNIT | TAG_FOCALPLANE_RES_UNIT_OLD => {
                    // TIFF/EP ResolutionUnit: inch, cm, mm, um.
                    self.focal_plane_unit = match self.uint(fmt, value) {
                        1 | 2 => 25.4,
                        3 => 10.0,
                        4 => 1.0,
                        5 => 0.001,
                        _ => 0.0,
                    };
                }
                TAG_MAKE => self.make = Some(cstr(value)),
                TAG_MODEL => self.model = Some(cstr(value)),
                _ => {}
            }
        }

        let name = tags::display_name(table, tag).into_bytes();
        let node = if tag == TAG_MAKER_NOTE && matches!(table, Table::Ifd) {
            let n = value.iter().position(|&c| c == 0).unwrap_or(value.len());
            if n == 0 {
                Node::Null
            } else {
                Node::Str(value[..n].to_vec())
            }
        } else {
            self.convert(fmt, count, value)
        };
        let is_pointer = matches!(table, Table::Ifd)
            && (matches!(tag, TAG_EXIF_IFD | TAG_GPS_IFD | TAG_INTEROP_IFD)
                || tag == TAG_SUB_IFD && self.tiff_mode);
        // TIFF files do not list the IFD pointers (observed on PHP 8.5).
        let deferred = is_pointer || tag == TAG_MAKER_NOTE && matches!(table, Table::Ifd);
        if !deferred {
            self.add_section_tag(section, name.clone(), node.clone());
        }
        if matches!(table, Table::Ifd)
            && section != S_THUMBNAIL
            && (TAG_XP_TITLE..=TAG_XP_SUBJECT).contains(&tag)
        {
            // Windows XP tags: also decoded in WINXP (observed on PHP 8.5).
            let v = self.xp_value(value);
            let name = tags::display_name(table, tag).into_bytes();
            self.add_section_tag(S_WINXP, name, Node::Str(v));
        }

        // Sub IFDs (Exif 2.3 section 4.6.3) and the maker note.
        if matches!(table, Table::Ifd) {
            let sub = match tag {
                TAG_EXIF_IFD => Some((S_EXIF, Table::Ifd)),
                TAG_GPS_IFD => Some((S_GPS, Table::Gps)),
                TAG_INTEROP_IFD => Some((S_INTEROP, Table::Interop)),
                TAG_SUB_IFD if self.tiff_mode => Some((S_THUMBNAIL, Table::Ifd)),
                _ => None,
            };
            if let Some((sec, tab)) = sub {
                // A 32-bit offset whatever the format (observed on PHP 8.5).
                let off = self.get32(if byte_count >= 4 { value } else { &ent[8..] });
                if off as u64 + 2 > self.file_size as u64 {
                    if self.tiff_mode {
                        self.warn(format!(
                            "Error in TIFF: filesize(x{:04X}) less than start of IFD dir(x{:04X})",
                            self.file_size,
                            off as u64 + 2
                        ));
                    } else {
                        self.warn("Illegal IFD Pointer");
                    }
                    return Err(SOFT);
                }
                self.found |= 1 << sec;
                self.found |= 1 << S_ANY_TAG;
                if self
                    .process_ifd(base + off as usize, base, sec, tab)
                    .is_err()
                    && !self.tiff_mode
                {
                    return Err(SOFT);
                }
            } else if tag == TAG_MAKER_NOTE && self.maker_note(value, base, byte_count).is_err() {
                return Err(SOFT);
            }
        }
        if is_pointer || tag == TAG_MAKER_NOTE && matches!(table, Table::Ifd) {
            // Added once the sub IFD was read (observed on PHP 8.5, test
            // bug54002.phpt: a failing maker note drops the tag).
            if !(is_pointer && self.tiff_mode) {
                self.add_section_tag(section, name, node);
            }
        }
        Ok(())
    }

    fn empty_tag(&mut self, tag: u16, table: Table) {
        self.warn(format!(
            "Process tag(x{tag:04X}={}): Cannot be empty",
            tags::diag_name(table, tag)
        ));
    }

    /// The Copyright tag: photographer and editor separated by NUL
    /// (Exif 2.3 section 4.6.5, Copyright).
    fn copyright_tag(&mut self, v: &[u8]) {
        // Observed on PHP 8.5: nothing when the photographer part is empty,
        // both parts when anything follows its NUL.
        let first = cstr(v);
        if first.is_empty() {
            return;
        }
        let rest = v.get(first.len() + 1..).unwrap_or(b"");
        let editor = cstr(rest);
        if !rest.is_empty() {
            let mut c = first.clone();
            c.extend_from_slice(b", ");
            c.extend_from_slice(&editor);
            self.copyright = Some(c);
            self.copyright_photographer = Some(first);
            self.copyright_editor = Some(editor);
        } else {
            self.copyright = Some(first);
            self.copyright_photographer = None;
            self.copyright_editor = None;
        }
    }

    /// UserComment: an 8-byte character code then the text (Exif 2.3
    /// section 4.6.5, table 9).
    fn user_comment_value(&self, v: &[u8]) -> (Option<Vec<u8>>, Option<&'static str>) {
        let non_empty = |b: Vec<u8>| if b.is_empty() { None } else { Some(b) };
        if v.len() >= 8 {
            let (code, text) = v.split_at(8);
            if code == b"UNICODE\0" {
                let from = if self.motorola {
                    &self.ini.decode_unicode_motorola
                } else {
                    &self.ini.decode_unicode_intel
                };
                let out = self
                    .cs
                    .convert(text, &self.ini.encode_unicode, from)
                    .unwrap_or_else(|| text.to_vec());
                return (non_empty(out), Some("UNICODE"));
            }
            if code == b"ASCII\0\0\0" {
                return (non_empty(cstr(text)), Some("ASCII"));
            }
            if code == b"JIS\0\0\0\0\0" {
                let from = if self.motorola {
                    &self.ini.decode_jis_motorola
                } else {
                    &self.ini.decode_jis_intel
                };
                let mut out = self
                    .cs
                    .convert(text, &self.ini.encode_jis, from)
                    .unwrap_or_else(|| text.to_vec());
                // The converted text keeps a terminating NUL (observed on
                // PHP 8.5; an empty one gives no UserComment at all).
                out.push(0);
                return (Some(out), Some("JIS"));
            }
            if code == b"\0\0\0\0\0\0\0\0" {
                // Trailing spaces are dropped (observed on PHP 8.5).
                let mut t = cstr(text);
                while t.last() == Some(&b' ') {
                    t.pop();
                }
                return (non_empty(t), Some("UNDEFINED"));
            }
        }
        (non_empty(cstr(v)), None)
    }

    /// The Windows XP tags: UCS-2 text in the file's byte order.
    fn xp_value(&self, v: &[u8]) -> Vec<u8> {
        let from = if self.motorola {
            &self.ini.decode_unicode_motorola
        } else {
            &self.ini.decode_unicode_intel
        };
        let mut out = self
            .cs
            .convert(v, &self.ini.encode_unicode, from)
            .unwrap_or_else(|| v.to_vec());
        while out.last() == Some(&0) {
            out.pop();
        }
        out
    }

    /// An IFD at absolute offset `dir`; offsets in it count from `base`.
    fn process_ifd(&mut self, dir: usize, base: usize, section: usize, table: Table) -> Res<()> {
        if self.nesting > MAX_NESTING {
            self.warn("corrupt EXIF header: maximum IFD tag nesting level reached");
            return Err(SOFT);
        }
        self.nesting += 1;
        let r = self.process_ifd_inner(dir, base, section, table);
        self.nesting -= 1;
        r
    }

    fn process_ifd_inner(
        &mut self,
        dir: usize,
        base: usize,
        section: usize,
        table: Table,
    ) -> Res<()> {
        let Some(nb) = self.bytes(dir, 2) else {
            if self.tiff_mode {
                self.warn(format!(
                    "Error in TIFF: filesize(x{:04X}) less than start of IFD dir(x{:04X})",
                    self.file_size,
                    dir as u64 + 2
                ));
            } else {
                self.warn("Illegal IFD size");
            }
            return Err(HARD);
        };
        let n = self.get16(nb) as usize;
        if self.bytes(dir + 2, n * 12).is_none() {
            if self.tiff_mode {
                self.warn(format!(
                    "Error in TIFF: filesize(x{:04X}) less than size of IFD(x{:04X} + x{:04X})",
                    self.file_size,
                    dir + 2,
                    n * 12
                ));
            } else {
                self.warn("Illegal IFD size");
            }
            return Err(HARD);
        }
        if self.tiff_mode {
            for i in 0..n {
                let e = self.bytes(dir + 2 + i * 12, 12).unwrap();
                let tag = self.get16(e);
                let fmt = self.get16(&e[2..]);
                if fmt > 12 {
                    self.notice(format!(
                        "Read from TIFF: tag(0x{tag:04X},{:>12}): Illegal format code 0x{fmt:04X}, switching to BYTE",
                        tags::diag_name(table, tag)
                    ));
                    if self.suppressed && section == S_IFD0 {
                        return Err(HARD);
                    }
                }
            }
        }
        for i in 0..n {
            self.process_tag(dir + 2 + i * 12, base, section, table)?;
        }
        if section == S_THUMBNAIL || section == S_MAKERNOTE {
            return Ok(());
        }
        let Some(nx) = self.bytes(dir + 2 + n * 12, 4) else {
            if self.tiff_mode {
                return Ok(());
            }
            // The next IFD offset is part of the directory (observed on PHP
            // 8.5, test bug72094.phpt).
            self.warn("Illegal IFD size");
            return Err(HARD);
        };
        let next = self.get32(nx);
        if next == 0 {
            return Ok(());
        }
        if self.tiff_mode {
            if next as u64 + 2 > self.file_size as u64 {
                self.warn(format!(
                    "Error in TIFF: filesize(x{:04X}) less than start of IFD dir(x{:04X})",
                    self.file_size,
                    next as u64 + 2
                ));
                return Ok(());
            }
            let _ = self.process_ifd(base + next as usize, base, S_THUMBNAIL, Table::Ifd);
            if self.read_thumbnail {
                self.thumbnail_check_tiff();
            }
            return Ok(());
        }
        if next as u64 >= self.file_size as u64 || self.bytes(base + next as usize, 2).is_none() {
            self.warn("Illegal IFD offset");
            return Err(HARD);
        }
        self.process_ifd(base + next as usize, base, S_THUMBNAIL, Table::Ifd)?;
        if self.thumb.filetype != 0 && self.thumb.size_set && self.thumb.offset_set {
            let end = self.thumb.offset as u64 + self.thumb.size as u64;
            if self.thumb.size == 0 || end > (self.file.len() - base) as u64 {
                if self.read_thumbnail {
                    self.warn("Thumbnail goes IFD boundary or end of file reached");
                }
                return Err(HARD);
            }
            if self.thumb.filetype != IMAGETYPE_JPEG {
                // A strip thumbnail inside a JPEG file is refused (observed
                // on PHP 8.5, test bug68113.phpt).
                return Err(HARD);
            }
            if self.read_thumbnail {
                let s = base + self.thumb.offset as usize;
                self.thumb.data = Some(self.file[s..s + self.thumb.size as usize].to_vec());
            }
        }
        Ok(())
    }

    fn thumbnail_check_tiff(&mut self) {
        if self.thumb.filetype == 0 || !self.thumb.offset_set || !self.thumb.size_set {
            return;
        }
        let end = self.thumb.offset as u64 + self.thumb.size as u64;
        if self.thumb.size == 0 || end > self.file_size as u64 {
            self.warn("Thumbnail goes IFD boundary or end of file reached");
            return;
        }
        let s = self.thumb.offset as usize;
        self.thumb.data = Some(self.file[s..s + self.thumb.size as usize].to_vec());
    }

    /// The maker note `value` (a slice of the file): an IFD in one of the
    /// layouts the makers use (see [`makernote`]).
    fn maker_note(&mut self, value: &'a [u8], base: usize, len: usize) -> Res<()> {
        let make = self.make.clone().unwrap_or_default();
        let Some(desc) = makernote::find(&make) else {
            return Ok(());
        };
        if !desc.id.is_empty() && value.len() >= desc.id.len() && !value.starts_with(desc.id) {
            // Another layout of a known maker: the section is seen, its
            // tags are not read (observed on PHP 8.5, test bug72682.phpt).
            self.found |= 1 << S_MAKERNOTE;
            return Ok(());
        }
        // Absolute position of the maker note data.
        let start = value.as_ptr() as usize - self.file.as_ptr() as usize;
        let off = desc.ifd_offset;
        if len < off + 2 {
            self.warn(format!(
                "IFD data too short: 0x{len:04X} offset 0x{off:04X}"
            ));
            return Ok(());
        }
        self.process_ifd(start + off, base, S_MAKERNOTE, Table::Maker(desc.maker))
    }

    /// A TIFF header at absolute offset `t` (Exif in JPEG or HEIF).
    fn process_tiff_in_jpeg(&mut self, t: usize, len: usize) -> Res<()> {
        // The header is read from the file, its length checked against the
        // segment (observed on PHP 8.5: tests bug72094.phpt, bug79282.phpt).
        let h = match self.bytes(t, 4) {
            Some(h) if len >= 2 => h,
            _ => {
                self.warn("Missing TIFF alignment marker");
                return Err(HARD);
            }
        };
        match &h[..2] {
            b"II" => self.motorola = false,
            b"MM" => self.motorola = true,
            _ => {
                self.warn("Invalid TIFF alignment marker");
                return Err(HARD);
            }
        }
        self.have_order = true;
        if self.get16(&h[2..]) != 0x2A {
            self.warn("Invalid TIFF start (1)");
            return Err(HARD);
        }
        let first = match self.bytes(t + 4, 4) {
            Some(o) => self.get32(o),
            None => u32::MAX,
        };
        if first as u64 > len as u64 {
            self.warn("Invalid IFD start");
            return Ok(());
        }
        self.found |= 1 << S_IFD0;
        match self.process_ifd(t + first as usize, t, S_IFD0, Table::Ifd) {
            Err(f) if f.hard => Err(f),
            _ => Ok(()),
        }
    }

    // ------------------------------------------------------------ JPEG

    /// The JPEG markers (ITU T.81 annex B); one segment is read at a time
    /// as PHP does, so a short segment ends the scan.
    fn scan_jpeg(&mut self) -> Res<()> {
        let f = self.file;
        let mut pos = 2usize;
        let mut last = 0u8;
        loop {
            let mut a = 0;
            let mut marker;
            if last == 0xFE && f.get(pos).is_some_and(|&c| c != 0xFF) {
                // PHP manual (exif_read_data) / test bug13213.phpt: a COM
                // segment with a wrong length is skipped to the next 0xFF.
                self.notice(
                    "Image has corrupt COM section: some software set wrong length information",
                );
                while f.get(pos).is_some_and(|&c| c != 0xFF) {
                    pos += 1;
                }
            }
            loop {
                let Some(&c) = f.get(pos) else {
                    self.warn("Unexpected end of file reached");
                    return Ok(());
                };
                pos += 1;
                marker = c;
                if c != 0xFF {
                    break;
                }
                a += 1;
                if a > 10 {
                    self.warn("Too many padding bytes");
                    return Err(HARD);
                }
            }
            if pos + 2 > f.len() {
                self.warn("Unexpected end of file reached");
                return Ok(());
            }
            let itemlen = u16::from_be_bytes([f[pos], f[pos + 1]]) as usize;
            if itemlen < 2 {
                self.warn("File structure corrupted");
                return Err(HARD);
            }
            let avail = f.len() - pos - 2;
            if avail < itemlen - 2 {
                self.warn(format!(
                    "Error reading from file: got=x{avail:04X}(={avail}) != itemlen-2=x{:04X}(={})",
                    itemlen - 2,
                    itemlen - 2
                ));
                return Err(HARD);
            }
            let data_at = pos + 2;
            let data = &f[data_at..data_at + itemlen - 2];
            pos += itemlen;
            last = marker;
            match marker {
                0xDA | 0xD9 => return Ok(()),
                0xFE => self.comment(data),
                0xE1 if data.starts_with(b"Exif\0\0") => {
                    if self
                        .process_tiff_in_jpeg(data_at + 6, data.len() - 6)
                        .is_err()
                    {
                        self.warn("File structure corrupted");
                        return Err(HARD);
                    }
                }
                // SOFn (ITU T.81 table B.1): precision, lines, samples per
                // line, components.
                0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) && data.len() >= 6 => {
                    self.height = u16::from_be_bytes([data[1], data[2]]) as u32;
                    self.width = u16::from_be_bytes([data[3], data[4]]) as u32;
                    self.is_color = data[5] == 3;
                }
                _ => {}
            }
        }
    }

    fn comment(&mut self, data: &[u8]) {
        let n = data.iter().position(|&c| c == 0).unwrap_or(data.len());
        self.comments.push(Node::Str(data[..n].to_vec()));
        self.found |= 1 << S_COMMENT;
    }

    // ------------------------------------------------------------ files

    /// Reads the file by its type; Err after the error was reported.
    fn scan_file(&mut self) -> Res<()> {
        let f = self.file;
        if f.len() < 2 {
            self.warn(format!("File too small ({})", f.len()));
            return Err(HARD);
        }
        let short = f.len() < 12;
        if f[..2] == [0xFF, 0xD8] {
            self.file_type = IMAGETYPE_JPEG;
            if self.scan_jpeg().is_err() {
                self.warn("Invalid JPEG file");
                return Err(HARD);
            }
            return Ok(());
        }
        if f.len() >= 4 && (&f[..4] == b"II*\0" || &f[..4] == b"MM\0*") {
            self.motorola = f[0] == b'M';
            self.have_order = true;
            self.tiff_mode = true;
            self.file_type = if self.motorola {
                IMAGETYPE_TIFF_MM
            } else {
                IMAGETYPE_TIFF_II
            };
            if f.len() < 8 {
                self.warn("Invalid TIFF file");
                return Err(HARD);
            }
            let first = self.get32(&f[4..]);
            self.found |= 1 << S_IFD0;
            if let Err(f) = self.process_ifd(first as usize, 0, S_IFD0, Table::Ifd)
                && f.hard
            {
                // Past the error limit the file is given up (observed on PHP
                // 8.5, tests bug71534.phpt, nesting_level_oom.phpt).
                return Err(HARD);
            }
            if self.read_thumbnail {
                self.thumbnail_check_tiff();
            }
            return Ok(());
        }
        if imageinfo::heif_brand(f) == Some(IMAGETYPE_HEIF) || is_heif_ftyp(f) {
            self.file_type = IMAGETYPE_HEIF;
            return self.scan_heif();
        }
        // Too short for a HEIF header: no diagnostic (observed on PHP 8.5,
        // ext/standard/tests/image/bug70052_2.wbmp).
        if !short {
            self.warn("File not supported");
        }
        Err(HARD)
    }

    /// HEIF: the item of type "Exif" located through `iloc`
    /// (ISO/IEC 23008-12 annex A: a 4-byte offset to the TIFF header).
    fn scan_heif(&mut self) -> Res<()> {
        let f = self.file;
        let Some(loc) = heif_exif_location(f) else {
            self.warn("Invalid HEIF file");
            return Err(HARD);
        };
        let (off, len) = loc;
        if len < 4 || off + len > f.len() {
            self.warn("Invalid HEIF file");
            return Err(HARD);
        }
        let skip = u32::from_be_bytes(f[off..off + 4].try_into().unwrap()) as usize;
        let t = off + 4 + skip;
        if t + 8 > off + len {
            self.warn("Invalid HEIF file");
            return Err(HARD);
        }
        if self.process_tiff_in_jpeg(t, off + len - t).is_err() {
            self.warn("File structure corrupted");
            return Err(HARD);
        }
        Ok(())
    }

    // ------------------------------------------------------------ result

    fn computed(&mut self) -> Vec<(Vec<u8>, Node)> {
        let mut c: Vec<(Vec<u8>, Node)> = Vec::new();
        let mut put = |k: &str, v: Node| c.push((k.as_bytes().to_vec(), v));
        if self.width != 0 && self.height != 0 {
            put(
                "html",
                Node::s(format!(
                    "width=\"{}\" height=\"{}\"",
                    self.width, self.height
                )),
            );
            put("Height", Node::Int(self.height as i64));
            put("Width", Node::Int(self.width as i64));
        }
        put("IsColor", Node::Int(self.is_color as i64));
        if self.have_order {
            put("ByteOrderMotorola", Node::Int(self.motorola as i64));
        }
        if self.focal_plane_x_res != 0.0
            && self.exif_image_width != 0
            && self.focal_plane_unit != 0.0
        {
            self.ccd_width =
                self.exif_image_width as f64 * self.focal_plane_unit / self.focal_plane_x_res;
            put("CCDWidth", Node::s(format!("{}mm", self.ccd_width as i64)));
        }
        // APEX values (Exif 2.3 annex C): Tv = -log2(exposure time),
        // Av = 2 log2(F number). The computed exposure time is shown when
        // only ShutterSpeedValue is given (observed on PHP 8.5).
        if let (None, Some(tv)) = (self.exposure_time, self.shutter_speed) {
            let et = (-tv * std::f64::consts::LN_2).exp();
            if et > 0.0 {
                let s = if et <= 0.5 {
                    format!("{:.3} s (1/{})", et, (0.5 + 1.0 / et) as i64)
                } else {
                    format!("{:.3} s", et)
                };
                put("ExposureTime", Node::s(s));
            }
        }
        let mut aperture = self.aperture_fnumber;
        if aperture == 0.0 && self.aperture_value != 0.0 {
            aperture = (self.aperture_value * std::f64::consts::LN_2 * 0.5).exp();
        }
        if aperture == 0.0 && self.max_aperture != 0.0 {
            aperture = (self.max_aperture * std::f64::consts::LN_2 * 0.5).exp();
        }
        if aperture != 0.0 {
            put("ApertureFNumber", Node::s(format!("f/{:.1}", aperture)));
        }
        if self.distance != 0.0 {
            let d = if self.distance < 0.0 {
                "Infinite".to_string()
            } else {
                format!("{:.2}m", self.distance)
            };
            put("FocusDistance", Node::s(d));
        }
        if let Some((text, enc)) = self.user_comment.clone() {
            put(
                "UserComment",
                match text {
                    Some(t) => Node::Str(t),
                    None => Node::Null,
                },
            );
            if let Some(e) = enc {
                put("UserCommentEncoding", Node::s(e));
            }
        }
        if let Some(cp) = &self.copyright {
            put("Copyright", Node::Str(cp.clone()));
        }
        if let Some(p) = &self.copyright_photographer {
            put("Copyright.Photographer", Node::Str(p.clone()));
        }
        if let Some(e) = &self.copyright_editor {
            put("Copyright.Editor", Node::Str(e.clone()));
        }
        if self.thumb.filetype != 0 {
            put("Thumbnail.FileType", Node::Int(self.thumb.filetype));
            put(
                "Thumbnail.MimeType",
                Node::s(imageinfo::mime_type(self.thumb.filetype)),
            );
        }
        if self.thumb.height != 0 && self.thumb.width != 0 {
            put("Thumbnail.Height", Node::Int(self.thumb.height as i64));
            put("Thumbnail.Width", Node::Int(self.thumb.width as i64));
        }
        c
    }

    /// The size of the thumbnail image itself (its JPEG SOF), when the
    /// IFD did not give it.
    fn thumbnail_size(&mut self) {
        let Some(d) = &self.thumb.data else { return };
        if self.thumb.filetype != IMAGETYPE_JPEG {
            return;
        }
        if self.thumb.width != 0 && self.thumb.height != 0 {
            return;
        }
        let mut pos = 2usize;
        if d.len() < 2 || d[0] != 0xFF || d[1] != 0xD8 {
            return;
        }
        loop {
            while pos < d.len() && d[pos] == 0xFF {
                pos += 1;
            }
            let Some(&m) = d.get(pos) else { return };
            if pos == 0 || d[pos - 1] != 0xFF {
                return;
            }
            pos += 1;
            if pos + 2 > d.len() {
                return;
            }
            let l = u16::from_be_bytes([d[pos], d[pos + 1]]) as usize;
            if l < 2 {
                return;
            }
            match m {
                0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) => {
                    if pos + 7 <= d.len() {
                        self.thumb.height = u16::from_be_bytes([d[pos + 3], d[pos + 4]]) as u32;
                        self.thumb.width = u16::from_be_bytes([d[pos + 5], d[pos + 6]]) as u32;
                    }
                    return;
                }
                0xDA | 0xD9 => return,
                _ => pos += l,
            }
        }
    }
}

fn cstr(v: &[u8]) -> Vec<u8> {
    let n = v.iter().position(|&c| c == 0).unwrap_or(v.len());
    v[..n].to_vec()
}

/// A HEIF `ftyp` box (any brand PHP's exif reader accepts).
fn is_heif_ftyp(f: &[u8]) -> bool {
    f.len() >= 12 && &f[4..8] == b"ftyp" && {
        let b = &f[8..12];
        b == b"heic" || b == b"heix" || b == b"mif1"
    }
}

/// Offset and length of the "Exif" item of a HEIF file.
fn heif_exif_location(f: &[u8]) -> Option<(usize, usize)> {
    let top = imageinfo::boxes(f, 0, f.len());
    let &(_, ms, me) = top.iter().find(|b| &b.0 == b"meta")?;
    let meta = imageinfo::boxes(f, ms + 4, me);
    let mut exif_id = None;
    for &(t, s, e) in &meta {
        if &t != b"iinf" || e < s + 6 {
            continue;
        }
        // ISO/IEC 14496-12 ItemInfoBox: a 16-bit (version 0) or 32-bit
        // entry count, then the ItemInfoEntry boxes.
        let p = if f[s] == 0 { s + 6 } else { s + 8 };
        let entries = imageinfo::boxes(f, p, e);
        for &(it, is, ie) in &entries {
            if &it != b"infe" || ie < is + 4 {
                continue;
            }
            let v = f[is];
            if v >= 2 {
                let (id, q) = if v == 2 {
                    (be16(f, is + 4) as u32, is + 6)
                } else {
                    (be32(f, is + 4), is + 8)
                };
                let q = q + 2;
                if q + 4 <= ie && &f[q..q + 4] == b"Exif" {
                    exif_id = Some(id);
                }
            }
        }
    }
    let id = exif_id?;
    for &(t, s, e) in &meta {
        if &t != b"iloc" || e < s + 8 {
            continue;
        }
        let ver = f[s];
        // Version 0 tables are not read (observed on PHP 8.5: "Invalid HEIF
        // file" for ext/standard/tests/image/test4pix.heic).
        if ver == 0 {
            return None;
        }
        let sizes = be16(f, s + 4) as u32;
        let off_size = (sizes >> 12) & 15;
        let len_size = (sizes >> 8) & 15;
        let base_size = (sizes >> 4) & 15;
        let idx_size = if ver == 1 || ver == 2 { sizes & 15 } else { 0 };
        let mut p = s + 6;
        let count = if ver < 2 {
            p += 2;
            be16(f, p - 2) as u32
        } else {
            p += 4;
            be32(f, p - 4)
        };
        let rd = |p: &mut usize, n: u32| -> Option<u64> {
            let n = n as usize;
            if *p + n > e {
                return None;
            }
            let mut v = 0u64;
            for i in 0..n {
                v = (v << 8) | f[*p + i] as u64;
            }
            *p += n;
            Some(v)
        };
        for _ in 0..count {
            let item = if ver < 2 {
                rd(&mut p, 2)?
            } else {
                rd(&mut p, 4)?
            } as u32;
            if ver == 1 || ver == 2 {
                rd(&mut p, 2)?;
            }
            rd(&mut p, 2)?;
            let base = rd(&mut p, base_size)?;
            let ext = rd(&mut p, 2)?;
            let mut first = None;
            for _ in 0..ext {
                if idx_size > 0 {
                    rd(&mut p, idx_size)?;
                }
                let o = rd(&mut p, off_size)?;
                let l = rd(&mut p, len_size)?;
                if first.is_none() {
                    first = Some((o, l));
                }
            }
            if item == id {
                let (o, l) = first?;
                let start = base.checked_add(o)? as usize;
                return Some((start, l as usize));
            }
        }
    }
    None
}

fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Options of exif_read_data().
#[derive(Debug, Clone, Default)]
pub struct ReadOptions {
    /// `$required_sections`, as given.
    pub required: Option<Vec<u8>>,
    pub as_arrays: bool,
    pub read_thumbnail: bool,
}

/// The section bits of a `$required_sections` list (comma or space
/// separated names, case-insensitive).
fn required_mask(s: &[u8]) -> u32 {
    let mut m = 0u32;
    for name in s.split(|&c| c == b',' || c == b' ') {
        if name.is_empty() {
            continue;
        }
        if let Some(i) = SECTION_NAMES
            .iter()
            .position(|n| n.as_bytes().eq_ignore_ascii_case(name))
        {
            m |= 1 << i;
        }
    }
    m
}

/// exif_read_data(): the result array, or None (false).
pub fn read_data(
    d: &[u8],
    meta: &FileMeta,
    opts: &ReadOptions,
    ini: &Ini,
    cs: &dyn Charset,
) -> (Option<Node>, Vec<Diag>) {
    let mut info = Info::new(d, ini, cs);
    info.read_thumbnail = opts.read_thumbnail;
    let ok = info.scan_file().is_ok();
    if !ok {
        return (None, info.diags);
    }
    let required = opts.required.as_deref().map(required_mask).unwrap_or(0);
    let found = info.found | (1 << S_FILE) | (1 << S_COMPUTED);
    if required & !found != 0 {
        return (None, info.diags);
    }
    if opts.read_thumbnail {
        info.thumbnail_size();
    }
    let mut file: Vec<(Vec<u8>, Node)> = Vec::new();
    if let Some(n) = &meta.name {
        file.push((b"FileName".to_vec(), Node::Str(n.clone())));
    }
    file.push((b"FileDateTime".to_vec(), Node::Int(meta.mtime)));
    file.push((b"FileSize".to_vec(), Node::Int(d.len() as i64)));
    file.push((b"FileType".to_vec(), Node::Int(info.file_type)));
    file.push((
        b"MimeType".to_vec(),
        Node::s(imageinfo::mime_type(info.file_type)),
    ));
    let mut list = Vec::new();
    for (i, n) in SECTION_NAMES.iter().enumerate().skip(S_ANY_TAG) {
        if info.found & (1 << i) != 0 {
            list.push(*n);
        }
    }
    file.push((b"SectionsFound".to_vec(), Node::s(list.join(", "))));
    let computed = info.computed();
    if opts.read_thumbnail && info.thumb.filetype != 0 {
        let v = match info.thumb.data.take() {
            Some(t) => Node::Str(t),
            None => Node::Null,
        };
        info.sections[S_THUMBNAIL].push((b"THUMBNAIL".to_vec(), v));
    }
    let mut out: Vec<(Vec<u8>, Node)> = Vec::new();
    if opts.as_arrays {
        out.push((b"FILE".to_vec(), Node::Map(file)));
        out.push((b"COMPUTED".to_vec(), Node::Map(computed)));
    } else {
        out.extend(file);
        out.push((b"COMPUTED".to_vec(), Node::Map(computed)));
    }
    for (i, name) in SECTION_NAMES.iter().enumerate().skip(S_IFD0) {
        let tags = if i == S_COMMENT {
            if info.comments.is_empty() {
                continue;
            }
            let c = std::mem::take(&mut info.comments);
            // COMMENT is a list in both modes (test exif000.phpt).
            out.push((name.as_bytes().to_vec(), Node::List(c)));
            continue;
        } else {
            std::mem::take(&mut info.sections[i])
        };
        if tags.is_empty() {
            continue;
        }
        if opts.as_arrays || i == S_THUMBNAIL {
            out.push((name.as_bytes().to_vec(), Node::Map(tags)));
        } else {
            for (k, v) in tags {
                map_set(&mut out, k, v);
            }
        }
    }
    (Some(Node::Map(out)), info.diags)
}

/// What exif_thumbnail() returns.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub image_type: i64,
}

/// exif_thumbnail(): the embedded thumbnail.
pub fn thumbnail(d: &[u8], ini: &Ini, cs: &dyn Charset) -> (Option<Thumbnail>, Vec<Diag>) {
    let mut info = Info::new(d, ini, cs);
    info.read_thumbnail = true;
    if info.scan_file().is_err() {
        return (None, info.diags);
    }
    info.thumbnail_size();
    let r = info.thumb.data.take().map(|data| Thumbnail {
        data,
        width: info.thumb.width,
        height: info.thumb.height,
        image_type: info.thumb.filetype,
    });
    (r, info.diags)
}
