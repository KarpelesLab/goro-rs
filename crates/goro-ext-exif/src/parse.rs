//! The image scanner of exif.c: JPEG marker stream, TIFF / EXIF IFDs,
//! maker notes, HEIF meta boxes and thumbnails.
//!
//! C works with pointers into several buffers (the file sections read
//! from the stream, temporary buffers for out-of-range tag values); here a
//! pointer is a buffer index plus an offset (`Ptr`), and the valid ranges
//! of offset references are `Info`s over one buffer.

use goro_core::vm::Vm;

use crate::tables::*;

pub const SECTION_FILE: usize = 0;
pub const SECTION_COMPUTED: usize = 1;
pub const SECTION_ANY_TAG: usize = 2;
pub const SECTION_IFD0: usize = 3;
pub const SECTION_THUMBNAIL: usize = 4;
pub const SECTION_COMMENT: usize = 5;
pub const SECTION_APP0: usize = 6;
pub const SECTION_EXIF: usize = 7;
pub const SECTION_FPIX: usize = 8;
pub const SECTION_GPS: usize = 9;
pub const SECTION_INTEROP: usize = 10;
pub const SECTION_APP12: usize = 11;
pub const SECTION_WINXP: usize = 12;
pub const SECTION_MAKERNOTE: usize = 13;
pub const SECTION_COUNT: usize = 14;

pub const FOUND_COMPUTED: u32 = 1 << SECTION_COMPUTED;
pub const FOUND_FILE: u32 = 1 << SECTION_FILE;
const FOUND_ANY_TAG: u32 = 1 << SECTION_ANY_TAG;
const FOUND_IFD0: u32 = 1 << SECTION_IFD0;
const FOUND_THUMBNAIL: u32 = 1 << SECTION_THUMBNAIL;
const FOUND_EXIF: u32 = 1 << SECTION_EXIF;
const FOUND_GPS: u32 = 1 << SECTION_GPS;
const FOUND_INTEROP: u32 = 1 << SECTION_INTEROP;
const FOUND_WINXP: u32 = 1 << SECTION_WINXP;
const FOUND_MAKERNOTE: u32 = 1 << SECTION_MAKERNOTE;

pub const IMAGE_FILETYPE_UNKNOWN: i64 = 0;
pub const IMAGE_FILETYPE_JPEG: i64 = 2;
pub const IMAGE_FILETYPE_TIFF_II: i64 = 7;
pub const IMAGE_FILETYPE_TIFF_MM: i64 = 8;
pub const IMAGE_FILETYPE_HEIF: i64 = 20;

const MAX_IFD_NESTING_LEVEL: i32 = 10;
const MAX_IFD_TAGS: i32 = 1000;
const EXIF_MAX_ERRORS: i32 = 10;

const BYTES_PER_FORMAT: [u32; 14] = [0, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 4, 8, 1];
const NUM_FORMATS: u32 = 13;

const M_SOF0: i32 = 0xC0;
const M_SOF1: i32 = 0xC1;
const M_SOF2: i32 = 0xC2;
const M_SOF3: i32 = 0xC3;
const M_SOF5: i32 = 0xC5;
const M_SOF6: i32 = 0xC6;
const M_SOF7: i32 = 0xC7;
const M_SOF9: i32 = 0xC9;
const M_SOF10: i32 = 0xCA;
const M_SOF11: i32 = 0xCB;
const M_SOF13: i32 = 0xCD;
const M_SOF14: i32 = 0xCE;
const M_SOF15: i32 = 0xCF;
const M_SOI: u8 = 0xD8;
const M_EOI: i32 = 0xD9;
const M_SOS: i32 = 0xDA;
const M_EXIF: i32 = 0xE1;
const M_APP12: i32 = 0xEC;
const M_COM: i32 = 0xFE;
const M_PSEUDO: i32 = 0x123;

fn is_sof(m: i32) -> bool {
    matches!(
        m,
        M_SOF0
            | M_SOF1
            | M_SOF2
            | M_SOF3
            | M_SOF5
            | M_SOF6
            | M_SOF7
            | M_SOF9
            | M_SOF10
            | M_SOF11
            | M_SOF13
            | M_SOF14
            | M_SOF15
    )
}

pub type Table = &'static [(u16, &'static str)];

/// exif_get_tagname(): TAG_TABLE_END adds TAG_NONE / TAG_COMPUTED_VALUE
/// (as unsigned shorts).
pub fn get_tagname(tag: i64, table: Table) -> Option<&'static str> {
    match tag {
        0xFFFF => return Some("No tag value"),
        0xFFFE => return Some("Computed value"),
        _ => {}
    }
    table.iter().find(|e| e.0 as i64 == tag).map(|e| e.1)
}

fn tagname_debug(tag: u32, table: Table) -> &'static str {
    get_tagname(tag as i64, table).unwrap_or("UndefinedTag")
}

fn tagname_key(tag: u32, table: Table) -> Vec<u8> {
    match get_tagname(tag as i64, table) {
        Some(n) => n.as_bytes().to_vec(),
        None => format!("UndefinedTag:0x{:04X}", tag).into_bytes(),
    }
}

pub fn get_sectionname(section: usize) -> &'static str {
    match section {
        SECTION_FILE => "FILE",
        SECTION_COMPUTED => "COMPUTED",
        SECTION_ANY_TAG => "ANY_TAG",
        SECTION_IFD0 => "IFD0",
        SECTION_THUMBNAIL => "THUMBNAIL",
        SECTION_COMMENT => "COMMENT",
        SECTION_APP0 => "APP0",
        SECTION_EXIF => "EXIF",
        SECTION_FPIX => "FPIX",
        SECTION_GPS => "GPS",
        SECTION_INTEROP => "INTEROP",
        SECTION_APP12 => "APP12",
        SECTION_WINXP => "WINXP",
        SECTION_MAKERNOTE => "MAKERNOTE",
        _ => "",
    }
}

pub fn get_tag_table(section: usize) -> Table {
    match section {
        SECTION_GPS => GPS,
        SECTION_INTEROP => IOP,
        _ => IFD,
    }
}

/// exif_get_sectionlist()
pub fn get_sectionlist(list: u32) -> String {
    let mut s = String::new();
    for i in 0..SECTION_COUNT {
        if list & (1 << i) != 0 {
            s.push_str(get_sectionname(i));
            s.push_str(", ");
        }
    }
    if s.len() > 2 {
        s.truncate(s.len() - 2);
    }
    s
}

// ---------------------------------------------------------------------------
// maker notes

#[derive(Clone, Copy, PartialEq)]
enum MnOrder {
    Intel,
    Motorola,
    Normal,
}

#[derive(Clone, Copy, PartialEq)]
enum MnOffset {
    Normal,
    Maker,
}

struct MakerNote {
    /// None: REQUIRES_CUSTOM_PARSING
    table: Option<Table>,
    make: &'static [u8],
    id: Option<&'static [u8]>,
    offset: i32,
    order: MnOrder,
    mode: MnOffset,
}

macro_rules! mn {
    ($t:expr, $make:expr, $id:expr, $len:expr, $off:expr, $o:ident, $m:ident) => {
        MakerNote {
            table: $t,
            make: $make,
            id: $id,
            offset: $off,
            order: MnOrder::$o,
            mode: MnOffset::$m,
        }
    };
}

static MAKER_NOTES: &[MakerNote] = &[
    mn!(Some(VND_CANON), b"Canon", None, 0, 0, Intel, Normal),
    mn!(Some(VND_CASIO), b"CASIO", None, 0, 0, Motorola, Normal),
    mn!(
        Some(VND_FUJI),
        b"FUJIFILM",
        Some(b"FUJIFILM\x0C\x00\x00\x00"),
        12,
        12,
        Intel,
        Maker
    ),
    mn!(
        Some(VND_NIKON),
        b"NIKON",
        Some(b"Nikon\x00\x01\x00"),
        8,
        8,
        Normal,
        Normal
    ),
    mn!(Some(VND_NIKON_990), b"NIKON", None, 0, 0, Normal, Normal),
    mn!(
        Some(VND_OLYMPUS),
        b"OLYMPUS OPTICAL CO.,LTD",
        Some(b"OLYMP\x00\x01\x00"),
        8,
        8,
        Normal,
        Normal
    ),
    mn!(Some(VND_SAMSUNG), b"SAMSUNG", None, 0, 0, Normal, Normal),
    mn!(
        Some(VND_PANASONIC),
        b"Panasonic",
        Some(b"Panasonic\x00\x00\x00"),
        12,
        12,
        Normal,
        Normal
    ),
    mn!(
        None,
        b"DJI",
        Some(b"[ae_dbg_info:"),
        13,
        13,
        Motorola,
        Normal
    ),
    mn!(Some(VND_DJI), b"DJI", None, 0, 0, Normal, Normal),
    mn!(
        Some(VND_SONY),
        b"SONY",
        Some(b"SONY DSC \x00\x00\x00"),
        12,
        12,
        Normal,
        Normal
    ),
    mn!(Some(VND_SONY), b"SONY", None, 0, 0, Normal, Normal),
    mn!(
        Some(VND_PENTAX),
        b"PENTAX",
        Some(b"AOC\x00"),
        6,
        6,
        Normal,
        Normal
    ),
    mn!(
        Some(VND_MINOLTA),
        b"Minolta, KONICA MINOLTA",
        None,
        0,
        0,
        Normal,
        Normal
    ),
    mn!(
        Some(VND_SIGMA),
        b"SIGMA, FOVEON",
        Some(b"SIGMA\x00\x00\x00"),
        10,
        10,
        Normal,
        Normal
    ),
    mn!(
        Some(VND_SIGMA),
        b"SIGMA, FOVEON",
        Some(b"FOVEON\x00\x00\x00"),
        10,
        10,
        Normal,
        Normal
    ),
    mn!(
        Some(VND_KYOCERA),
        b"KYOCERA, CONTAX",
        Some(b"KYOCERA            \x00\x00\x00"),
        22,
        22,
        Normal,
        Maker
    ),
    mn!(
        Some(VND_RICOH),
        b"RICOH",
        Some(b"Ricoh"),
        5,
        5,
        Motorola,
        Normal
    ),
    mn!(
        Some(VND_RICOH),
        b"RICOH",
        Some(b"RICOH"),
        5,
        5,
        Motorola,
        Normal
    ),
    mn!(
        Some(VND_OLYMPUS),
        b"AGFA",
        Some(b"AGFA \x00\x01"),
        8,
        8,
        Normal,
        Normal
    ),
    mn!(
        Some(VND_OLYMPUS),
        b"EPSON",
        Some(b"EPSON\x00\x01\x00"),
        8,
        8,
        Normal,
        Normal
    ),
];

// ---------------------------------------------------------------------------
// values

#[derive(Clone, Debug)]
pub enum Val {
    None,
    S(Vec<u8>),
    U(u32),
    I(i32),
    F(f32),
    D(f64),
    Ur(u32, u32),
    Sr(i32, i32),
    List(Vec<Val>),
}

pub struct InfoData {
    pub tag: u16,
    pub format: u16,
    pub length: u32,
    pub name: Vec<u8>,
    pub value: Val,
}

#[derive(Default)]
pub struct Thumbnail {
    pub filetype: i64,
    pub width: u64,
    pub height: u64,
    pub size: u64,
    pub offset: u64,
    pub data: Option<Vec<u8>>,
}

pub struct XpField {
    pub tag: u32,
    pub value: Option<Vec<u8>>,
}

/// The stream being scanned (php_stream over the whole content).
pub struct Stream {
    pub data: Vec<u8>,
    pub pos: u64,
    /// Plain files can be positioned past their end.
    pub past_end: bool,
}

impl Stream {
    fn getc(&mut self) -> i32 {
        match self.data.get(self.pos as usize) {
            Some(&b) if self.pos < self.data.len() as u64 => {
                self.pos += 1;
                b as i32
            }
            _ => -1,
        }
    }

    fn read(&mut self, n: u64) -> Vec<u8> {
        let len = self.data.len() as u64;
        if self.pos >= len {
            return Vec::new();
        }
        let end = self.pos.saturating_add(n).min(len);
        let v = self.data[self.pos as usize..end as usize].to_vec();
        self.pos = end;
        v
    }

    fn seek(&mut self, off: u64) -> bool {
        if off > self.data.len() as u64 && !self.past_end {
            return false;
        }
        self.pos = off;
        true
    }

    fn tell(&self) -> u64 {
        self.pos
    }
}

#[derive(Clone, Copy, Debug)]
struct Ptr {
    b: usize,
    o: i64,
}

impl Ptr {
    fn add(self, n: i64) -> Ptr {
        Ptr {
            b: self.b,
            o: self.o.wrapping_add(n),
        }
    }
}

/// exif_offset_info: base address for offset references and the valid
/// range, within buffer `b`.
#[derive(Clone, Copy, Debug)]
struct Info {
    b: usize,
    base: i64,
    start: i64,
    end: i64,
}

impl Info {
    fn new(base: Ptr, start: Ptr, len: u64) -> Info {
        Info {
            b: start.b,
            base: base.o,
            start: start.o,
            end: start.o.wrapping_add(len as i64),
        }
    }

    /// exif_offset_info_try_get()
    fn try_get(&self, offset: u64, length: u64) -> Option<Ptr> {
        if offset >= (1u64 << 62) || length >= (1u64 << 62) {
            return None;
        }
        let start = self.base + offset as i64;
        let end = start + length as i64;
        if start < self.start || end > self.end {
            return None;
        }
        Some(Ptr {
            b: self.b,
            o: start,
        })
    }

    /// exif_offset_info_contains()
    fn contains(&self, p: Ptr, length: i64) -> bool {
        if p.b != self.b || length < 0 {
            return false;
        }
        p.o >= self.start && p.o + length <= self.end
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Level {
    Warning,
    Notice,
}

pub struct ImageInfo {
    pub stream: Stream,
    pub file_name: Option<Vec<u8>>,
    pub file_date_time: i64,
    pub file_size: u64,
    pub file_type: i64,
    pub height: i32,
    pub width: i32,
    pub is_color: i32,
    make: Option<Vec<u8>>,
    model: Option<Vec<u8>>,
    pub aperture_fnumber: f32,
    pub exposure_time: f32,
    focalplane_units: f64,
    pub ccd_width: f32,
    focalplane_x_res: f64,
    exif_image_width: u64,
    pub focal_length: f32,
    pub distance: f32,
    pub motorola_intel: i32,
    pub user_comment: Option<Vec<u8>>,
    pub user_comment_length: i32,
    pub user_comment_encoding: Option<Vec<u8>>,
    pub encode_unicode: Vec<u8>,
    pub decode_unicode_be: Vec<u8>,
    pub decode_unicode_le: Vec<u8>,
    pub encode_jis: Vec<u8>,
    pub decode_jis_be: Vec<u8>,
    pub decode_jis_le: Vec<u8>,
    pub copyright: Option<Vec<u8>>,
    pub copyright_photographer: Option<Vec<u8>>,
    pub copyright_editor: Option<Vec<u8>>,
    pub xp_fields: Vec<XpField>,
    pub thumb: Thumbnail,
    pub sections_found: u32,
    pub info_list: Vec<Vec<InfoData>>,
    pub read_thumbnail: bool,
    pub read_all: bool,
    ifd_nesting_level: i32,
    ifd_count: i32,
    num_errors: i32,
    /// The buffers pointers refer to.
    mem: Vec<Vec<u8>>,
    /// file sections: (marker, buffer)
    sections: Vec<(i32, usize)>,
}

fn c_str(v: &[u8]) -> Vec<u8> {
    let n = v.iter().position(|&c| c == 0).unwrap_or(v.len());
    v[..n].to_vec()
}

impl ImageInfo {
    pub fn new(stream: Stream) -> ImageInfo {
        ImageInfo {
            stream,
            file_name: None,
            file_date_time: 0,
            file_size: 0,
            file_type: IMAGE_FILETYPE_UNKNOWN,
            height: 0,
            width: 0,
            is_color: 0,
            make: None,
            model: None,
            aperture_fnumber: 0.0,
            exposure_time: 0.0,
            focalplane_units: 0.0,
            ccd_width: 0.0,
            focalplane_x_res: 0.0,
            exif_image_width: 0,
            focal_length: 0.0,
            distance: 0.0,
            motorola_intel: -1,
            user_comment: None,
            user_comment_length: 0,
            user_comment_encoding: None,
            encode_unicode: Vec::new(),
            decode_unicode_be: Vec::new(),
            decode_unicode_le: Vec::new(),
            encode_jis: Vec::new(),
            decode_jis_be: Vec::new(),
            decode_jis_le: Vec::new(),
            copyright: None,
            copyright_photographer: None,
            copyright_editor: None,
            xp_fields: Vec::new(),
            thumb: Thumbnail::default(),
            sections_found: 0,
            info_list: (0..SECTION_COUNT).map(|_| Vec::new()).collect(),
            read_thumbnail: false,
            read_all: false,
            ifd_nesting_level: 0,
            ifd_count: 0,
            num_errors: 0,
            mem: Vec::new(),
            sections: Vec::new(),
        }
    }

    /// exif_error_docref()
    pub fn error(&mut self, vm: &mut Vm, level: Level, msg: &str) {
        let func = vm.current_builtin_name().unwrap_or_default();
        self.num_errors += 1;
        if self.num_errors > EXIF_MAX_ERRORS {
            if self.num_errors == EXIF_MAX_ERRORS + 1 {
                emit(
                    vm,
                    level,
                    &format!(
                        "{}(): Further exif parsing errors have been suppressed",
                        func
                    ),
                );
            }
            return;
        }
        let params = self
            .file_name
            .as_deref()
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .unwrap_or_default();
        emit(vm, level, &format!("{}({}): {}", func, params, msg));
    }

    fn warn(&mut self, vm: &mut Vm, msg: &str) {
        self.error(vm, Level::Warning, msg)
    }

    fn mi(&self) -> bool {
        self.motorola_intel != 0
    }

    // -- memory -----------------------------------------------------------

    fn alloc(&mut self, data: Vec<u8>) -> Ptr {
        self.mem.push(data);
        Ptr {
            b: self.mem.len() - 1,
            o: 0,
        }
    }

    fn rd(&self, p: Ptr, i: i64) -> u8 {
        let idx = p.o.wrapping_add(i);
        if idx < 0 {
            return 0;
        }
        self.mem[p.b].get(idx as usize).copied().unwrap_or(0)
    }

    fn g16(&self, p: Ptr, mi: bool) -> u32 {
        let (a, b) = (self.rd(p, 0) as u32, self.rd(p, 1) as u32);
        if mi { (a << 8) | b } else { (b << 8) | a }
    }

    fn g32(&self, p: Ptr, mi: bool) -> u32 {
        let v = [self.rd(p, 0), self.rd(p, 1), self.rd(p, 2), self.rd(p, 3)];
        if mi {
            u32::from_be_bytes(v)
        } else {
            u32::from_le_bytes(v)
        }
    }

    fn g64(&self, p: Ptr, mi: bool) -> u64 {
        let mut v = [0u8; 8];
        for (i, b) in v.iter_mut().enumerate() {
            *b = self.rd(p, i as i64);
        }
        if mi {
            u64::from_be_bytes(v)
        } else {
            u64::from_le_bytes(v)
        }
    }

    fn bytes(&self, p: Ptr, n: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(n.min(1 << 24) as usize);
        let buf = &self.mem[p.b];
        for i in 0..n as i64 {
            let idx = p.o + i;
            out.push(if idx >= 0 {
                buf.get(idx as usize).copied().unwrap_or(0)
            } else {
                0
            });
        }
        out
    }

    fn strnlen(&self, p: Ptr, n: u64) -> u64 {
        let mut i = 0;
        while i < n && self.rd(p, i as i64) != 0 {
            i += 1;
        }
        i
    }

    // -- conversions --------------------------------------------------------

    /// exif_convert_any_format()
    fn convert_any_format(&self, p: Ptr, format: u32, mi: bool) -> f64 {
        match format {
            TAG_FMT_SBYTE => self.rd(p, 0) as i8 as f64,
            TAG_FMT_BYTE => self.rd(p, 0) as f64,
            TAG_FMT_USHORT => self.g16(p, mi) as f64,
            TAG_FMT_ULONG => self.g32(p, mi) as f64,
            TAG_FMT_URATIONAL => {
                let den = self.g32(p.add(4), mi);
                if den == 0 {
                    0.0
                } else {
                    self.g32(p, mi) as f64 / den as f64
                }
            }
            TAG_FMT_SRATIONAL => {
                let den = self.g32(p.add(4), mi) as i32;
                if den == 0 {
                    0.0
                } else {
                    // (double)php_ifd_get32s() is unsigned
                    self.g32(p, mi) as f64 / den as f64
                }
            }
            TAG_FMT_SSHORT => self.g16(p, mi) as u16 as i16 as f64,
            TAG_FMT_SLONG => self.g32(p, mi) as f64,
            TAG_FMT_SINGLE => f32::from_bits(self.g32(p, false)) as f64,
            TAG_FMT_DOUBLE => f64::from_bits(self.g64(p, false)),
            _ => 0.0,
        }
    }

    /// exif_convert_any_to_int()
    #[allow(clippy::manual_checked_ops)]
    fn convert_any_to_int(&self, p: Ptr, format: u32, mi: bool) -> u64 {
        match format {
            TAG_FMT_SBYTE => self.rd(p, 0) as i8 as i64 as u64,
            TAG_FMT_BYTE => self.rd(p, 0) as u64,
            TAG_FMT_USHORT => self.g16(p, mi) as u64,
            TAG_FMT_ULONG => self.g32(p, mi) as u64,
            TAG_FMT_URATIONAL => {
                let den = self.g32(p.add(4), mi);
                if den == 0 {
                    0
                } else {
                    (self.g32(p, mi) / den) as u64
                }
            }
            TAG_FMT_SRATIONAL => {
                let num = self.g32(p, mi) as i32;
                let den = self.g32(p.add(4), mi) as i32;
                if den == 0 {
                    0
                } else if num == i32::MIN && den == -1 {
                    i32::MAX as u64
                } else {
                    (num / den) as i64 as u64
                }
            }
            TAG_FMT_SSHORT => self.g16(p, mi) as u64,
            TAG_FMT_SLONG => self.g32(p, mi) as u64,
            TAG_FMT_SINGLE => {
                let x = f32::from_bits(self.g32(p, false));
                if x < 0.0 || x.is_nan() { 0 } else { x as u64 }
            }
            TAG_FMT_DOUBLE => {
                let x = f64::from_bits(self.g64(p, false));
                if x < 0.0 || x.is_nan() { 0 } else { x as u64 }
            }
            _ => 0,
        }
    }

    // -- image info lists ----------------------------------------------------

    /// exif_iif_add_value()
    #[allow(clippy::too_many_arguments)]
    fn add_value(
        &mut self,
        vm: &mut Vm,
        section: usize,
        name: Vec<u8>,
        tag: i32,
        format: u32,
        length: u64,
        value: Option<Vec<u8>>,
        value_len: u64,
        mi: bool,
    ) {
        // (value, when given, holds value_len bytes)
        let mut d = InfoData {
            tag: tag as u16,
            format: format as u16,
            length: length as u32,
            name,
            value: Val::None,
        };
        let mut value = value;
        let mut length = length;
        match format {
            TAG_FMT_STRING => {
                if length > value_len {
                    self.warn(
                        vm,
                        &format!("length > value_len: {} > {}", length as i32, value_len),
                    );
                    value = None;
                }
                if let Some(v) = value {
                    let n = v[..length as usize]
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(length as usize);
                    d.value = Val::S(v[..n].to_vec());
                    d.length = n as u32;
                } else {
                    d.length = 0;
                    d.value = Val::S(Vec::new());
                }
            }
            TAG_FMT_USHORT | TAG_FMT_ULONG | TAG_FMT_URATIONAL | TAG_FMT_SSHORT | TAG_FMT_SLONG
            | TAG_FMT_SRATIONAL | TAG_FMT_SINGLE | TAG_FMT_DOUBLE => {
                if length != 0 {
                    let v = value.unwrap_or_default();
                    let bpf = BYTES_PER_FORMAT[format as usize] as u64;
                    let rd16 = |o: usize| {
                        let (a, b) = (v[o] as u32, v[o + 1] as u32);
                        if mi { (a << 8) | b } else { (b << 8) | a }
                    };
                    let rd32 = |o: usize, mi: bool| {
                        let x = [v[o], v[o + 1], v[o + 2], v[o + 3]];
                        if mi {
                            u32::from_be_bytes(x)
                        } else {
                            u32::from_le_bytes(x)
                        }
                    };
                    let mut list = Vec::new();
                    let mut vp: u64 = 0;
                    for _ in 0..length {
                        if value_len < vp || value_len - vp < bpf {
                            self.warn(vm, "Value too short");
                            break;
                        }
                        let o = vp as usize;
                        let item = match format {
                            TAG_FMT_USHORT => Val::U(rd16(o)),
                            TAG_FMT_ULONG => Val::U(rd32(o, mi)),
                            TAG_FMT_URATIONAL => Val::Ur(rd32(o, mi), rd32(o + 4, mi)),
                            TAG_FMT_SSHORT => Val::I(rd16(o) as u16 as i16 as i32),
                            TAG_FMT_SLONG => Val::I(rd32(o, mi) as i32),
                            TAG_FMT_SRATIONAL => {
                                Val::Sr(rd32(o, mi) as i32, rd32(o + 4, mi) as i32)
                            }
                            // php_ifd_get_float(value): always the first one
                            TAG_FMT_SINGLE => Val::F(f32::from_bits(rd32(0, false))),
                            _ => {
                                let mut x = [0u8; 8];
                                x.copy_from_slice(&v[..8]);
                                Val::D(f64::from_bits(u64::from_le_bytes(x)))
                            }
                        };
                        list.push(item);
                        vp += bpf;
                    }
                    if length > 1 {
                        // (uninitialized entries of the C list)
                        let zero = match format {
                            TAG_FMT_URATIONAL => Val::Ur(0, 0),
                            TAG_FMT_SRATIONAL => Val::Sr(0, 0),
                            TAG_FMT_SINGLE => Val::F(0.0),
                            TAG_FMT_DOUBLE => Val::D(0.0),
                            TAG_FMT_SSHORT | TAG_FMT_SLONG => Val::I(0),
                            _ => Val::U(0),
                        };
                        if length <= 1 << 20 {
                            while (list.len() as u64) < length {
                                list.push(zero.clone());
                            }
                        }
                        d.value = Val::List(list);
                    } else if let Some(x) = list.pop() {
                        d.value = x;
                    } else {
                        d.value = match format {
                            TAG_FMT_URATIONAL => Val::Ur(0, 0),
                            TAG_FMT_SRATIONAL => Val::Sr(0, 0),
                            TAG_FMT_SINGLE => Val::F(0.0),
                            TAG_FMT_DOUBLE => Val::D(0.0),
                            TAG_FMT_SSHORT | TAG_FMT_SLONG => Val::I(0),
                            _ => Val::U(0),
                        };
                    }
                }
            }
            _ => {
                let byte_like = matches!(format, TAG_FMT_SBYTE | TAG_FMT_BYTE | TAG_FMT_UNDEFINED);
                if !byte_like {
                    // (otherwise not freed from memory)
                    d.tag = TAG_FMT_UNDEFINED as u16;
                }
                if format != TAG_FMT_UNDEFINED && length == 0 {
                    // bytes do not need a buffer for NUL when length == 0
                } else {
                    if length > value_len {
                        self.warn(
                            vm,
                            &format!("length > value_len: {} > {}", length as i32, value_len),
                        );
                        value = None;
                    }
                    if let Some(v) = value {
                        if tag == TAG_MAKER_NOTE {
                            length = v[..length as usize]
                                .iter()
                                .position(|&c| c == 0)
                                .unwrap_or(length as usize)
                                as u64;
                        }
                        d.value = Val::S(v[..length as usize].to_vec());
                        d.length = length as u32;
                    } else {
                        d.length = 0;
                        d.value = Val::S(Vec::new());
                    }
                }
            }
        }
        self.info_list[section].push(d);
        self.sections_found |= 1 << section;
    }

    /// exif_iif_add_tag() with the value at `p` (value_len bytes)
    #[allow(clippy::too_many_arguments)]
    fn add_tag_at(
        &mut self,
        vm: &mut Vm,
        section: usize,
        name: Vec<u8>,
        tag: i32,
        format: u32,
        length: u64,
        p: Ptr,
        value_len: u64,
    ) {
        let v = self.bytes(p, value_len);
        let mi = self.mi();
        self.add_value(
            vm,
            section,
            name,
            tag,
            format,
            length,
            Some(v),
            value_len,
            mi,
        );
    }

    /// exif_iif_add_tag()
    #[allow(clippy::too_many_arguments)]
    pub fn add_tag(
        &mut self,
        vm: &mut Vm,
        section: usize,
        name: &str,
        tag: i32,
        format: u32,
        length: u64,
        value: Option<Vec<u8>>,
        value_len: u64,
    ) {
        let mi = self.mi();
        let value = value.map(|mut v| {
            v.resize(value_len as usize, 0);
            v
        });
        self.add_value(
            vm,
            section,
            name.as_bytes().to_vec(),
            tag,
            format,
            length,
            value,
            value_len,
            mi,
        );
    }

    /// exif_iif_add_int()
    pub fn add_int(&mut self, section: usize, name: &str, value: i32) {
        self.info_list[section].push(InfoData {
            tag: TAG_NONE as u16,
            format: TAG_FMT_SLONG as u16,
            length: 1,
            name: name.as_bytes().to_vec(),
            value: Val::I(value),
        });
        self.sections_found |= 1 << section;
    }

    /// exif_iif_add_str() (value truncated at its NUL like estrdup())
    pub fn add_str(&mut self, section: usize, name: &[u8], value: Option<&[u8]>) {
        if let Some(v) = value {
            self.info_list[section].push(InfoData {
                tag: TAG_NONE as u16,
                format: TAG_FMT_STRING as u16,
                length: 1,
                name: name.to_vec(),
                value: Val::S(c_str(v)),
            });
            self.sections_found |= 1 << section;
        }
    }

    /// exif_iif_add_buffer()
    pub fn add_buffer(&mut self, section: usize, name: &str, length: i32, value: Option<&[u8]>) {
        if let Some(v) = value {
            let n = (length.max(0) as usize).min(v.len());
            self.info_list[section].push(InfoData {
                tag: TAG_NONE as u16,
                format: TAG_FMT_UNDEFINED as u16,
                length: length as u32,
                name: name.as_bytes().to_vec(),
                value: Val::S(v[..n].to_vec()),
            });
            self.sections_found |= 1 << section;
        }
    }

    // -- file sections --------------------------------------------------------

    /// exif_file_sections_add(): the section index and its buffer.
    fn sections_add(&mut self, typ: i32, size: u64) -> (usize, Ptr) {
        let p = self.alloc(vec![0u8; size as usize]);
        self.sections.push((typ, p.b));
        (self.sections.len() - 1, p)
    }

    // -- thumbnails -------------------------------------------------------------

    /// exif_ifd_make_value()
    fn ifd_make_value(d: &InfoData, mi: bool) -> Vec<u8> {
        let byte_count = BYTES_PER_FORMAT
            .get(d.format as usize)
            .copied()
            .unwrap_or(0) as u64
            * d.length as u64;
        let mut out = vec![0u8; byte_count.max(4) as usize];
        if d.length == 0 {
            return out;
        }
        let format = d.format as u32;
        let put16 = |o: &mut [u8], v: u32| {
            if mi {
                o[0] = (v >> 8) as u8;
                o[1] = v as u8;
            } else {
                o[1] = (v >> 8) as u8;
                o[0] = v as u8;
            }
        };
        let put32 = |o: &mut [u8], v: u32| {
            let b = if mi { v.to_be_bytes() } else { v.to_le_bytes() };
            o[..4].copy_from_slice(&b);
        };
        if format == TAG_FMT_UNDEFINED
            || format == TAG_FMT_STRING
            || (byte_count > 1 && (format == TAG_FMT_BYTE || format == TAG_FMT_SBYTE))
        {
            if let Val::S(s) = &d.value {
                let n = (byte_count as usize).min(s.len());
                out[..n].copy_from_slice(&s[..n]);
            }
            return out;
        }
        if format == TAG_FMT_BYTE || format == TAG_FMT_SBYTE {
            if let Val::S(s) = &d.value {
                out[0] = s.first().copied().unwrap_or(0);
            }
            return out;
        }
        let items: Vec<&Val> = match &d.value {
            Val::List(l) if d.length > 1 => l.iter().collect(),
            v => vec![v],
        };
        let mut o = 0usize;
        for v in items {
            match (format, v) {
                (TAG_FMT_USHORT, Val::U(x)) => {
                    put16(&mut out[o..], *x);
                    o += 2;
                }
                (TAG_FMT_ULONG, Val::U(x)) => {
                    put32(&mut out[o..], *x);
                    o += 4;
                }
                (TAG_FMT_SSHORT, Val::I(x)) => {
                    put16(&mut out[o..], *x as u32);
                    o += 2;
                }
                (TAG_FMT_SLONG, Val::I(x)) => {
                    put32(&mut out[o..], *x as u32);
                    o += 4;
                }
                (TAG_FMT_URATIONAL, Val::Ur(n, dn)) => {
                    put32(&mut out[o..], *n);
                    put32(&mut out[o + 4..], *dn);
                    o += 8;
                }
                (TAG_FMT_SRATIONAL, Val::Sr(n, dn)) => {
                    put32(&mut out[o..], *n as u32);
                    put32(&mut out[o + 4..], *dn as u32);
                    o += 8;
                }
                (TAG_FMT_SINGLE, Val::F(f)) => {
                    out[o..o + 4].copy_from_slice(&f.to_le_bytes());
                    o += 4;
                }
                (TAG_FMT_DOUBLE, Val::D(f)) => {
                    out[o..o + 8].copy_from_slice(&f.to_le_bytes());
                    o += 8;
                }
                _ => {}
            }
            if o >= out.len() {
                break;
            }
        }
        out
    }

    /// exif_thumbnail_build()
    fn thumbnail_build(&mut self) {
        if !self.read_thumbnail || self.thumb.offset == 0 || self.thumb.size == 0 {
            return;
        }
        match self.thumb.filetype {
            IMAGE_FILETYPE_TIFF_II | IMAGE_FILETYPE_TIFF_MM => {}
            _ => return,
        }
        let mi = self.mi();
        let list = &self.info_list[SECTION_THUMBNAIL];
        let mut new_size: u64 = 8 + 2 + list.len() as u64 * 12 + 4;
        let mut new_value = new_size;
        for d in list {
            let bc = BYTES_PER_FORMAT
                .get(d.format as usize)
                .copied()
                .unwrap_or(0) as i64
                * d.length as i64;
            let bc = bc as i32;
            if bc > 4 {
                new_size += bc as u64;
            }
        }
        let new_move = new_size;
        let old = self.thumb.data.take().unwrap_or_default();
        let old_size = self.thumb.size as usize;
        let mut data = vec![0u8; old_size + new_size as usize];
        let n = old_size.min(old.len());
        data[new_move as usize..new_move as usize + n].copy_from_slice(&old[..n]);
        self.thumb.size += new_size;
        let put16 = |o: &mut [u8], v: u32| {
            if mi {
                o[0] = (v >> 8) as u8;
                o[1] = v as u8;
            } else {
                o[1] = (v >> 8) as u8;
                o[0] = v as u8;
            }
        };
        let put32 = |o: &mut [u8], v: u32| {
            let b = if mi { v.to_be_bytes() } else { v.to_le_bytes() };
            o[..4].copy_from_slice(&b);
        };
        if mi {
            data[..8].copy_from_slice(b"MM\x00\x2a\x00\x00\x00\x08");
        } else {
            data[..8].copy_from_slice(b"II\x2a\x00\x08\x00\x00\x00");
        }
        let mut nd = 8usize;
        put16(&mut data[nd..], list.len() as u32);
        nd += 2;
        for d in list {
            let bc = (BYTES_PER_FORMAT
                .get(d.format as usize)
                .copied()
                .unwrap_or(0) as i64
                * d.length as i64) as i32;
            let tag = d.tag as i32;
            if tag == TAG_STRIP_OFFSETS || tag == TAG_JPEG_INTERCHANGE_FORMAT {
                put16(&mut data[nd..], d.tag as u32);
                put16(&mut data[nd + 2..], TAG_FMT_ULONG);
                put32(&mut data[nd + 4..], 1);
                put32(&mut data[nd + 8..], new_move as u32);
            } else {
                put16(&mut data[nd..], d.tag as u32);
                put16(&mut data[nd + 2..], d.format as u32);
                put32(&mut data[nd + 4..], d.length);
                let v = Self::ifd_make_value(d, mi);
                if bc <= 4 {
                    data[nd + 8..nd + 12].copy_from_slice(&v[..4]);
                } else {
                    put32(&mut data[nd + 8..], new_value as u32);
                    let start = new_value as usize;
                    let len = (bc as usize).min(v.len());
                    if start + len <= data.len() {
                        data[start..start + len].copy_from_slice(&v[..len]);
                    }
                    new_value += bc as u64;
                }
            }
            nd += 12;
        }
        data[nd..nd + 4].fill(0);
        self.thumb.data = Some(data);
    }

    /// exif_thumbnail_extract()
    fn thumbnail_extract(&mut self, vm: &mut Vm, info: &Info) {
        if self.thumb.data.is_some() {
            self.warn(vm, "Multiple possible thumbnails");
            return;
        }
        if !self.read_thumbnail {
            return;
        }
        if self.thumb.size >= 65536 || self.thumb.size == 0 || self.thumb.offset == 0 {
            self.warn(vm, "Illegal thumbnail size/offset");
            return;
        }
        let Some(p) = info.try_get(self.thumb.offset, self.thumb.size) else {
            self.warn(vm, "Thumbnail goes IFD boundary or end of file reached");
            return;
        };
        self.thumb.data = Some(self.bytes(p, self.thumb.size));
        self.thumbnail_build();
    }

    /// exif_scan_thumbnail()
    pub fn scan_thumbnail(&mut self, vm: &mut Vm) -> bool {
        let size = self.thumb.size as usize;
        let data = match &self.thumb.data {
            Some(d) if size >= 4 => d.clone(),
            _ => return false,
        };
        let at = |i: usize| data.get(i).copied().unwrap_or(0);
        if at(0) != 0xFF || at(1) != 0xD8 || at(2) != 0xFF {
            if self.thumb.width == 0 && self.thumb.height == 0 {
                self.warn(vm, "Thumbnail is not a JPEG image");
            }
            return false;
        }
        let mut length = 2usize;
        let mut pos = 0usize;
        loop {
            pos += length;
            if pos >= size {
                return false;
            }
            let mut c = at(pos);
            pos += 1;
            if pos >= size {
                return false;
            }
            if c != 0xFF {
                return false;
            }
            let mut n = 8i32;
            loop {
                c = at(pos);
                pos += 1;
                if c != 0xFF {
                    break;
                }
                let go = n != 0;
                n -= 1;
                if !go {
                    break;
                }
                if pos + 3 >= size {
                    return false;
                }
            }
            if c == 0xFF {
                return false;
            }
            let marker = c as i32;
            if pos >= size {
                return false;
            }
            length = ((at(pos) as usize) << 8) | at(pos + 1) as usize;
            if length > size || pos >= size - length {
                return false;
            }
            if is_sof(marker) {
                if length < 8 || size - 8 < pos {
                    return false;
                }
                self.thumb.height = ((at(pos + 3) as u64) << 8) | at(pos + 4) as u64;
                self.thumb.width = ((at(pos + 5) as u64) << 8) | at(pos + 6) as u64;
                return true;
            }
            if marker == M_SOS || marker == M_EOI {
                self.warn(vm, "Could not compute size of thumbnail");
                return false;
            }
        }
    }

    // -- comments / strings -------------------------------------------------------

    /// zend_multibyte_encoding_converter() with mbstring's encodings (None:
    /// an encoding is unknown).
    fn convert(data: &[u8], to: &[u8], from: &[u8]) -> Option<Vec<u8>> {
        use goro_ext_mbstring::mbfl;
        let to = mbfl::name2encoding(to)?;
        let from = mbfl::name2encoding(from)?;
        let (subst, mode) = goro_ext_mbstring::state::MBSTRG.with(|s| {
            let s = s.borrow();
            if s.initialized {
                (
                    s.current_filter_illegal_substchar,
                    s.current_filter_illegal_mode,
                )
            } else {
                ('?' as u32, mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR)
            }
        });
        Some(mbfl::fast_convert(data, from, to, subst, mode).0)
    }

    /// exif_process_user_comment(): (comment, its length, encoding)
    fn process_user_comment(
        &mut self,
        p: Ptr,
        byte_count: i32,
    ) -> (Option<Vec<u8>>, i32, Option<Vec<u8>>) {
        let mut p = p;
        let mut bc = byte_count;
        let mut encoding = None;
        if bc >= 8 {
            let head = self.bytes(p, 8);
            if &head == b"UNICODE\0" {
                encoding = Some(c_str(&head));
                p = p.add(8);
                bc -= 8;
                let decode: Vec<u8>;
                let two = self.bytes(p, 2);
                if bc >= 2 && two == b"\xFE\xFF" {
                    decode = b"UCS-2BE".to_vec();
                    p = p.add(2);
                    bc -= 2;
                } else if bc >= 2 && two == b"\xFF\xFE" {
                    decode = b"UCS-2LE".to_vec();
                    p = p.add(2);
                    bc -= 2;
                } else if self.mi() {
                    decode = self.decode_unicode_be.clone();
                } else {
                    decode = self.decode_unicode_le.clone();
                }
                let data = self.bytes(p, bc.max(0) as u64);
                return match Self::convert(&data, &self.encode_unicode, &decode) {
                    Some(r) => {
                        let n = r.len() as i32;
                        (Some(r), n, encoding)
                    }
                    None => {
                        let (r, n) = string_raw(&data);
                        (r, n, encoding)
                    }
                };
            } else if &head == b"ASCII\0\0\0" {
                encoding = Some(c_str(&head));
                p = p.add(8);
                bc -= 8;
            } else if &head == b"JIS\0\0\0\0\0" {
                encoding = Some(c_str(&head));
                p = p.add(8);
                bc -= 8;
                let from = if self.mi() {
                    self.decode_jis_be.clone()
                } else {
                    self.decode_jis_le.clone()
                };
                let data = self.bytes(p, bc.max(0) as u64);
                return match Self::convert(&data, &self.encode_jis, &from) {
                    Some(r) => {
                        let n = r.len() as i32;
                        (Some(r), n, encoding)
                    }
                    None => {
                        let (r, n) = string_raw(&data);
                        (r, n, encoding)
                    }
                };
            } else if head == [0u8; 8] {
                encoding = Some(b"UNDEFINED".to_vec());
                p = p.add(8);
                bc -= 8;
            }
        }
        // Olympus pads with trailing spaces: remove these first.
        if bc > 0 {
            let mut a = bc - 1;
            while a != 0 && self.rd(p, a as i64) == b' ' {
                let idx = p.o + a as i64;
                if idx >= 0 {
                    if let Some(c) = self.mem[p.b].get_mut(idx as usize) {
                        *c = 0;
                    }
                }
                a -= 1;
            }
        }
        // exif_process_string()
        let n = self.strnlen(p, bc.max(0) as u64);
        let s = self.bytes(p, n);
        let len = s.len() as i32;
        (Some(s), len, encoding)
    }

    /// exif_process_unicode()
    fn process_unicode(&self, tag: u32, p: Ptr, byte_count: i32) -> XpField {
        let data = self.bytes(p, byte_count.max(0) as u64);
        let from = if self.mi() {
            &self.decode_unicode_be
        } else {
            &self.decode_unicode_le
        };
        let value = match Self::convert(&data, &self.encode_unicode, from) {
            Some(r) => Some(r),
            None => string_raw(&data).0,
        };
        XpField { tag, value }
    }

    // -- IFDs -------------------------------------------------------------------------

    /// exif_process_IFD_in_MAKERNOTE()
    fn ifd_in_makernote(
        &mut self,
        vm: &mut Vm,
        value_ptr: Ptr,
        value_len: i32,
        info: &Info,
        displacement: u64,
    ) -> bool {
        let section = SECTION_MAKERNOTE;
        let mut found = None;
        for mn in MAKER_NOTES {
            match &self.make {
                Some(m) if m.as_slice() == mn.make => {}
                _ => continue,
            }
            if let Some(id) = mn.id {
                if value_len >= id.len() as i32 {
                    // strncmp(): stops at the first NUL of either
                    let v = self.bytes(value_ptr, id.len() as u64);
                    let mut differ = false;
                    for i in 0..id.len() {
                        if v[i] != id[i] {
                            differ = true;
                            break;
                        }
                        if v[i] == 0 {
                            break;
                        }
                    }
                    if differ {
                        continue;
                    }
                }
            }
            found = Some(mn);
            break;
        }
        let Some(mn) = found else {
            // unknown manufacturer, not an error, use it as a string
            return true;
        };
        if value_len < 2 || mn.offset >= value_len - 1 {
            self.warn(
                vm,
                &format!(
                    "IFD data too short: 0x{:04X} offset 0x{:04X}",
                    value_len, mn.offset
                ),
            );
            return true;
        }
        let Some(table) = mn.table else {
            return true;
        };
        let dir_start = value_ptr.add(mn.offset as i64);
        self.sections_found |= FOUND_MAKERNOTE;
        let old_mi = self.motorola_intel;
        match mn.order {
            MnOrder::Intel => self.motorola_intel = 0,
            MnOrder::Motorola => self.motorola_intel = 1,
            MnOrder::Normal => {}
        }
        let mut n = self.g16(dir_start, self.mi()) as i32;
        if 2 + n * 12 > value_len {
            self.error(
                vm,
                Level::Notice,
                "Potentially invalid endianness, trying again with different endianness before imminent failure.",
            );
            self.motorola_intel = if self.motorola_intel == 0 { 1 } else { 0 };
            n = self.g16(dir_start, self.mi()) as i32;
        }
        if 2 + n * 12 > value_len {
            self.warn(
                vm,
                &format!(
                    "Illegal IFD size: 2 + 0x{:04X}*12 = 0x{:04X} > 0x{:04X}",
                    n,
                    2 + n * 12,
                    value_len
                ),
            );
            return false;
        }
        if (mn.offset as i64) > (value_len - (2 + n * 12)) as i64 {
            self.warn(
                vm,
                &format!(
                    "Illegal IFD size: 0x{:04X} > 0x{:04X}",
                    mn.offset + 2 + n * 12,
                    value_len
                ),
            );
            return false;
        }
        let new_info;
        let info = match mn.mode {
            MnOffset::Maker => {
                new_info = Info::new(value_ptr, value_ptr, value_len as u64);
                &new_info
            }
            MnOffset::Normal => info,
        };
        for de in 0..n {
            let off = 2 + 12 * de as i64;
            if !self.ifd_tag(
                vm,
                dir_start.add(off),
                info,
                displacement,
                section,
                false,
                table,
            ) {
                return false;
            }
        }
        self.motorola_intel = old_mi;
        true
    }

    /// exif_process_IFD_TAG()
    #[allow(clippy::too_many_arguments)]
    fn ifd_tag(
        &mut self,
        vm: &mut Vm,
        dir_entry: Ptr,
        info: &Info,
        displacement: u64,
        section: usize,
        read_next: bool,
        table: Table,
    ) -> bool {
        let c = self.ifd_count;
        self.ifd_count += 1;
        if c > MAX_IFD_TAGS {
            self.warn(vm, "corrupt EXIF header: maximum IFD tag count reached");
            return false;
        }
        if self.ifd_nesting_level > MAX_IFD_NESTING_LEVEL {
            self.warn(
                vm,
                "corrupt EXIF header: maximum directory nesting level reached",
            );
            return false;
        }
        self.ifd_nesting_level += 1;
        let r = self.ifd_tag_impl(vm, dir_entry, info, displacement, section, read_next, table);
        self.ifd_nesting_level -= 1;
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn ifd_tag_impl(
        &mut self,
        vm: &mut Vm,
        dir_entry: Ptr,
        info: &Info,
        displacement: u64,
        section: usize,
        read_next: bool,
        table: Table,
    ) -> bool {
        let mi = self.mi();
        let tag = self.g16(dir_entry, mi);
        let mut format = self.g16(dir_entry.add(2), mi);
        let components = self.g32(dir_entry.add(4), mi);
        if format == 0 || format > NUM_FORMATS {
            self.warn(
                vm,
                &format!(
                    "Process tag(x{:04X}={}): Illegal format code 0x{:04X}, suppose BYTE",
                    tag,
                    tagname_debug(tag, table),
                    format
                ),
            );
            format = TAG_FMT_BYTE;
        }
        let bcs = components as i64 * BYTES_PER_FORMAT[format as usize] as i64;
        if bcs > i32::MAX as i64 {
            self.warn(
                vm,
                &format!(
                    "Process tag(x{:04X}={}): Illegal byte_count",
                    tag,
                    tagname_debug(tag, table)
                ),
            );
            return false;
        }
        let byte_count = bcs as u64;
        let value_ptr;
        if byte_count > 4 {
            let offset_val = self.g32(dir_entry.add(8), mi) as u64;
            match info.try_get(offset_val, byte_count) {
                Some(p) => value_ptr = p,
                None => {
                    let fs = self.file_size;
                    if byte_count > fs
                        || offset_val > fs - byte_count
                        || !matches!(
                            self.file_type,
                            IMAGE_FILETYPE_TIFF_II | IMAGE_FILETYPE_TIFF_MM | IMAGE_FILETYPE_JPEG
                        )
                    {
                        self.warn(
                            vm,
                            &format!(
                                "Process tag(x{:04X}={}): Illegal pointer offset(x{:04X} + x{:04X} = x{:04X} > x{:04X})",
                                tag,
                                tagname_debug(tag, table),
                                offset_val as u32,
                                byte_count as u32,
                                offset_val.wrapping_add(byte_count) as u32,
                                fs as u32
                            ),
                        );
                        return false;
                    }
                    let fpos = self.stream.tell();
                    let target = displacement.wrapping_add(offset_val);
                    self.stream.seek(target);
                    let fgot = self.stream.tell();
                    if fgot != target {
                        self.warn(
                            vm,
                            &format!(
                                "Wrong file pointer: 0x{:08X} != 0x{:08X}",
                                fgot as u32, target as u32
                            ),
                        );
                        return false;
                    }
                    let got = self.stream.read(byte_count);
                    self.stream.seek(fpos);
                    if got.len() as u64 != byte_count {
                        self.warn(vm, "Unexpected end of file reached");
                        return false;
                    }
                    value_ptr = self.alloc(got);
                }
            }
        } else {
            value_ptr = dir_entry.add(8);
        }
        self.sections_found |= FOUND_ANY_TAG;

        macro_rules! require_non_empty {
            () => {
                if byte_count == 0 {
                    self.warn(
                        vm,
                        &format!(
                            "Process tag(x{:04X}={}): Cannot be empty",
                            tag,
                            tagname_debug(tag, table)
                        ),
                    );
                    return false;
                }
            };
        }

        let unsigned = |f: u32| match f {
            TAG_FMT_SBYTE => TAG_FMT_BYTE,
            TAG_FMT_SRATIONAL => TAG_FMT_URATIONAL,
            TAG_FMT_SSHORT => TAG_FMT_USHORT,
            TAG_FMT_SLONG => TAG_FMT_ULONG,
            f => f,
        };
        let itag = tag as i32;
        if section == SECTION_THUMBNAIL {
            if self.thumb.data.is_none() {
                require_non_empty!();
                match itag {
                    TAG_IMAGEWIDTH | TAG_COMP_IMAGE_WIDTH => {
                        self.thumb.width = self.convert_any_to_int(value_ptr, unsigned(format), mi);
                    }
                    TAG_IMAGEHEIGHT | TAG_COMP_IMAGE_HEIGHT => {
                        self.thumb.height =
                            self.convert_any_to_int(value_ptr, unsigned(format), mi);
                    }
                    TAG_STRIP_OFFSETS | TAG_JPEG_INTERCHANGE_FORMAT => {
                        self.thumb.offset =
                            self.convert_any_to_int(value_ptr, unsigned(format), mi);
                    }
                    TAG_STRIP_BYTE_COUNTS => {
                        if matches!(
                            self.file_type,
                            IMAGE_FILETYPE_TIFF_II | IMAGE_FILETYPE_TIFF_MM
                        ) {
                            self.thumb.filetype = self.file_type;
                        } else {
                            self.thumb.filetype = IMAGE_FILETYPE_TIFF_MM;
                        }
                        self.thumb.size = self.convert_any_to_int(value_ptr, unsigned(format), mi);
                    }
                    TAG_JPEG_INTERCHANGE_FORMAT_LEN => {
                        if self.thumb.filetype == IMAGE_FILETYPE_UNKNOWN {
                            self.thumb.filetype = IMAGE_FILETYPE_JPEG;
                            self.thumb.size =
                                self.convert_any_to_int(value_ptr, unsigned(format), mi);
                        }
                    }
                    _ => {}
                }
            }
        } else if section == SECTION_IFD0 || section == SECTION_EXIF {
            match itag {
                TAG_COPYRIGHT => {
                    if byte_count > 1 {
                        let length = self.strnlen(value_ptr, byte_count);
                        if length > 0 {
                            if length < byte_count - 1 {
                                let ph = self.bytes(value_ptr, length);
                                let ed = c_str(&self.bytes(
                                    value_ptr.add(length as i64 + 1),
                                    byte_count - length - 1,
                                ));
                                let mut c = ph.clone();
                                c.extend_from_slice(b", ");
                                c.extend_from_slice(&ed);
                                self.copyright_photographer = Some(ph);
                                self.copyright_editor = Some(ed);
                                self.copyright = Some(c);
                            } else {
                                self.copyright = Some(c_str(&self.bytes(value_ptr, byte_count)));
                            }
                        }
                    }
                }
                TAG_USERCOMMENT => {
                    self.user_comment = None;
                    self.user_comment_encoding = None;
                    let (c, n, e) = self.process_user_comment(value_ptr, byte_count as i32);
                    self.user_comment = c;
                    self.user_comment_length = n;
                    self.user_comment_encoding = e;
                }
                TAG_XP_TITLE | TAG_XP_COMMENTS | TAG_XP_AUTHOR | TAG_XP_KEYWORDS
                | TAG_XP_SUBJECT => {
                    self.sections_found |= FOUND_WINXP;
                    let f = self.process_unicode(tag, value_ptr, byte_count as i32);
                    self.xp_fields.push(f);
                }
                TAG_FNUMBER => {
                    require_non_empty!();
                    self.aperture_fnumber = self.convert_any_format(value_ptr, format, mi) as f32;
                }
                TAG_APERTURE | TAG_MAX_APERTURE => {
                    if self.aperture_fnumber == 0.0 {
                        require_non_empty!();
                        let x = self.convert_any_format(value_ptr, format, mi);
                        self.aperture_fnumber = ((x * (2.0f32.ln() as f64) * 0.5) as f32).exp();
                    }
                }
                TAG_SHUTTERSPEED => {
                    if self.exposure_time == 0.0 {
                        require_non_empty!();
                        let x = self.convert_any_format(value_ptr, format, mi);
                        self.exposure_time = ((-x * (2.0f32.ln() as f64)) as f32).exp();
                    }
                }
                TAG_EXPOSURETIME => {
                    self.exposure_time = -1.0;
                }
                TAG_COMP_IMAGE_WIDTH => {
                    require_non_empty!();
                    self.exif_image_width =
                        self.convert_any_to_int(value_ptr, unsigned(format), mi);
                }
                TAG_FOCALPLANE_X_RES => {
                    require_non_empty!();
                    self.focalplane_x_res = self.convert_any_format(value_ptr, format, mi);
                }
                TAG_SUBJECT_DISTANCE => {
                    require_non_empty!();
                    self.distance = self.convert_any_format(value_ptr, format, mi) as f32;
                }
                TAG_FOCALPLANE_RESOLUTION_UNIT => {
                    require_non_empty!();
                    match self.convert_any_to_int(value_ptr, format, mi) {
                        1 | 2 => self.focalplane_units = 25.4,
                        3 => self.focalplane_units = 10.0,
                        4 => self.focalplane_units = 1.0,
                        5 => self.focalplane_units = 0.001,
                        _ => {}
                    }
                }
                TAG_SUB_IFD => {
                    if format == TAG_FMT_IFD {
                        self.error(vm, Level::Notice, "Skip SUB IFD");
                    }
                }
                TAG_MAKE => {
                    self.make = Some(c_str(&self.bytes(value_ptr, byte_count)));
                }
                TAG_MODEL => {
                    self.model = Some(c_str(&self.bytes(value_ptr, byte_count)));
                }
                TAG_MAKER_NOTE => {
                    if !self.ifd_in_makernote(vm, value_ptr, byte_count as i32, info, displacement)
                    {
                        return false;
                    }
                }
                TAG_EXIF_IFD_POINTER | TAG_GPS_IFD_POINTER | TAG_INTEROP_IFD_POINTER => {
                    if read_next {
                        require_non_empty!();
                        let sub = match itag {
                            TAG_EXIF_IFD_POINTER => {
                                self.sections_found |= FOUND_EXIF;
                                SECTION_EXIF
                            }
                            TAG_GPS_IFD_POINTER => {
                                self.sections_found |= FOUND_GPS;
                                SECTION_GPS
                            }
                            _ => {
                                self.sections_found |= FOUND_INTEROP;
                                SECTION_INTEROP
                            }
                        };
                        let offset_val = self.g32(value_ptr, self.mi()) as u64;
                        let Some(sub_start) = info.try_get(offset_val, 0) else {
                            self.warn(vm, "Illegal IFD Pointer");
                            return false;
                        };
                        if !self.ifd_in_jpeg(vm, sub_start, info, displacement, sub, itag) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        let name = tagname_key(tag, table);
        self.add_tag_at(
            vm,
            section,
            name,
            itag,
            format,
            components as u64,
            value_ptr,
            byte_count,
        );
        true
    }

    /// exif_process_IFD_in_JPEG()
    fn ifd_in_jpeg(
        &mut self,
        vm: &mut Vm,
        dir_start: Ptr,
        info: &Info,
        displacement: u64,
        section: usize,
        tag: i32,
    ) -> bool {
        self.sections_found |= FOUND_IFD0;
        if !info.contains(dir_start, 2) {
            self.warn(vm, "Illegal IFD size");
            return false;
        }
        let n = self.g16(dir_start, self.mi()) as i32;
        if !info.contains(dir_start.add(2), n as i64 * 12) {
            self.warn(
                vm,
                &format!(
                    "Illegal IFD size: x{:04X} + 2 + x{:04X}*12 = x{:04X} > x{:04X}",
                    (dir_start.o + 2 - info.start) as i32,
                    n,
                    (dir_start.o + 2 + n as i64 * 12 - info.start) as i32,
                    (info.end - info.start) as u32
                ),
            );
            return false;
        }
        for de in 0..n {
            if !self.ifd_tag(
                vm,
                dir_start.add(2 + 12 * de as i64),
                info,
                displacement,
                section,
                true,
                get_tag_table(section),
            ) {
                return false;
            }
        }
        // Ignore IFD2 if it purportedly exists
        if section == SECTION_THUMBNAIL {
            return true;
        }
        let next_p = dir_start.add(2 + n as i64 * 12);
        if !info.contains(next_p, 4) {
            self.warn(vm, "Illegal IFD size");
            return false;
        }
        let mut next: i32 = 0;
        if tag != TAG_EXIF_IFD_POINTER && tag != TAG_GPS_IFD_POINTER {
            next = self.g32(next_p, self.mi()) as i32;
        }
        if next != 0 {
            let Some(nds) = info.try_get(next as i64 as u64, 0) else {
                self.warn(vm, "Illegal IFD offset");
                return false;
            };
            if self.ifd_in_jpeg(vm, nds, info, displacement, SECTION_THUMBNAIL, 0) {
                if self.thumb.filetype != IMAGE_FILETYPE_UNKNOWN
                    && self.thumb.size != 0
                    && self.thumb.offset != 0
                    && self.read_thumbnail
                {
                    self.thumbnail_extract(vm, info);
                }
                return true;
            }
            return false;
        }
        true
    }

    /// exif_process_TIFF_in_JPEG()
    fn tiff_in_jpeg(&mut self, vm: &mut Vm, buf: Ptr, length: u64, displacement: u64) {
        if length < 2 {
            self.warn(vm, "Missing TIFF alignment marker");
            return;
        }
        let a = (self.rd(buf, 0), self.rd(buf, 1));
        if a == (b'I', b'I') {
            self.motorola_intel = 0;
        } else if a == (b'M', b'M') {
            self.motorola_intel = 1;
        } else {
            self.warn(vm, "Invalid TIFF alignment marker");
            return;
        }
        if length < 8 {
            self.warn(vm, "Invalid TIFF start (1)");
            return;
        }
        let v2a = self.g16(buf.add(2), self.mi());
        let off = self.g32(buf.add(4), self.mi());
        if v2a != 0x2a || off < 8 {
            self.warn(vm, "Invalid TIFF start (1)");
            return;
        }
        if off as u64 > length {
            self.warn(vm, "Invalid IFD start");
            return;
        }
        self.sections_found |= FOUND_IFD0;
        let info = Info::new(buf, buf, length);
        self.ifd_in_jpeg(
            vm,
            buf.add(off as i64),
            &info,
            displacement,
            SECTION_IFD0,
            0,
        );
        if self.focalplane_x_res != 0.0 {
            self.ccd_width = (self.exif_image_width as f64 * self.focalplane_units
                / self.focalplane_x_res) as f32;
        }
    }

    /// exif_process_APP1()
    fn process_app1(&mut self, vm: &mut Vm, buf: Ptr, length: u64, displacement: u64) {
        if length <= 8 || self.bytes(buf.add(2), 6) != b"Exif\0\0" {
            self.warn(vm, "Incorrect APP1 Exif Identifier Code");
            return;
        }
        self.tiff_in_jpeg(vm, buf.add(8), length - 8, displacement + 8);
    }

    /// exif_process_APP12()
    fn process_app12(&mut self, vm: &mut Vm, buf: Ptr, length: u64) {
        let l1 = self.strnlen(buf.add(2), length - 2);
        if l1 > 0 {
            let v = self.bytes(buf.add(2), l1);
            self.add_tag(
                vm,
                SECTION_APP12,
                "Company",
                TAG_NONE,
                TAG_FMT_STRING,
                l1,
                Some(v),
                l1,
            );
            if length > 2 + l1 + 1 {
                let l2 = self.strnlen(buf.add(2 + l1 as i64 + 1), length - 2 - l1 - 1);
                let v = self.bytes(buf.add(2 + l1 as i64 + 1), l2);
                self.add_tag(
                    vm,
                    SECTION_APP12,
                    "Info",
                    TAG_NONE,
                    TAG_FMT_STRING,
                    l2,
                    Some(v),
                    l2,
                );
            }
        }
    }

    /// exif_scan_JPEG_header()
    fn scan_jpeg_header(&mut self, vm: &mut Vm) -> bool {
        let mut last_marker = M_PSEUDO;
        let mut comment_correction = 1;
        loop {
            if last_marker == M_COM && comment_correction != 0 {
                comment_correction = 2;
            }
            let mut marker;
            loop {
                marker = self.stream.getc();
                if marker == -1 {
                    self.warn(vm, "File structure corrupted");
                    return false;
                }
                if last_marker == M_COM && comment_correction > 0 {
                    if marker != 0xFF {
                        marker = 0xff;
                        comment_correction -= 1;
                    } else {
                        last_marker = M_PSEUDO;
                    }
                }
                if marker != 0xff {
                    break;
                }
            }
            if last_marker == M_COM && comment_correction == 0 {
                self.error(
                    vm,
                    Level::Notice,
                    "Image has corrupt COM section: some software set wrong length information",
                );
            }
            if last_marker == M_COM && comment_correction != 0 {
                return true;
            }
            let fpos = self.stream.tell();
            let lh = self.stream.getc();
            if lh == -1 {
                self.warn(vm, "File structure corrupted");
                return false;
            }
            let ll = self.stream.getc();
            if ll == -1 {
                self.warn(vm, "File structure corrupted");
                return false;
            }
            let itemlen = ((lh as u64) << 8) | ll as u64;
            if itemlen < 2 {
                self.warn(vm, "File structure corrupted");
                return false;
            }
            let (_, data) = self.sections_add(marker, itemlen);
            self.mem[data.b][0] = lh as u8;
            self.mem[data.b][1] = ll as u8;
            let got = self.stream.read(itemlen - 2);
            let gl = got.len() as u64;
            self.mem[data.b][2..2 + got.len()].copy_from_slice(&got);
            if gl != itemlen - 2 {
                self.warn(
                    vm,
                    &format!(
                        "Error reading from file: got=x{:04X}(={}) != itemlen-2=x{:04X}(={})",
                        gl as u32,
                        gl as i32,
                        (itemlen - 2) as u32,
                        (itemlen - 2) as i32
                    ),
                );
                return false;
            }
            match marker {
                M_SOS => {
                    if self.read_all {
                        let fpos = self.stream.tell();
                        let size = self.file_size.wrapping_sub(fpos);
                        let got = self.stream.read(size);
                        let ok = got.len() as u64 == size;
                        self.sections_add(M_PSEUDO, 0);
                        if !ok {
                            self.warn(vm, "Unexpected end of file reached");
                            return false;
                        }
                    }
                    return true;
                }
                M_EOI => {
                    self.warn(vm, "No image in jpeg!");
                    return self.sections_found & !FOUND_COMPUTED != 0;
                }
                M_COM => {
                    let v = self.bytes(data.add(2), itemlen - 2);
                    self.add_tag(
                        vm,
                        SECTION_COMMENT,
                        "Comment",
                        TAG_COMPUTED_VALUE,
                        TAG_FMT_STRING,
                        itemlen - 2,
                        Some(v),
                        itemlen - 2,
                    );
                }
                M_EXIF => {
                    if self.sections_found & FOUND_IFD0 == 0 {
                        self.process_app1(vm, data, itemlen, fpos);
                    }
                }
                M_APP12 => self.process_app12(vm, data, itemlen),
                m if is_sof(m) => {
                    if itemlen - 2 < 6 {
                        return false;
                    }
                    self.height = self.g16(data.add(3), true) as i32;
                    self.width = self.g16(data.add(5), true) as i32;
                    self.is_color = if self.rd(data, 7) == 3 { 1 } else { 0 };
                }
                _ => {}
            }
            last_marker = marker;
        }
    }

    /// exif_process_IFD_in_TIFF()
    fn ifd_in_tiff(&mut self, vm: &mut Vm, dir_offset: u64, section: usize) -> bool {
        let c = self.ifd_count;
        self.ifd_count += 1;
        if c > MAX_IFD_TAGS {
            return false;
        }
        if self.ifd_nesting_level > MAX_IFD_NESTING_LEVEL {
            return false;
        }
        self.ifd_nesting_level += 1;
        let r = self.ifd_in_tiff_impl(vm, dir_offset, section);
        self.ifd_nesting_level -= 1;
        r
    }

    fn read_thumbnail_from_file(&mut self, vm: &mut Vm) {
        self.stream.seek(self.thumb.offset);
        let got = self.stream.read(self.thumb.size);
        if got.len() as u64 != self.thumb.size {
            self.warn(vm, "Thumbnail goes IFD boundary or end of file reached");
            self.thumb.data = None;
        } else {
            self.thumb.data = Some(got);
            self.thumbnail_build();
        }
    }

    fn ifd_in_tiff_impl(&mut self, vm: &mut Vm, dir_offset: u64, section: usize) -> bool {
        let table = get_tag_table(section);
        let fs = self.file_size;
        if !(fs >= 2 && fs - 2 >= dir_offset) {
            self.warn(
                vm,
                &format!(
                    "Error in TIFF: filesize(x{:04X}) less than start of IFD dir(x{:04X})",
                    fs as u32,
                    dir_offset.wrapping_add(2) as u32
                ),
            );
            return false;
        }
        let (sn, sp) = self.sections_add(M_PSEUDO, 2);
        self.stream.seek(dir_offset);
        let got = self.stream.read(2);
        if got.len() != 2 {
            return false;
        }
        self.mem[sp.b][..2].copy_from_slice(&got);
        let mi = self.mi();
        let num_entries = self.g16(sp, mi) as u64;
        let dir_size = 2 + 12 * num_entries + 4;
        if !(fs >= dir_size && fs - dir_size >= dir_offset) {
            self.warn(
                vm,
                &format!(
                    "Error in TIFF: filesize(x{:04X}) less than size of IFD dir(x{:04X})",
                    fs as u32,
                    dir_offset.wrapping_add(dir_size) as u32
                ),
            );
            return false;
        }
        self.mem[sp.b].resize(dir_size as usize, 0);
        let got = self.stream.read(dir_size - 2);
        if got.len() as u64 != dir_size - 2 {
            return false;
        }
        self.mem[sp.b][2..dir_size as usize].copy_from_slice(&got);
        let next_offset = self.g32(sp.add(dir_size as i64 - 4), mi) as u64;
        let mut ifd_size = dir_size;
        let mut entry_value: u64 = 0;
        for i in 0..num_entries {
            let de = sp.add(2 + i as i64 * 12);
            let entry_tag = self.g16(de, mi) as i32;
            let mut entry_type = self.g16(de.add(2), mi);
            if entry_type > NUM_FORMATS {
                self.error(
                    vm,
                    Level::Notice,
                    &format!(
                        "Read from TIFF: tag(0x{:04X},{:>12}): Illegal format code 0x{:04X}, switching to BYTE",
                        entry_tag,
                        tagname_debug(entry_tag as u32, table),
                        entry_type
                    ),
                );
                entry_type = TAG_FMT_BYTE;
            }
            let entry_length =
                self.g32(de.add(4), mi)
                    .wrapping_mul(BYTES_PER_FORMAT[entry_type as usize]) as u64;
            if entry_length <= 4 {
                match entry_type {
                    TAG_FMT_USHORT => entry_value = self.g16(de.add(8), mi) as u64,
                    TAG_FMT_SSHORT => {
                        entry_value = self.g16(de.add(8), mi) as u16 as i16 as i64 as u64
                    }
                    TAG_FMT_ULONG | TAG_FMT_SLONG => entry_value = self.g32(de.add(8), mi) as u64,
                    _ => {}
                }
                match entry_tag {
                    TAG_IMAGEWIDTH | TAG_COMP_IMAGE_WIDTH => self.width = entry_value as i32,
                    TAG_IMAGEHEIGHT | TAG_COMP_IMAGE_HEIGHT => self.height = entry_value as i32,
                    TAG_PHOTOMETRIC_INTERPRETATION => match entry_value {
                        0 | 1 | 4 => self.is_color = 0,
                        2 | 3 | 5 | 6 | 8 => self.is_color = 1,
                        _ => {}
                    },
                    _ => {}
                }
            } else {
                let entry_offset = self.g32(de.add(8), mi) as u64;
                if entry_offset.wrapping_add(entry_length) > dir_offset.wrapping_add(ifd_size)
                    && entry_offset == dir_offset.wrapping_add(ifd_size)
                {
                    ifd_size = entry_offset
                        .wrapping_add(entry_length)
                        .wrapping_sub(dir_offset);
                }
            }
        }
        let cur_size = self.mem[sp.b].len() as u64;
        if !(fs >= cur_size && fs - cur_size >= dir_offset) {
            self.warn(
                vm,
                &format!(
                    "Error in TIFF: filesize(x{:04X}) less than size of IFD(x{:04X})",
                    fs as u32,
                    dir_offset.wrapping_add(cur_size) as u32
                ),
            );
            return false;
        }
        if ifd_size > dir_size {
            if fs < ifd_size || dir_offset > fs - ifd_size {
                self.warn(
                    vm,
                    &format!(
                        "Error in TIFF: filesize(x{:04X}) less than size of IFD(x{:04X} + x{:04X})",
                        fs as u32, dir_offset as u32, ifd_size as u32
                    ),
                );
                return false;
            }
            if sn >= self.sections.len() {
                self.warn(vm, "Illegal reallocating of undefined file section");
                return false;
            }
            self.mem[sp.b].resize(ifd_size as usize, 0);
            let got = self.stream.read(ifd_size - dir_size);
            self.mem[sp.b][dir_size as usize..dir_size as usize + got.len()].copy_from_slice(&got);
        }
        for i in 0..num_entries {
            let mi = self.mi();
            let de = sp.add(2 + i as i64 * 12);
            let entry_tag = self.g16(de, mi) as i32;
            if matches!(
                entry_tag,
                TAG_EXIF_IFD_POINTER | TAG_INTEROP_IFD_POINTER | TAG_GPS_IFD_POINTER | TAG_SUB_IFD
            ) {
                let sub_section = match entry_tag {
                    TAG_EXIF_IFD_POINTER => {
                        self.sections_found |= FOUND_EXIF;
                        SECTION_EXIF
                    }
                    TAG_GPS_IFD_POINTER => {
                        self.sections_found |= FOUND_GPS;
                        SECTION_GPS
                    }
                    TAG_INTEROP_IFD_POINTER => {
                        self.sections_found |= FOUND_INTEROP;
                        SECTION_INTEROP
                    }
                    _ => {
                        self.sections_found |= FOUND_THUMBNAIL;
                        SECTION_THUMBNAIL
                    }
                };
                let entry_offset = self.g32(de.add(8), mi) as u64;
                self.ifd_in_tiff(vm, entry_offset, sub_section);
                if section != SECTION_THUMBNAIL
                    && entry_tag == TAG_SUB_IFD
                    && self.thumb.filetype != IMAGE_FILETYPE_UNKNOWN
                    && self.thumb.size != 0
                    && self.thumb.offset != 0
                    && self.read_thumbnail
                    && self.thumb.data.is_none()
                {
                    self.read_thumbnail_from_file(vm);
                }
            } else {
                let base = Ptr {
                    b: sp.b,
                    o: -(dir_offset as i64),
                };
                let info = Info::new(base, sp, ifd_size);
                if !self.ifd_tag(vm, de, &info, 0, section, false, table) {
                    return false;
                }
            }
        }
        if next_offset != 0 && section != SECTION_THUMBNAIL {
            self.ifd_in_tiff(vm, next_offset, SECTION_THUMBNAIL);
            if self.thumb.data.is_none()
                && self.thumb.offset != 0
                && self.thumb.size != 0
                && self.read_thumbnail
            {
                self.read_thumbnail_from_file(vm);
            }
        }
        true
    }

    /// exif_scan_HEIF_header()
    fn scan_heif_header(&mut self, vm: &mut Vm, buf: &mut [u8; 16]) -> bool {
        let fs = self.file_size;
        let mut ret = false;
        let mut offset = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64;
        while fs - 16 > offset {
            if !self.stream.seek(offset) {
                break;
            }
            let got = self.stream.read(16);
            if got.len() != 16 {
                break;
            }
            buf.copy_from_slice(&got);
            let (hs, bsize, btype) = parse_box(buf);
            if bsize < hs as u64 {
                break;
            }
            if &btype == b"meta" {
                let limit = bsize - hs as u64;
                if limit < 36 {
                    break;
                }
                let remain = 16 - hs;
                let mut pos = (0u32, 0u32);
                let left = self.stream.data.len() as u64
                    - self.stream.pos.min(self.stream.data.len() as u64);
                if limit - remain as u64 <= left {
                    let mut data = buf[hs..16].to_vec();
                    let rest = self.stream.read(limit - remain as u64);
                    if rest.len() as u64 == limit - remain as u64 {
                        data.extend_from_slice(&rest);
                        pos = parse_meta(&data);
                    }
                }
                let (poff, psize) = (pos.0 as u64, pos.1 as u64);
                if psize >= 2 && psize < fs && fs - psize >= poff && self.stream.seek(poff + 2) {
                    let limit = psize - 2;
                    let got = self.stream.read(limit);
                    if got.len() as u64 == limit {
                        let p = self.alloc(got);
                        self.process_app1(vm, p, limit, poff + 2);
                        ret = true;
                    }
                }
                break;
            }
            match offset.checked_add(bsize) {
                Some(n) => offset = n,
                None => break,
            }
        }
        ret
    }

    /// exif_scan_FILE_header()
    pub fn scan_file_header(&mut self, vm: &mut Vm) -> bool {
        self.file_type = IMAGE_FILETYPE_UNKNOWN;
        if self.file_size < 2 {
            self.warn(vm, &format!("File too small ({})", self.file_size as i32));
            return false;
        }
        self.stream.seek(0);
        let mut fh = [0u8; 16];
        let got = self.stream.read(2);
        if got.len() != 2 {
            return false;
        }
        fh[..2].copy_from_slice(&got);
        if fh[0] == 0xff && fh[1] == M_SOI {
            self.file_type = IMAGE_FILETYPE_JPEG;
            if self.scan_jpeg_header(vm) {
                return true;
            }
            self.warn(vm, "Invalid JPEG file");
            return false;
        }
        if self.file_size >= 8 {
            let got = self.stream.read(6);
            if got.len() != 6 {
                return false;
            }
            fh[2..8].copy_from_slice(&got);
            if &fh[..4] == b"II\x2A\x00" || &fh[..4] == b"MM\x00\x2a" {
                let mm = fh[0] == b'M';
                self.file_type = if mm {
                    IMAGE_FILETYPE_TIFF_MM
                } else {
                    IMAGE_FILETYPE_TIFF_II
                };
                self.motorola_intel = mm as i32;
                self.sections_found |= FOUND_IFD0;
                let off = if mm {
                    u32::from_be_bytes([fh[4], fh[5], fh[6], fh[7]])
                } else {
                    u32::from_le_bytes([fh[4], fh[5], fh[6], fh[7]])
                };
                if self.ifd_in_tiff(vm, off as u64, SECTION_IFD0) {
                    return true;
                }
                self.warn(vm, "Invalid TIFF file");
                return false;
            }
            if self.file_size > 16 && &fh[4..8] == b"ftyp" {
                let got = self.stream.read(4);
                if got.len() == 4 {
                    fh[8..12].copy_from_slice(&got);
                    if matches!(&fh[8..12], b"heic" | b"heix" | b"mif1") {
                        if self.scan_heif_header(vm, &mut fh) {
                            self.file_type = IMAGE_FILETYPE_HEIF;
                            return true;
                        }
                        self.warn(vm, "Invalid HEIF file");
                        return false;
                    }
                }
            }
            self.warn(vm, "File not supported");
            return false;
        }
        false
    }
}

fn emit(vm: &mut Vm, level: Level, msg: &str) {
    let line = vm.current_line;
    match level {
        Level::Warning => vm.emit_warning_at(msg, line),
        Level::Notice => vm.emit_notice_at(msg, line),
    }
}

/// exif_process_string_raw(): the buffer and its "length" (byte_count + 1
/// for the NUL it adds).
fn string_raw(v: &[u8]) -> (Option<Vec<u8>>, i32) {
    if v.is_empty() {
        (None, 0)
    } else {
        let mut r = v.to_vec();
        r.push(0);
        let n = r.len() as i32;
        (Some(r), n)
    }
}

/// exif_isobmff_parse_box(): (header size, size, type)
fn parse_box(b: &[u8]) -> (usize, u64, [u8; 4]) {
    let g32 = |o: usize| {
        u32::from_be_bytes([
            b.get(o).copied().unwrap_or(0),
            b.get(o + 1).copied().unwrap_or(0),
            b.get(o + 2).copied().unwrap_or(0),
            b.get(o + 3).copied().unwrap_or(0),
        ])
    };
    let size = g32(0) as u64;
    let t = g32(4).to_be_bytes();
    if size != 1 {
        return (8, size, t);
    }
    let size = ((g32(8) as u64) << 32) | g32(12) as u64;
    (16, size, t)
}

/// exif_isobmff_parse_meta(): (offset, size) of the Exif item
#[allow(unused_assignments, clippy::needless_late_init)]
fn parse_meta(data: &[u8]) -> (u32, u32) {
    let end = data.len();
    let at = |i: usize| data.get(i).copied().unwrap_or(0);
    let g16 = |i: usize| ((at(i) as u32) << 8) | at(i + 1) as u32;
    let g32 = |i: usize| u32::from_be_bytes([at(i), at(i + 1), at(i + 2), at(i + 3)]);
    let mut pos = (0u32, 0u32);
    let mut exif_id: i64 = -1;
    let mut box_offset = 4usize;
    while box_offset + 16 < end {
        let (hs, bsize, btype) = parse_box(&data[box_offset..]);
        if bsize < hs as u64 {
            return pos;
        }
        let mut p = box_offset;
        let mut remain = (end - p) as u64;
        macro_rules! advance {
            ($n:expr) => {{
                let n = $n as u64;
                if remain < n {
                    return pos;
                }
                remain -= n;
                p += n as usize;
            }};
        }
        if &btype == b"iinf" {
            advance!(hs + 4);
            let version = at(p - 4);
            let item_count;
            if version < 2 {
                advance!(2);
                item_count = g16(p - 2) as i64;
            } else {
                advance!(4);
                item_count = g32(p - 4) as i32 as i64;
            }
            let mut i = 0;
            while i < item_count && p + 20 < end {
                let (ihs, isize, _) = parse_box(&data[p..]);
                if isize < ihs as u64 {
                    return pos;
                }
                if remain < (ihs + 12) as u64 {
                    return pos;
                }
                if &data[p + ihs + 8..p + ihs + 12] == b"Exif" {
                    exif_id = g16(p + ihs + 4) as i64;
                    break;
                }
                advance!(isize);
                i += 1;
            }
            if exif_id < 0 {
                break;
            }
        } else if &btype == b"iloc" {
            advance!(hs + 6);
            let version = at(p - 6);
            let item_count;
            if version < 2 {
                advance!(2);
                item_count = g16(p - 2) as i64;
            } else {
                advance!(4);
                item_count = g32(p - 4) as i32 as i64;
            }
            let mut i = 0;
            while i < item_count && p + 16 < end {
                if g16(p) as i64 == exif_id {
                    pos = (g32(p + 8), g32(p + 12));
                    break;
                }
                i += 1;
                p += 16;
            }
            break;
        }
        if ((end - 16 - box_offset) as u64) <= bsize {
            break;
        }
        box_offset += bsize as usize;
    }
    pos
}
