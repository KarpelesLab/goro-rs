//! A port of the parts of libzip (1.11) that ext/zip uses: archive
//! opening (central directory lookup and consistency checks), the entry
//! table with its pending changes, name lookup, entry reading, and the
//! archive rewrite done by zip_close().

#![allow(dead_code)]

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::codec;

// ---------------------------------------------------------------------------
// constants (zip.h, zipint.h)

pub const ZIP_CREATE: u32 = 1;
pub const ZIP_EXCL: u32 = 2;
pub const ZIP_CHECKCONS: u32 = 4;
pub const ZIP_TRUNCATE: u32 = 8;
pub const ZIP_RDONLY: u32 = 16;

pub const FL_NOCASE: u32 = 1;
pub const FL_NODIR: u32 = 2;
pub const FL_COMPRESSED: u32 = 4;
pub const FL_UNCHANGED: u32 = 8;
pub const FL_RECOMPRESS: u32 = 16;
pub const FL_ENCRYPTED: u32 = 32;
pub const FL_ENC_GUESS: u32 = 0;
pub const FL_ENC_RAW: u32 = 64;
pub const FL_ENC_STRICT: u32 = 128;
pub const FL_LOCAL: u32 = 256;
pub const FL_CENTRAL: u32 = 512;
pub const FL_FORCE_ZIP64: u32 = 1024;
pub const FL_ENC_UTF_8: u32 = 2048;
pub const FL_ENC_CP437: u32 = 4096;
pub const FL_OVERWRITE: u32 = 8192;
pub const FL_ENCODING_ALL: u32 = FL_ENC_GUESS | FL_ENC_CP437 | FL_ENC_UTF_8;

pub const AFL_RDONLY: u32 = 2;
pub const AFL_IS_TORRENTZIP: u32 = 4;
pub const AFL_WANT_TORRENTZIP: u32 = 8;
pub const AFL_CREATE_OR_KEEP_FILE_FOR_EMPTY_ARCHIVE: u32 = 16;

pub const CM_DEFAULT: i32 = -1;
pub const CM_REPLACED_DEFAULT: i32 = -2;
pub const CM_STORE: u16 = 0;
pub const CM_DEFLATE: u16 = 8;
pub const CM_BZIP2: u16 = 12;
pub const CM_LZMA: u16 = 14;
pub const CM_ZSTD: u16 = 93;
pub const CM_XZ: u16 = 95;
pub const CM_WINZIP_AES: u16 = 99;

pub const EM_NONE: u16 = 0;
pub const EM_TRAD_PKWARE: u16 = 1;
pub const EM_AES_128: u16 = 0x0101;
pub const EM_AES_192: u16 = 0x0102;
pub const EM_AES_256: u16 = 0x0103;
pub const EM_UNKNOWN: u16 = 0xffff;

pub const OPSYS_UNIX: u8 = 3;
pub const OPSYS_DEFAULT: u8 = OPSYS_UNIX;

pub const EXT_ATTRIB_DEFAULT: u32 = 0o100666 << 16;
pub const EXT_ATTRIB_DEFAULT_DIR: u32 = 0o040777 << 16;

pub const TORRENTZIP_SIGNATURE: &[u8] = b"TORRENTZIPPED-";
pub const TORRENTZIP_COMPRESSION_FLAGS: u32 = 0xffff;

const GPBF_ENCRYPTED: u16 = 0x0001;
const GPBF_DATA_DESCRIPTOR: u16 = 0x0008;
const GPBF_STRONG_ENCRYPTION: u16 = 0x0040;
const GPBF_ENCODING_UTF_8: u16 = 0x0800;
const GPBF_ALLOWED_MASK: u16 = 0x083e;

const EF_LOCAL: u16 = FL_LOCAL as u16;
const EF_CENTRAL: u16 = FL_CENTRAL as u16;
const EF_BOTH: u16 = EF_LOCAL | EF_CENTRAL;
const EF_UTF_8_COMMENT: u16 = 0x6375;
const EF_UTF_8_NAME: u16 = 0x7075;
const EF_WINZIP_AES: u16 = 0x9901;
const EF_ZIP64: u16 = 0x0001;

const DIRENT_COMP_METHOD: u32 = 0x0001;
const DIRENT_FILENAME: u32 = 0x0002;
const DIRENT_COMMENT: u32 = 0x0004;
const DIRENT_EXTRA_FIELD: u32 = 0x0008;
const DIRENT_ATTRIBUTES: u32 = 0x0010;
const DIRENT_LAST_MOD: u32 = 0x0020;
const DIRENT_ENCRYPTION_METHOD: u32 = 0x0040;
const DIRENT_PASSWORD: u32 = 0x0080;
const DIRENT_ALL: u32 = u32::MAX;

const CDENTRYSIZE: usize = 46;
const LENTRYSIZE: usize = 30;
const EOCDLEN: usize = 22;
const EOCD64LOCLEN: usize = 20;
const EOCD64LEN: usize = 56;
const CDBUFSIZE: usize = 0xffff + EOCDLEN + EOCD64LOCLEN;
const BUFSIZE: usize = 8192;

const LOCAL_MAGIC: &[u8] = b"PK\x03\x04";
const CENTRAL_MAGIC: &[u8] = b"PK\x01\x02";
const EOCD_MAGIC: &[u8] = b"PK\x05\x06";
const DATADES_MAGIC: &[u8] = b"PK\x07\x08";
const EOCD64LOC_MAGIC: &[u8] = b"PK\x06\x07";
const EOCD64_MAGIC: &[u8] = b"PK\x06\x06";

// error codes
pub const ER_OK: i32 = 0;
pub const ER_MULTIDISK: i32 = 1;
pub const ER_RENAME: i32 = 2;
pub const ER_CLOSE: i32 = 3;
pub const ER_SEEK: i32 = 4;
pub const ER_READ: i32 = 5;
pub const ER_WRITE: i32 = 6;
pub const ER_CRC: i32 = 7;
pub const ER_ZIPCLOSED: i32 = 8;
pub const ER_NOENT: i32 = 9;
pub const ER_EXISTS: i32 = 10;
pub const ER_OPEN: i32 = 11;
pub const ER_TMPOPEN: i32 = 12;
pub const ER_ZLIB: i32 = 13;
pub const ER_MEMORY: i32 = 14;
pub const ER_CHANGED: i32 = 15;
pub const ER_COMPNOTSUPP: i32 = 16;
pub const ER_EOF: i32 = 17;
pub const ER_INVAL: i32 = 18;
pub const ER_NOZIP: i32 = 19;
pub const ER_INTERNAL: i32 = 20;
pub const ER_INCONS: i32 = 21;
pub const ER_REMOVE: i32 = 22;
pub const ER_DELETED: i32 = 23;
pub const ER_ENCRNOTSUPP: i32 = 24;
pub const ER_RDONLY: i32 = 25;
pub const ER_NOPASSWD: i32 = 26;
pub const ER_WRONGPASSWD: i32 = 27;
pub const ER_OPNOTSUPP: i32 = 28;
pub const ER_INUSE: i32 = 29;
pub const ER_TELL: i32 = 30;
pub const ER_COMPRESSED_DATA: i32 = 31;
pub const ER_CANCELLED: i32 = 32;
pub const ER_DATA_LENGTH: i32 = 33;
pub const ER_NOT_ALLOWED: i32 = 34;
pub const ER_TRUNCATED_ZIP: i32 = 35;

const DETAIL_CDIR_OVERLAPS_EOCD: i32 = 1;
const DETAIL_COMMENT_LENGTH_INVALID: i32 = 2;
const DETAIL_CDIR_LENGTH_INVALID: i32 = 3;
const DETAIL_CDIR_ENTRY_INVALID: i32 = 4;
const DETAIL_CDIR_WRONG_ENTRIES_COUNT: i32 = 5;
const DETAIL_ENTRY_HEADER_MISMATCH: i32 = 6;
const DETAIL_EOCD_LENGTH_INVALID: i32 = 7;
const DETAIL_EOCD64_OVERLAPS_EOCD: i32 = 8;
const DETAIL_EOCD64_WRONG_MAGIC: i32 = 9;
const DETAIL_EOCD64_MISMATCH: i32 = 10;
const DETAIL_CDIR_INVALID: i32 = 11;
const DETAIL_VARIABLE_SIZE_OVERFLOW: i32 = 12;
const DETAIL_INVALID_UTF8_IN_FILENAME: i32 = 13;
const DETAIL_INVALID_UTF8_IN_COMMENT: i32 = 14;
const DETAIL_INVALID_ZIP64_EF: i32 = 15;
const DETAIL_INVALID_WINZIPAES_EF: i32 = 16;
const DETAIL_EF_TRAILING_GARBAGE: i32 = 17;
const DETAIL_INVALID_EF_LENGTH: i32 = 18;
const DETAIL_INVALID_FILE_LENGTH: i32 = 19;
const DETAIL_STORED_SIZE_MISMATCH: i32 = 20;
const DETAIL_DATA_DESCRIPTOR_MISMATCH: i32 = 21;
const DETAIL_EOCD64_LOCATOR_MISMATCH: i32 = 22;
const DETAIL_UTF8_FILENAME_MISMATCH: i32 = 23;
const DETAIL_UTF8_COMMENT_MISMATCH: i32 = 24;
const DETAIL_COMPRESSED_DATA_TRAILING_GARBAGE: i32 = 25;
const MAX_DETAIL_INDEX: i64 = 0x7fffff;

fn detail_with_index(error: i32, index: u64) -> i32 {
    let idx = if index as i64 > MAX_DETAIL_INDEX || index > i64::MAX as u64 {
        MAX_DETAIL_INDEX as i32
    } else {
        index as i32
    };
    (idx << 8) | error
}

fn add_index_to_detail(error: i32, index: u64) -> i32 {
    detail_with_index(error & 0xff, index)
}

const ENOENT: i32 = 2;
const EFBIG: i32 = 27;
const EOPNOTSUPP: i32 = 95;

/// Helpers on compression method values (ZIP_CM_IS_DEFAULT, ZIP_CM_ACTUAL).
pub struct ZipInt;

impl ZipInt {
    pub fn cm_is_default(m: i32) -> bool {
        m == CM_DEFAULT || m == CM_REPLACED_DEFAULT
    }

    pub fn cm_actual(m: i32) -> u16 {
        if Self::cm_is_default(m) {
            CM_DEFLATE
        } else {
            m as u16
        }
    }
}

// ---------------------------------------------------------------------------
// errors

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZipError {
    pub zip_err: i32,
    pub sys_err: i32,
}

#[derive(Clone, Copy, PartialEq)]
enum Et {
    None,
    Sys,
    Zlib,
    Libzip,
}

const ERR_STR: &[(Et, &str)] = &[
    (Et::None, "No error"),
    (Et::None, "Multi-disk zip archives not supported"),
    (Et::Sys, "Renaming temporary file failed"),
    (Et::Sys, "Closing zip archive failed"),
    (Et::Sys, "Seek error"),
    (Et::Sys, "Read error"),
    (Et::Sys, "Write error"),
    (Et::None, "CRC error"),
    (Et::None, "Containing zip archive was closed"),
    (Et::None, "No such file"),
    (Et::None, "File already exists"),
    (Et::Sys, "Can't open file"),
    (Et::Sys, "Failure to create temporary file"),
    (Et::Zlib, "Zlib error"),
    (Et::None, "Malloc failure"),
    (Et::None, "Entry has been changed"),
    (Et::None, "Compression method not supported"),
    (Et::None, "Premature end of file"),
    (Et::None, "Invalid argument"),
    (Et::None, "Not a zip archive"),
    (Et::None, "Internal error"),
    (Et::Libzip, "Zip archive inconsistent"),
    (Et::Sys, "Can't remove file"),
    (Et::None, "Entry has been deleted"),
    (Et::None, "Encryption method not supported"),
    (Et::None, "Read-only archive"),
    (Et::None, "No password provided"),
    (Et::None, "Wrong password provided"),
    (Et::None, "Operation not supported"),
    (Et::None, "Resource still in use"),
    (Et::Sys, "Tell error"),
    (Et::None, "Compressed data invalid"),
    (Et::None, "Operation cancelled"),
    (Et::None, "Unexpected length of data"),
    (Et::None, "Not allowed in torrentzip"),
    (Et::None, "Possibly truncated or corrupted zip archive"),
];

/// (is per-entry, description)
const ERR_DETAILS: &[(bool, &str)] = &[
    (false, "no detail"),
    (
        false,
        "central directory overlaps EOCD, or there is space between them",
    ),
    (false, "archive comment length incorrect"),
    (false, "central directory length invalid"),
    (true, "central header invalid"),
    (false, "central directory count of entries is incorrect"),
    (true, "local and central headers do not match"),
    (false, "wrong EOCD length"),
    (
        false,
        "EOCD64 overlaps EOCD, or there is space between them",
    ),
    (false, "EOCD64 magic incorrect"),
    (false, "EOCD64 and EOCD do not match"),
    (false, "invalid value in central directory"),
    (true, "variable size fields overflow header"),
    (true, "invalid UTF-8 in filename"),
    (true, "invalid UTF-8 in comment"),
    (true, "invalid Zip64 extra field"),
    (true, "invalid WinZip AES extra field"),
    (true, "garbage at end of extra fields"),
    (true, "extra field length is invalid"),
    (
        true,
        "file length in header doesn't match actual file length",
    ),
    (
        true,
        "compressed and uncompressed sizes don't match for stored file",
    ),
    (true, "local header and data descriptor do not match"),
    (false, "EOCD64 and EOCD64 locator do not match"),
    (true, "UTF-8 filename is ASCII and doesn't match filename"),
    (true, "UTF-8 comment is ASCII and doesn't match comment"),
    (false, "garbage at end of compressed data"),
];

/// strerror(errno) as glibc words it.
pub fn os_strerror(errno: i32) -> String {
    let s = std::io::Error::from_raw_os_error(errno).to_string();
    match s.rfind(" (os error ") {
        Some(p) => s[..p].to_string(),
        None => s,
    }
}

impl ZipError {
    pub fn new(ze: i32, se: i32) -> ZipError {
        ZipError {
            zip_err: ze,
            sys_err: se,
        }
    }

    pub fn set(&mut self, ze: i32, se: i32) {
        self.zip_err = ze;
        self.sys_err = se;
    }

    pub fn clear(&mut self) {
        self.zip_err = ER_OK;
        self.sys_err = 0;
    }

    pub fn is_ok(&self) -> bool {
        self.zip_err == ER_OK
    }

    fn from_io(ze: i32, e: &std::io::Error) -> ZipError {
        ZipError::new(ze, e.raw_os_error().unwrap_or(0))
    }

    /// zip_error_strerror()
    pub fn strerror(&self) -> String {
        if self.zip_err < 0 || self.zip_err as usize >= ERR_STR.len() {
            return format!("Unknown error {}", self.zip_err);
        }
        let (et, desc) = ERR_STR[self.zip_err as usize];
        let sys: Option<String> = match et {
            Et::Sys => Some(os_strerror(self.sys_err)),
            Et::Zlib => Some(goro_zlib::z_error(self.sys_err).to_string()),
            Et::Libzip => {
                let error = (self.sys_err & 0xff) as usize;
                let index = (self.sys_err >> 8) & MAX_DETAIL_INDEX as i32;
                if error == 0 {
                    None
                } else if error >= ERR_DETAILS.len() {
                    Some(format!("invalid detail error {}", error))
                } else if ERR_DETAILS[error].0 && (index as i64) < MAX_DETAIL_INDEX {
                    Some(format!("entry {}: {}", index, ERR_DETAILS[error].1))
                } else {
                    Some(ERR_DETAILS[error].1.to_string())
                }
            }
            Et::None => None,
        };
        match sys {
            None => desc.to_string(),
            Some(s) => format!("{}: {}", desc, s),
        }
    }
}

type Zr<T> = Result<T, ZipError>;

// ---------------------------------------------------------------------------
// strings (zip_string.c, zip_utf-8.c)

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Enc {
    Unknown,
    Ascii,
    Utf8Known,
    Utf8Guessed,
    Cp437,
    Error,
}

#[derive(Clone, Debug)]
pub struct ZString {
    pub raw: Vec<u8>,
    enc: Cell<Enc>,
}

static CP437: [u16; 256] = [
    0x0000, 0x263A, 0x263B, 0x2665, 0x2666, 0x2663, 0x2660, 0x2022, 0x25D8, 0x25CB, 0x25D9, 0x2642,
    0x2640, 0x266A, 0x266B, 0x263C, 0x25BA, 0x25C4, 0x2195, 0x203C, 0x00B6, 0x00A7, 0x25AC, 0x21A8,
    0x2191, 0x2193, 0x2192, 0x2190, 0x221F, 0x2194, 0x25B2, 0x25BC, 0x0020, 0x0021, 0x0022, 0x0023,
    0x0024, 0x0025, 0x0026, 0x0027, 0x0028, 0x0029, 0x002A, 0x002B, 0x002C, 0x002D, 0x002E, 0x002F,
    0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037, 0x0038, 0x0039, 0x003A, 0x003B,
    0x003C, 0x003D, 0x003E, 0x003F, 0x0040, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047,
    0x0048, 0x0049, 0x004A, 0x004B, 0x004C, 0x004D, 0x004E, 0x004F, 0x0050, 0x0051, 0x0052, 0x0053,
    0x0054, 0x0055, 0x0056, 0x0057, 0x0058, 0x0059, 0x005A, 0x005B, 0x005C, 0x005D, 0x005E, 0x005F,
    0x0060, 0x0061, 0x0062, 0x0063, 0x0064, 0x0065, 0x0066, 0x0067, 0x0068, 0x0069, 0x006A, 0x006B,
    0x006C, 0x006D, 0x006E, 0x006F, 0x0070, 0x0071, 0x0072, 0x0073, 0x0074, 0x0075, 0x0076, 0x0077,
    0x0078, 0x0079, 0x007A, 0x007B, 0x007C, 0x007D, 0x007E, 0x2302, 0x00C7, 0x00FC, 0x00E9, 0x00E2,
    0x00E4, 0x00E0, 0x00E5, 0x00E7, 0x00EA, 0x00EB, 0x00E8, 0x00EF, 0x00EE, 0x00EC, 0x00C4, 0x00C5,
    0x00C9, 0x00E6, 0x00C6, 0x00F4, 0x00F6, 0x00F2, 0x00FB, 0x00F9, 0x00FF, 0x00D6, 0x00DC, 0x00A2,
    0x00A3, 0x00A5, 0x20A7, 0x0192, 0x00E1, 0x00ED, 0x00F3, 0x00FA, 0x00F1, 0x00D1, 0x00AA, 0x00BA,
    0x00BF, 0x2310, 0x00AC, 0x00BD, 0x00BC, 0x00A1, 0x00AB, 0x00BB, 0x2591, 0x2592, 0x2593, 0x2502,
    0x2524, 0x2561, 0x2562, 0x2556, 0x2555, 0x2563, 0x2551, 0x2557, 0x255D, 0x255C, 0x255B, 0x2510,
    0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x255E, 0x255F, 0x255A, 0x2554, 0x2569, 0x2566,
    0x2560, 0x2550, 0x256C, 0x2567, 0x2568, 0x2564, 0x2565, 0x2559, 0x2558, 0x2552, 0x2553, 0x256B,
    0x256A, 0x2518, 0x250C, 0x2588, 0x2584, 0x258C, 0x2590, 0x2580, 0x03B1, 0x00DF, 0x0393, 0x03C0,
    0x03A3, 0x03C3, 0x00B5, 0x03C4, 0x03A6, 0x0398, 0x03A9, 0x03B4, 0x221E, 0x03C6, 0x03B5, 0x2229,
    0x2261, 0x00B1, 0x2265, 0x2264, 0x2320, 0x2321, 0x00F7, 0x2248, 0x00B0, 0x2219, 0x00B7, 0x221A,
    0x207F, 0x00B2, 0x25A0, 0x00A0,
];

fn cp437_to_utf8(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for &b in raw {
        let c = char::from_u32(CP437[b as usize] as u32).unwrap_or('\u{fffd}');
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
    out
}

impl ZString {
    /// _zip_string_new(): Ok(None) for an empty string.
    pub fn new(raw: &[u8], flags: u32) -> Zr<Option<ZString>> {
        if raw.is_empty() {
            return Ok(None);
        }
        let expected = match flags & FL_ENCODING_ALL {
            FL_ENC_GUESS => Enc::Unknown,
            FL_ENC_UTF_8 => Enc::Utf8Known,
            FL_ENC_CP437 => Enc::Cp437,
            _ => return Err(ZipError::new(ER_INVAL, 0)),
        };
        let s = ZString {
            raw: raw[..raw.len().min(0xffff)].to_vec(),
            enc: Cell::new(Enc::Unknown),
        };
        if expected != Enc::Unknown && s.guess(expected) == Enc::Error {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        Ok(Some(s))
    }

    fn raw_new(raw: Vec<u8>) -> ZString {
        ZString {
            raw,
            enc: Cell::new(Enc::Unknown),
        }
    }

    /// _zip_guess_encoding()
    pub fn guess(&self, expected: Enc) -> Enc {
        let cur = self.enc.get();
        if cur != Enc::Unknown {
            return cur;
        }
        let name = &self.raw;
        let mut can_be_ascii = true;
        let mut can_be_utf8 = true;
        let mut has_control = false;
        let mut i = 0;
        'outer: while i < name.len() {
            let c = name[i];
            if c < 128 {
                if c < 32 && c != b'\r' && c != b'\n' && c != b'\t' {
                    has_control = true;
                }
                i += 1;
                continue;
            }
            can_be_ascii = false;
            let ulen = if c & 0xe0 == 0xc0 {
                1
            } else if c & 0xf0 == 0xe0 {
                2
            } else if c & 0xf8 == 0xf0 {
                3
            } else {
                can_be_utf8 = false;
                break;
            };
            if i + ulen >= name.len() {
                can_be_utf8 = false;
                break;
            }
            for j in 1..=ulen {
                if name[i + j] & 0xc0 != 0x80 {
                    can_be_utf8 = false;
                    break 'outer;
                }
            }
            i += ulen + 1;
        }
        let enc = match expected {
            Enc::Utf8Known | Enc::Utf8Guessed => {
                if can_be_utf8 {
                    Enc::Utf8Known
                } else {
                    Enc::Error
                }
            }
            Enc::Ascii => {
                if can_be_ascii && !has_control {
                    Enc::Ascii
                } else {
                    Enc::Error
                }
            }
            Enc::Cp437 => Enc::Cp437,
            Enc::Unknown => {
                if can_be_ascii && !has_control {
                    Enc::Ascii
                } else if can_be_ascii {
                    Enc::Cp437
                } else if can_be_utf8 {
                    Enc::Utf8Guessed
                } else {
                    Enc::Cp437
                }
            }
            Enc::Error => Enc::Error,
        };
        self.enc.set(enc);
        enc
    }

    fn set_enc(&self, e: Enc) {
        self.enc.set(e);
    }

    pub fn is_ascii(&self) -> bool {
        self.enc.get() == Enc::Ascii || self.raw.iter().all(|&c| c & 0x80 == 0)
    }
}

/// _zip_string_get()
pub fn string_get(s: Option<&ZString>, flags: u32) -> Vec<u8> {
    let Some(s) = s else {
        return Vec::new();
    };
    if flags & FL_ENC_RAW == 0 {
        if s.enc.get() == Enc::Unknown {
            s.guess(Enc::Unknown);
        }
        let e = s.enc.get();
        if (flags & FL_ENC_STRICT != 0 && e != Enc::Ascii && e != Enc::Utf8Known) || e == Enc::Cp437
        {
            return cp437_to_utf8(&s.raw);
        }
    }
    s.raw.clone()
}

fn string_equal(a: Option<&ZString>, b: Option<&ZString>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.raw == b.raw,
        _ => false,
    }
}

fn string_len(s: Option<&ZString>) -> usize {
    s.map_or(0, |s| s.raw.len())
}

fn guess_enc(s: Option<&ZString>) -> Enc {
    match s {
        None => Enc::Ascii,
        Some(s) => s.guess(Enc::Unknown),
    }
}

/// strlen(): libzip takes C strings
pub fn c_str(v: &[u8]) -> &[u8] {
    match v.iter().position(|&c| c == 0) {
        Some(n) => &v[..n],
        None => v,
    }
}

// ---------------------------------------------------------------------------
// buffers

struct Rd<'a> {
    d: &'a [u8],
    pos: usize,
    ok: bool,
}

impl<'a> Rd<'a> {
    fn new(d: &'a [u8]) -> Rd<'a> {
        Rd {
            d,
            pos: 0,
            ok: true,
        }
    }

    fn left(&self) -> usize {
        self.d.len().saturating_sub(self.pos)
    }

    fn get(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.left() < n {
            self.ok = false;
            return None;
        }
        let s = &self.d[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }

    fn get8(&mut self) -> u8 {
        self.get(1).map_or(0, |b| b[0])
    }

    fn get16(&mut self) -> u16 {
        self.get(2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn get32(&mut self) -> u32 {
        self.get(4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn get64(&mut self) -> u64 {
        self.get(8).map_or(0, |b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_le_bytes(a)
        })
    }

    fn eof(&self) -> bool {
        self.ok && self.pos == self.d.len()
    }
}

struct Wr(Vec<u8>);

impl Wr {
    fn p16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn p32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn p64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn put(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
}

// ---------------------------------------------------------------------------
// extra fields (zip_extra_field.c)

#[derive(Clone, Debug)]
pub struct ExtraField {
    pub id: u16,
    pub flags: u16,
    pub data: Vec<u8>,
}

fn ef_parse(data: &[u8], flags: u16) -> Zr<Vec<ExtraField>> {
    let mut b = Rd::new(data);
    let mut out = Vec::new();
    while b.ok && b.left() >= 4 {
        let id = b.get16();
        let len = b.get16() as usize;
        match b.get(len) {
            Some(d) => out.push(ExtraField {
                id,
                flags,
                data: d.to_vec(),
            }),
            None => return Err(ZipError::new(ER_INCONS, DETAIL_INVALID_EF_LENGTH)),
        }
    }
    if !b.eof() {
        let rest = &data[b.pos.min(data.len())..];
        if rest.len() >= 4 || rest.iter().any(|&c| c != 0) {
            return Err(ZipError::new(ER_INCONS, DETAIL_EF_TRAILING_GARBAGE));
        }
    }
    Ok(out)
}

fn ef_is_internal(id: u16) -> bool {
    matches!(
        id,
        EF_UTF_8_COMMENT | EF_UTF_8_NAME | EF_WINZIP_AES | EF_ZIP64
    )
}

fn ef_get_by_id(ef: &[ExtraField], id: u16, flags: u16) -> Option<&[u8]> {
    ef.iter()
        .find(|e| e.id == id && e.flags & flags & EF_BOTH != 0)
        .map(|e| e.data.as_slice())
}

fn ef_merge(mut to: Vec<ExtraField>, from: Vec<ExtraField>) -> Vec<ExtraField> {
    for f in from {
        if let Some(t) = to.iter_mut().find(|t| t.id == f.id && t.data == f.data) {
            t.flags |= f.flags & EF_BOTH;
        } else {
            to.push(f);
        }
    }
    to
}

fn ef_size(ef: &[ExtraField], flags: u16) -> usize {
    ef.iter()
        .filter(|e| e.flags & flags & EF_BOTH != 0)
        .map(|e| 4 + e.data.len())
        .sum()
}

fn ef_write(out: &mut Wr, ef: &[ExtraField], flags: u16) {
    for e in ef {
        if e.flags & flags & EF_BOTH != 0 {
            out.p16(e.id);
            out.p16(e.data.len() as u16);
            out.put(&e.data);
        }
    }
}

/// _zip_ef_delete_by_id(ZIP_EXTRA_FIELD_ALL, ZIP_EXTRA_FIELD_ALL, flags)
fn ef_delete_all(ef: Vec<ExtraField>, flags: u16) -> Vec<ExtraField> {
    ef.into_iter()
        .filter_map(|mut e| {
            if e.flags & flags & EF_BOTH != 0 {
                e.flags &= !(flags & EF_BOTH);
                if e.flags & EF_BOTH == 0 {
                    return None;
                }
            }
            Some(e)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// DOS time

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DosTime {
    pub time: u16,
    pub date: u16,
}

/// _zip_d2u_time(): mktime() in the local time zone
pub fn d2u_time(d: DosTime) -> i64 {
    // SAFETY: tm is plain data, mktime only reads/normalizes it.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_isdst = -1;
        tm.tm_year = ((d.date >> 9) & 127) as i32 + 1980 - 1900;
        tm.tm_mon = ((d.date >> 5) & 15) as i32 - 1;
        tm.tm_mday = (d.date & 31) as i32;
        tm.tm_hour = ((d.time >> 11) & 31) as i32;
        tm.tm_min = ((d.time >> 5) & 63) as i32;
        tm.tm_sec = ((d.time << 1) & 62) as i32;
        libc::mktime(&mut tm) as i64
    }
}

/// _zip_u2d_time(): localtime()
pub fn u2d_time(t: i64) -> Zr<DosTime> {
    // SAFETY: localtime_r writes the struct it is given.
    unsafe {
        let tt = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&tt, &mut tm).is_null() {
            return Err(ZipError::new(ER_INVAL, 75));
        }
        if tm.tm_year < 80 {
            tm.tm_year = 80;
        }
        Ok(DosTime {
            date: (((tm.tm_year + 1900 - 1980) << 9) + ((tm.tm_mon + 1) << 5) + tm.tm_mday) as u16,
            time: ((tm.tm_hour << 11) + (tm.tm_min << 5) + (tm.tm_sec >> 1)) as u16,
        })
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// directory entries (zip_dirent.c)

#[derive(Clone, Debug)]
pub struct Dirent {
    pub changed: u32,
    pub local_extra_fields_read: bool,
    pub crc_valid: bool,
    pub last_mod_mtime: Option<i64>,
    pub version_madeby: u16,
    pub version_needed: u16,
    pub bitflags: u16,
    pub comp_method: i32,
    pub last_mod: DosTime,
    pub crc: u32,
    pub comp_size: u64,
    pub uncomp_size: u64,
    pub filename: Option<ZString>,
    pub extra_fields: Vec<ExtraField>,
    pub comment: Option<ZString>,
    pub disk_number: u32,
    pub int_attrib: u16,
    pub ext_attrib: u32,
    pub offset: u64,
    pub compression_level: u32,
    pub encryption_method: u16,
    pub password: Option<Vec<u8>>,
}

impl Default for Dirent {
    /// _zip_dirent_init()
    fn default() -> Dirent {
        Dirent {
            changed: 0,
            local_extra_fields_read: false,
            crc_valid: true,
            last_mod_mtime: None,
            version_madeby: 63 | ((OPSYS_DEFAULT as u16) << 8),
            version_needed: 10,
            bitflags: 0,
            comp_method: CM_DEFAULT,
            last_mod: DosTime::default(),
            crc: 0,
            comp_size: 0,
            uncomp_size: 0,
            filename: None,
            extra_fields: Vec::new(),
            comment: None,
            disk_number: 0,
            int_attrib: 0,
            ext_attrib: EXT_ATTRIB_DEFAULT,
            offset: 0,
            compression_level: 0,
            encryption_method: EM_NONE,
            password: None,
        }
    }
}

impl Dirent {
    /// _zip_dirent_clone()
    fn clone_of(orig: Option<&Dirent>) -> Dirent {
        let mut d = orig.cloned().unwrap_or_default();
        d.changed = 0;
        d
    }

    fn needs_zip64(&self, flags: u32) -> bool {
        self.uncomp_size >= u32::MAX as u64
            || self.comp_size >= u32::MAX as u64
            || (flags & FL_CENTRAL != 0 && self.offset >= u32::MAX as u64)
    }

    /// zip_dirent_get_last_mod_mtime()
    pub fn mtime(&mut self) -> i64 {
        if self.last_mod_mtime.is_none() {
            self.last_mod_mtime = Some(d2u_time(self.last_mod));
        }
        self.last_mod_mtime.unwrap()
    }

    /// _zip_dirent_merge()
    fn merge(&mut self, orig: &Dirent, replacing_data: bool) {
        if self.changed & DIRENT_ATTRIBUTES == 0 {
            self.ext_attrib = orig.ext_attrib;
            self.int_attrib = orig.int_attrib;
        }
        if self.changed & DIRENT_COMMENT == 0 {
            self.comment = orig.comment.clone();
        }
        if self.changed & DIRENT_COMP_METHOD == 0 {
            if replacing_data {
                self.comp_method = CM_DEFAULT;
                self.compression_level = 0;
            } else {
                self.comp_method = orig.comp_method;
                self.compression_level = orig.compression_level;
            }
        }
        if self.changed & DIRENT_ENCRYPTION_METHOD == 0 {
            self.encryption_method = if replacing_data {
                EM_NONE
            } else {
                orig.encryption_method
            };
        }
        if self.changed & DIRENT_EXTRA_FIELD == 0 {
            self.extra_fields = orig.extra_fields.clone();
        }
        if self.changed & DIRENT_FILENAME == 0 {
            self.filename = orig.filename.clone();
        }
        if self.changed & DIRENT_LAST_MOD == 0 {
            self.last_mod = orig.last_mod;
        }
        if self.changed & DIRENT_PASSWORD == 0 {
            self.password = orig.password.clone();
        }
    }

    /// zip_dirent_torrentzip_normalize()
    fn torrentzip_normalize(&mut self) {
        self.version_madeby = 0;
        self.version_needed = 20;
        self.bitflags = 2;
        self.comp_method = CM_DEFLATE as i32;
        self.compression_level = TORRENTZIP_COMPRESSION_FLAGS;
        self.disk_number = 0;
        self.int_attrib = 0;
        self.ext_attrib = 0;
    }

    /// zip_dirent_check_consistency()
    fn check_consistency(&self) -> i32 {
        if self.comp_method == CM_STORE as i32 {
            let hs: u64 = match self.encryption_method {
                EM_NONE => 0,
                EM_TRAD_PKWARE => 12,
                EM_AES_128 => 20,
                EM_AES_192 => 24,
                EM_AES_256 => 28,
                _ => return 0,
            };
            if self.uncomp_size.checked_add(hs).is_none() || self.comp_size != self.uncomp_size + hs
            {
                return DETAIL_STORED_SIZE_MISMATCH;
            }
        }
        0
    }
}

/// zip_file_attributes_t
#[derive(Clone, Copy, Default)]
pub struct Attributes {
    pub host_system: Option<u8>,
    pub ascii: Option<bool>,
    pub version_needed: Option<u16>,
    pub external_file_attributes: Option<u32>,
    /// (flags, mask)
    pub gpbf: Option<(u16, u16)>,
}

impl Dirent {
    /// _zip_dirent_apply_attributes()
    fn apply_attributes(&mut self, a: &Attributes, force_zip64: bool) -> bool {
        let mut changed = false;
        if let Some((flags, mask)) = a.gpbf {
            let mask = mask & GPBF_ALLOWED_MASK;
            let bf = (self.bitflags & !mask) | (flags & mask);
            if self.bitflags != bf {
                self.bitflags = bf;
                changed = true;
            }
        }
        if let Some(ascii) = a.ascii {
            let ia = (self.int_attrib & !1) | ascii as u16;
            if self.int_attrib != ia {
                self.int_attrib = ia;
                changed = true;
            }
        }
        if self.changed & DIRENT_ATTRIBUTES == 0 {
            if let Some(ea) = a.external_file_attributes {
                if self.ext_attrib != ea {
                    self.ext_attrib = ea;
                    changed = true;
                }
            }
        }
        let mut vn: u16 = if self.comp_method == CM_LZMA as i32 {
            63
        } else if matches!(self.encryption_method, EM_AES_128 | EM_AES_192 | EM_AES_256) {
            51
        } else if self.comp_method == CM_BZIP2 as i32 {
            46
        } else if force_zip64 || self.needs_zip64(0) {
            45
        } else if self.comp_method == CM_DEFLATE as i32
            || self.encryption_method == EM_TRAD_PKWARE
            || self
                .filename
                .as_ref()
                .is_some_and(|f| f.raw.last() == Some(&b'/'))
        {
            20
        } else {
            10
        };
        if let Some(v) = a.version_needed {
            vn = vn.max(v);
        }
        if self.version_needed != vn {
            self.version_needed = vn;
            changed = true;
        }
        let mut vm: u16 = 63 | (self.version_madeby & 0xff00);
        if self.changed & DIRENT_ATTRIBUTES == 0 {
            if let Some(hs) = a.host_system {
                vm = (vm & 0xff) | ((hs as u16) << 8);
            }
        }
        if self.version_madeby != vm {
            self.version_madeby = vm;
            changed = true;
        }
        changed
    }
}

/// _zip_dirent_process_ef_utf_8()
fn process_ef_utf8(
    ef: &[ExtraField],
    id: u16,
    s: Option<ZString>,
    check_consistency: bool,
) -> Result<Option<ZString>, ()> {
    let Some(data) = ef_get_by_id(ef, id, EF_BOTH) else {
        return Ok(s);
    };
    if data.len() < 5 || data[0] != 1 {
        return Ok(s);
    }
    let crc = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
    let raw = s.as_ref().map_or(&[][..], |s| &s.raw[..]);
    if goro_zlib::crc32(0, raw) == crc {
        if let Ok(Some(ef_str)) = ZString::new(&data[5..], FL_ENC_UTF_8) {
            if check_consistency && !string_equal(s.as_ref(), Some(&ef_str)) && ef_str.is_ascii() {
                return Err(());
            }
            return Ok(Some(ef_str));
        }
    }
    Ok(s)
}

/// _zip_dirent_process_winzip_aes()
fn process_winzip_aes(de: &mut Dirent) -> Zr<()> {
    if de.comp_method != CM_WINZIP_AES as i32 {
        return Ok(());
    }
    let ef = ef_get_by_id(&de.extra_fields, EF_WINZIP_AES, EF_BOTH);
    let Some(ef) = ef.filter(|e| e.len() >= 7) else {
        return Err(ZipError::new(ER_INCONS, DETAIL_INVALID_WINZIPAES_EF));
    };
    let ef = ef.to_vec();
    let mut b = Rd::new(&ef);
    let crc_valid = match b.get16() {
        1 => true,
        2 => false,
        _ => return Err(ZipError::new(ER_ENCRNOTSUPP, 0)),
    };
    if b.get(2) != Some(&b"AE"[..]) {
        return Err(ZipError::new(ER_ENCRNOTSUPP, 0));
    }
    let em = match b.get8() {
        1 => EM_AES_128,
        2 => EM_AES_192,
        3 => EM_AES_256,
        _ => return Err(ZipError::new(ER_ENCRNOTSUPP, 0)),
    };
    if ef.len() != 7 {
        return Err(ZipError::new(ER_INCONS, DETAIL_INVALID_WINZIPAES_EF));
    }
    de.crc_valid = crc_valid;
    de.encryption_method = em;
    de.comp_method = b.get16() as i32;
    Ok(())
}

/// _zip_dirent_read(): (entry, size read). `data` holds the bytes at the
/// header; for a local header with a data descriptor, `after` gives the
/// bytes from the header's start (to find the descriptor).
fn dirent_read(
    buf: &[u8],
    local: bool,
    central_comp_size: u64,
    check_consistency: bool,
) -> Zr<(Dirent, usize)> {
    let size = if local { LENTRYSIZE } else { CDENTRYSIZE };
    if buf.len() < size {
        return Err(ZipError::new(ER_NOZIP, 0));
    }
    let mut b = Rd::new(buf);
    if b.get(4) != Some(if local { LOCAL_MAGIC } else { CENTRAL_MAGIC }) {
        return Err(ZipError::new(ER_NOZIP, 0));
    }
    let version_madeby = if local { 0 } else { b.get16() };
    let version_needed = b.get16();
    let bitflags = b.get16();
    let comp_method = b.get16() as i32;
    let mut de = Dirent {
        version_madeby,
        version_needed,
        bitflags,
        comp_method,
        ..Default::default()
    };
    de.last_mod.time = b.get16();
    de.last_mod.date = b.get16();
    de.crc = b.get32();
    de.comp_size = b.get32() as u64;
    de.uncomp_size = b.get32() as u64;
    let filename_len = b.get16() as usize;
    let ef_len = b.get16() as usize;
    let comment_len;
    if local {
        comment_len = 0;
        de.disk_number = 0;
        de.int_attrib = 0;
        de.ext_attrib = 0;
        de.offset = 0;
    } else {
        comment_len = b.get16() as usize;
        de.disk_number = b.get16() as u32;
        de.int_attrib = b.get16();
        de.ext_attrib = b.get32();
        de.offset = b.get32() as u64;
    }
    if de.bitflags & GPBF_ENCRYPTED != 0 {
        de.encryption_method = if de.bitflags & GPBF_STRONG_ENCRYPTION != 0 {
            EM_UNKNOWN
        } else {
            EM_TRAD_PKWARE
        };
    } else {
        de.encryption_method = EM_NONE;
    }
    let variable = filename_len + ef_len + comment_len;
    if b.left() < variable {
        return Err(ZipError::new(ER_INCONS, DETAIL_VARIABLE_SIZE_OVERFLOW));
    }
    if filename_len > 0 {
        let raw = b.get(filename_len).unwrap_or_default();
        let s = ZString::raw_new(raw.to_vec());
        if de.bitflags & GPBF_ENCODING_UTF_8 != 0 && s.guess(Enc::Utf8Known) == Enc::Error {
            return Err(ZipError::new(ER_INCONS, DETAIL_INVALID_UTF8_IN_FILENAME));
        }
        de.filename = Some(s);
    }
    if ef_len > 0 {
        let raw = b.get(ef_len).unwrap_or_default();
        de.extra_fields = ef_parse(raw, if local { EF_LOCAL } else { EF_CENTRAL })?;
        if local {
            de.local_extra_fields_read = true;
        }
    }
    if comment_len > 0 {
        let raw = b.get(comment_len).unwrap_or_default();
        let s = ZString::raw_new(raw.to_vec());
        if de.bitflags & GPBF_ENCODING_UTF_8 != 0 && s.guess(Enc::Utf8Known) == Enc::Error {
            return Err(ZipError::new(ER_INCONS, DETAIL_INVALID_UTF8_IN_COMMENT));
        }
        de.comment = Some(s);
    }
    let had_name = de.filename.is_some();
    match process_ef_utf8(
        &de.extra_fields,
        EF_UTF_8_NAME,
        de.filename.take(),
        check_consistency,
    ) {
        Ok(s) => de.filename = s,
        Err(()) => return Err(ZipError::new(ER_INCONS, DETAIL_UTF8_FILENAME_MISMATCH)),
    }
    let _ = had_name;
    if !local {
        match process_ef_utf8(
            &de.extra_fields,
            EF_UTF_8_COMMENT,
            de.comment.take(),
            check_consistency,
        ) {
            Ok(s) => de.comment = s,
            Err(()) => return Err(ZipError::new(ER_INCONS, DETAIL_UTF8_COMMENT_MISMATCH)),
        }
    }
    let mut is_zip64 = false;
    if de.uncomp_size == u32::MAX as u64
        || de.comp_size == u32::MAX as u64
        || de.offset == u32::MAX as u64
    {
        let fl = if local { EF_LOCAL } else { EF_CENTRAL };
        if let Some(ef) = ef_get_by_id(&de.extra_fields, EF_ZIP64, fl).map(|e| e.to_vec()) {
            process_ef_zip64(&mut de, &ef, local)?;
        }
        is_zip64 = true;
    }
    if local && de.bitflags & GPBF_DATA_DESCRIPTOR != 0 {
        let start = size + variable;
        let dd = start as u64 + central_comp_size;
        let dd = buf.get(dd as usize..).unwrap_or_default();
        let mut db = Rd::new(dd);
        if dd.len() >= 4 && &dd[..4] == DATADES_MAGIC {
            db.get(4);
        }
        let df_crc = db.get32();
        let df_comp = if is_zip64 {
            db.get64()
        } else {
            db.get32() as u64
        };
        let df_uncomp = if is_zip64 {
            db.get64()
        } else {
            db.get32() as u64
        };
        if !db.ok {
            return Err(ZipError::new(ER_INTERNAL, 0));
        }
        if (de.crc != 0 && de.crc != df_crc)
            || (de.comp_size != 0 && de.comp_size != df_comp)
            || (de.uncomp_size != 0 && de.uncomp_size != df_uncomp)
        {
            return Err(ZipError::new(ER_INCONS, DETAIL_DATA_DESCRIPTOR_MISMATCH));
        }
        de.crc = df_crc;
        de.comp_size = df_comp;
        de.uncomp_size = df_uncomp;
    }
    if de.offset > i64::MAX as u64 {
        return Err(ZipError::new(ER_SEEK, EFBIG));
    }
    process_winzip_aes(&mut de)?;
    de.extra_fields.retain(|e| !ef_is_internal(e.id));
    Ok((de, size + variable))
}

/// zip_dirent_process_ef_zip64()
fn process_ef_zip64(de: &mut Dirent, ef: &[u8], local: bool) -> Zr<()> {
    let mut b = Rd::new(ef);
    if de.uncomp_size == u32::MAX as u64 {
        de.uncomp_size = b.get64();
    } else if local {
        b.get(8);
    }
    if de.comp_size == u32::MAX as u64 {
        de.comp_size = b.get64();
    }
    if !local {
        if de.offset == u32::MAX as u64 {
            de.offset = b.get64();
        }
        if de.disk_number == 0xffff {
            de.disk_number = b.get32();
        }
    }
    if !b.eof() {
        let mut ok = true;
        match ef.len() {
            28 | 24 => {
                if ef.len() == 28 {
                    let mut c = Rd::new(&ef[24..]);
                    if de.disk_number != c.get32() {
                        ok = false;
                    }
                }
                let mut c = Rd::new(ef);
                if de.uncomp_size != c.get64()
                    || de.comp_size != c.get64()
                    || de.offset != c.get64()
                {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        if !ok {
            return Err(ZipError::new(ER_INCONS, DETAIL_INVALID_ZIP64_EF));
        }
    }
    Ok(())
}

/// _zip_ef_utf8()
fn ef_utf8(id: u16, s: Option<&ZString>) -> Zr<ExtraField> {
    let raw = s.map_or(&[][..], |s| &s.raw[..]);
    if raw.len() + 5 > 0xffff {
        return Err(ZipError::new(ER_INVAL, 0));
    }
    let mut d = vec![1u8];
    d.extend_from_slice(&goro_zlib::crc32(0, raw).to_le_bytes());
    d.extend_from_slice(raw);
    Ok(ExtraField {
        id,
        flags: EF_BOTH,
        data: d,
    })
}

/// _zip_dirent_write(): Ok(is_zip64)
fn dirent_write(out: &mut Wr, de: &mut Dirent, flags: u32, want_torrentzip: bool) -> Zr<bool> {
    let mut ef: Vec<ExtraField> = Vec::new();
    let name_enc = guess_enc(de.filename.as_ref());
    let com_enc = guess_enc(de.comment.as_ref());
    if (name_enc == Enc::Utf8Known && com_enc == Enc::Ascii)
        || (name_enc == Enc::Ascii && com_enc == Enc::Utf8Known)
        || (name_enc == Enc::Utf8Known && com_enc == Enc::Utf8Known)
    {
        de.bitflags |= GPBF_ENCODING_UTF_8;
    } else {
        de.bitflags &= !GPBF_ENCODING_UTF_8;
        if name_enc == Enc::Utf8Known {
            ef.insert(0, ef_utf8(EF_UTF_8_NAME, de.filename.as_ref())?);
        }
        if flags & FL_LOCAL == 0 && com_enc == Enc::Utf8Known {
            ef.insert(0, ef_utf8(EF_UTF_8_COMMENT, de.comment.as_ref())?);
        }
    }
    if de.encryption_method == EM_NONE {
        de.bitflags &= !GPBF_ENCRYPTED;
    } else {
        de.bitflags |= GPBF_ENCRYPTED;
    }
    let is_really_zip64 = de.needs_zip64(flags);
    let is_zip64 =
        (flags & (FL_LOCAL | FL_FORCE_ZIP64)) == (FL_LOCAL | FL_FORCE_ZIP64) || is_really_zip64;
    let is_winzip_aes = matches!(de.encryption_method, EM_AES_128 | EM_AES_192 | EM_AES_256);
    if is_zip64 {
        let mut e = Wr(Vec::new());
        if flags & FL_LOCAL != 0 {
            if flags & FL_FORCE_ZIP64 != 0
                || de.comp_size > u32::MAX as u64
                || de.uncomp_size > u32::MAX as u64
            {
                e.p64(de.uncomp_size);
                e.p64(de.comp_size);
            }
        } else if flags & FL_FORCE_ZIP64 != 0
            || de.comp_size > u32::MAX as u64
            || de.uncomp_size > u32::MAX as u64
            || de.offset > u32::MAX as u64
        {
            if de.uncomp_size >= u32::MAX as u64 {
                e.p64(de.uncomp_size);
            }
            if de.comp_size >= u32::MAX as u64 {
                e.p64(de.comp_size);
            }
            if de.offset >= u32::MAX as u64 {
                e.p64(de.offset);
            }
        }
        ef.insert(
            0,
            ExtraField {
                id: EF_ZIP64,
                flags: EF_BOTH,
                data: e.0,
            },
        );
    }
    if is_winzip_aes {
        let mut e = Wr(Vec::new());
        e.p16(2);
        e.put(b"AE");
        e.0.push((de.encryption_method & 0xff) as u8);
        e.p16(de.comp_method as u16);
        ef.insert(
            0,
            ExtraField {
                id: EF_WINZIP_AES,
                flags: EF_BOTH,
                data: e.0,
            },
        );
    }
    out.put(if flags & FL_LOCAL != 0 {
        LOCAL_MAGIC
    } else {
        CENTRAL_MAGIC
    });
    if flags & FL_LOCAL == 0 {
        out.p16(de.version_madeby);
    }
    out.p16(if is_really_zip64 { 45 } else { 0 }.max(de.version_needed));
    out.p16(de.bitflags);
    if is_winzip_aes {
        out.p16(CM_WINZIP_AES);
    } else {
        out.p16(ZipInt::cm_actual(de.comp_method));
    }
    let dostime = if want_torrentzip {
        DosTime {
            time: 0xbc00,
            date: 0x2198,
        }
    } else {
        de.last_mod
    };
    out.p16(dostime.time);
    out.p16(dostime.date);
    if is_winzip_aes && de.uncomp_size < 20 {
        out.p32(0);
    } else {
        out.p32(de.crc);
    }
    if flags & FL_LOCAL != 0
        && (de.comp_size >= u32::MAX as u64 || de.uncomp_size >= u32::MAX as u64)
    {
        out.p32(u32::MAX);
        out.p32(u32::MAX);
    } else {
        out.p32(de.comp_size.min(u32::MAX as u64) as u32);
        out.p32(de.uncomp_size.min(u32::MAX as u64) as u32);
    }
    out.p16(string_len(de.filename.as_ref()) as u16);
    let mut ef_total = ef_size(&ef, EF_BOTH);
    if !want_torrentzip {
        ef_total += ef_size(&de.extra_fields, flags as u16);
    }
    out.p16(ef_total as u16);
    if flags & FL_LOCAL == 0 {
        out.p16(if want_torrentzip {
            0
        } else {
            string_len(de.comment.as_ref()) as u16
        });
        out.p16(de.disk_number as u16);
        out.p16(de.int_attrib);
        out.p32(de.ext_attrib);
        out.p32(de.offset.min(u32::MAX as u64) as u32);
    }
    if let Some(f) = &de.filename {
        out.put(&f.raw);
    }
    ef_write(out, &ef, EF_BOTH);
    if !want_torrentzip {
        let fl = flags as u16;
        ef_write(out, &de.extra_fields, fl);
    }
    if flags & FL_LOCAL == 0 && !want_torrentzip {
        if let Some(c) = &de.comment {
            out.put(&c.raw);
        }
    }
    Ok(is_zip64)
}

// ---------------------------------------------------------------------------
// sources

/// The data of an added or replaced entry (zip_source_t).
#[derive(Clone)]
pub enum Source {
    /// zip_source_buffer()
    Buffer { data: Rc<Vec<u8>>, mtime: i64 },
    /// zip_source_file(): read at close time
    File {
        path: String,
        start: u64,
        /// 0: to the end
        len: u64,
        size: u64,
        mtime: i64,
        attributes: Attributes,
        /// read when added (ZIP_FL_OPEN_FILE_NOW)
        data: Option<Rc<Vec<u8>>>,
    },
}

/// zip_stat_t (fields always present; `valid` as libzip)
#[derive(Clone, Debug)]
pub struct Stat {
    pub name: Vec<u8>,
    pub index: u64,
    pub crc: u32,
    pub size: u64,
    pub mtime: i64,
    pub comp_size: u64,
    pub comp_method: u16,
    pub encryption_method: u16,
    pub valid: u32,
}

pub const STAT_NAME: u32 = 1;
pub const STAT_INDEX: u32 = 2;
pub const STAT_SIZE: u32 = 4;
pub const STAT_COMP_SIZE: u32 = 8;
pub const STAT_MTIME: u32 = 16;
pub const STAT_CRC: u32 = 32;
pub const STAT_COMP_METHOD: u32 = 64;
pub const STAT_ENCRYPTION_METHOD: u32 = 128;

impl Default for Stat {
    /// zip_stat_init()
    fn default() -> Stat {
        Stat {
            name: Vec::new(),
            index: u64::MAX,
            crc: 0,
            size: 0,
            mtime: -1,
            comp_size: 0,
            comp_method: CM_STORE,
            encryption_method: EM_NONE,
            valid: 0,
        }
    }
}

impl Source {
    /// zip_source_file_create()
    pub fn file(path: &str, start: u64, len: i64) -> Zr<Source> {
        if len < -2 {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        let length = if len < 0 { 0 } else { len as u64 };
        if start > i64::MAX as u64 || start.checked_add(length).is_none() {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        let md = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                return Err(ZipError::from_io(ER_READ, &e));
            }
        };
        use std::os::unix::fs::MetadataExt;
        let mode = md.mode();
        let attributes = Attributes {
            host_system: Some(OPSYS_UNIX),
            external_file_attributes: Some((mode << 16) | if mode & 0o200 != 0 { 0 } else { 1 }),
            ..Default::default()
        };
        let mut l = length;
        if md.is_file() {
            if start + length > md.size() {
                return Err(ZipError::new(ER_INVAL, 0));
            }
            if l == 0 && len != -2 {
                l = md.size() - start;
            }
        }
        Ok(Source::File {
            path: path.to_string(),
            start,
            len: l,
            size: md.size(),
            mtime: md.mtime(),
            attributes,
            data: None,
        })
    }

    /// The source with its data already read.
    pub fn preloaded(self, d: Vec<u8>) -> Source {
        match self {
            Source::File {
                path,
                start,
                len,
                size,
                mtime,
                attributes,
                ..
            } => Source::File {
                path,
                start,
                len,
                size,
                mtime,
                attributes,
                data: Some(Rc::new(d)),
            },
            s => s,
        }
    }

    pub fn buffer(data: Vec<u8>) -> Source {
        Source::Buffer {
            data: Rc::new(data),
            mtime: now(),
        }
    }

    /// zip_source_stat()
    pub fn stat(&self) -> Stat {
        let mut st = Stat::default();
        match self {
            Source::Buffer { data, mtime } => {
                st.mtime = *mtime;
                st.size = data.len() as u64;
                st.comp_size = st.size;
                st.valid = STAT_MTIME
                    | STAT_SIZE
                    | STAT_COMP_SIZE
                    | STAT_COMP_METHOD
                    | STAT_ENCRYPTION_METHOD;
            }
            Source::File { len, mtime, .. } => {
                st.mtime = *mtime;
                st.valid = STAT_MTIME;
                if *len > 0 {
                    st.size = *len;
                    st.valid |= STAT_SIZE;
                }
            }
        }
        st
    }

    fn attributes(&self) -> Attributes {
        match self {
            Source::Buffer { .. } => Attributes::default(),
            Source::File { attributes, .. } => *attributes,
        }
    }

    /// The whole data (opening and reading the file for file sources).
    pub fn read_all(&self) -> Zr<Vec<u8>> {
        match self {
            Source::Buffer { data, .. } => Ok((**data).clone()),
            Source::File { data: Some(d), .. } => Ok((**d).clone()),
            Source::File {
                path, start, len, ..
            } => {
                use std::io::{Read, Seek, SeekFrom};
                let mut f =
                    std::fs::File::open(path).map_err(|e| ZipError::from_io(ER_OPEN, &e))?;
                if *start > 0 {
                    f.seek(SeekFrom::Start(*start))
                        .map_err(|e| ZipError::from_io(ER_SEEK, &e))?;
                }
                let mut out = Vec::new();
                if *len > 0 {
                    (&mut f)
                        .take(*len)
                        .read_to_end(&mut out)
                        .map_err(|e| ZipError::from_io(ER_READ, &e))?;
                    if (out.len() as u64) < *len {
                        return Err(ZipError::new(ER_EOF, 0));
                    }
                } else {
                    f.read_to_end(&mut out)
                        .map_err(|e| ZipError::from_io(ER_READ, &e))?;
                }
                Ok(out)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the archive

#[derive(Clone, Default)]
pub struct Entry {
    pub orig: Option<Dirent>,
    pub changes: Option<Dirent>,
    pub source: Option<Source>,
    pub deleted: bool,
}

impl Entry {
    fn changed(&self, f: u32) -> bool {
        self.changes.as_ref().is_some_and(|c| c.changed & f != 0)
    }

    fn data_changed(&self) -> bool {
        self.source.is_some()
    }

    fn has_changes(&self) -> bool {
        self.data_changed() || self.deleted || self.changed(DIRENT_ALL)
    }

    /// _zip_unchange_data()
    fn unchange_data(&mut self) {
        self.source = None;
        if let Some(c) = &mut self.changes {
            if c.changed & DIRENT_COMP_METHOD != 0 && c.comp_method == CM_REPLACED_DEFAULT {
                c.changed &= !DIRENT_COMP_METHOD;
                if c.changed == 0 {
                    self.changes = None;
                }
            }
        }
        self.deleted = false;
    }
}

/// Progress / cancel hooks (zip_register_*_callback_with_state); the
/// callbacks themselves live in the PHP object.
pub trait ProgressHooks {
    fn progress(&mut self, state: f64);
    /// nonzero: cancel
    fn cancel(&mut self) -> i64;
    fn has_progress(&self) -> bool;
    fn has_cancel(&self) -> bool;
    fn precision(&self) -> f64;
}

struct Progress<'a> {
    hooks: &'a mut dyn ProgressHooks,
    last_update: f64,
    start: f64,
    end: f64,
}

impl Progress<'_> {
    /// _zip_progress_start()
    fn start(&mut self) -> bool {
        if self.hooks.has_progress() {
            self.last_update = 0.0;
            self.hooks.progress(0.0);
        }
        if self.hooks.has_cancel() && self.hooks.cancel() != 0 {
            return false;
        }
        true
    }

    fn subrange(&mut self, start: f64, end: f64) -> bool {
        self.start = start;
        self.end = end;
        self.update(0.0)
    }

    /// _zip_progress_update(): false to cancel
    fn update(&mut self, sub: f64) -> bool {
        if self.hooks.has_progress() {
            let current = sub.clamp(0.0, 1.0) * (self.end - self.start) + self.start;
            if current - self.last_update > self.hooks.precision()
                || (self.last_update < 1.0 && current == 1.0)
            {
                self.hooks.progress(current);
                self.last_update = current;
            }
        }
        if self.hooks.has_cancel() && self.hooks.cancel() != 0 {
            return false;
        }
        true
    }
}

pub struct Archive {
    /// the archive's file name (zip_source_file)
    pub fname: String,
    /// the content read at open time
    src_data: Option<Rc<Vec<u8>>>,
    pub open_flags: u32,
    pub flags: u32,
    pub ch_flags: u32,
    pub error: ZipError,
    pub default_password: Option<Vec<u8>>,
    pub comment_orig: Option<ZString>,
    pub comment_changes: Option<ZString>,
    pub comment_changed: bool,
    pub entries: Vec<Entry>,
    /// name -> (orig_index, current_index)
    names: HashMap<Vec<u8>, (i64, i64)>,
    torrent_mtime: i64,
    /// cleared when the archive is discarded (_zip_source_invalidate())
    alive: Rc<Cell<bool>>,
}

impl Drop for Archive {
    fn drop(&mut self) {
        self.alive.set(false);
    }
}

/// The decoded data of an opened entry (zip_file_t).
pub struct ZipFile {
    pub data: Vec<u8>,
    pub pos: usize,
    /// error reported when reaching the end of the data
    pub end_error: Option<ZipError>,
    pub error: ZipError,
    pub eof: bool,
    pub seekable: bool,
    /// the containing archive is still open
    pub alive: Rc<Cell<bool>>,
}

impl ZipFile {
    /// zip_fread()
    pub fn read(&mut self, n: usize) -> Result<Vec<u8>, ZipError> {
        if !self.error.is_ok() {
            return Err(self.error);
        }
        if !self.alive.get() {
            self.error = ZipError::new(ER_ZIPCLOSED, 0);
            return Err(self.error);
        }
        if n == 0 || self.eof {
            return Ok(Vec::new());
        }
        let avail = self.data.len() - self.pos.min(self.data.len());
        let take = n.min(avail);
        let out = self.data[self.pos..self.pos + take].to_vec();
        self.pos += take;
        if take < n {
            // the lower layers reach their end
            if let Some(e) = self.end_error.take() {
                self.error = e;
                if take == 0 {
                    return Err(e);
                }
            } else {
                self.eof = true;
            }
        }
        Ok(out)
    }

    /// zip_fseek(): false on error
    pub fn seek(&mut self, offset: i64, whence: i64) -> bool {
        if !self.seekable {
            self.error.set(ER_OPNOTSUPP, 0);
            return false;
        }
        let base = match whence {
            0 => 0,
            1 => self.pos as i64,
            2 => self.data.len() as i64,
            _ => {
                self.error.set(ER_INVAL, 0);
                return false;
            }
        };
        let np = base + offset;
        if np < 0 || np as usize > self.data.len() {
            self.error.set(ER_INVAL, 0);
            return false;
        }
        self.pos = np as usize;
        self.eof = false;
        true
    }

    pub fn tell(&self) -> i64 {
        self.pos as i64
    }
}

fn find_eocd(buf: &[u8], last: Option<usize>) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    let start = match last {
        None => buf.len() - 4,
        Some(0) => return None,
        Some(l) => l - 1,
    };
    let mut p = start as isize;
    while p >= 0 {
        let i = p as usize;
        if buf[i] == b'P' && &buf[i..i + 4] == EOCD_MAGIC {
            return Some(i);
        }
        p -= 1;
    }
    None
}

struct Cdir {
    entries: Vec<Dirent>,
    size: u64,
    offset: u64,
    comment: Option<ZString>,
    is_zip64: bool,
    this_disk: u32,
    eocd_disk: u32,
    disk_entries: u64,
    num_entries: u64,
    eocd_offset: u64,
}

impl Archive {
    fn allocate_new(fname: &str, flags: u32) -> Archive {
        let mut a = Archive {
            fname: fname.to_string(),
            src_data: None,
            open_flags: flags,
            flags: 0,
            ch_flags: 0,
            error: ZipError::default(),
            default_password: None,
            comment_orig: None,
            comment_changes: None,
            comment_changed: false,
            entries: Vec::new(),
            names: HashMap::new(),
            torrent_mtime: 0,
            alive: Rc::new(Cell::new(true)),
        };
        if flags & ZIP_RDONLY != 0 {
            a.flags |= AFL_RDONLY;
            a.ch_flags |= AFL_RDONLY;
        }
        a
    }

    pub fn is_rdonly(&self) -> bool {
        self.ch_flags & AFL_RDONLY != 0
    }

    fn want_torrentzip(&self) -> bool {
        self.ch_flags & AFL_WANT_TORRENTZIP != 0
    }

    fn is_torrentzip(&self) -> bool {
        self.flags & AFL_IS_TORRENTZIP != 0
    }

    /// zip_open(): Err(error code)
    pub fn open(fname: &str, flags: u32) -> Result<Archive, ZipError> {
        // zip_source_file_create(): stat the file
        let md = std::fs::metadata(fname);
        let exists = match &md {
            Ok(_) => true,
            Err(e) if e.raw_os_error() == Some(ENOENT) => false,
            Err(e) => return Err(ZipError::from_io(ER_READ, e)),
        };
        if exists && !md.as_ref().unwrap().is_file() {
            // zip_source_supports(): not seekable
            return Err(ZipError::new(ER_OPNOTSUPP, 0));
        }
        if flags & (ZIP_RDONLY | ZIP_TRUNCATE) == (ZIP_RDONLY | ZIP_TRUNCATE) {
            return Err(ZipError::new(ER_RDONLY, 0));
        }
        if !exists {
            if flags & ZIP_CREATE == 0 {
                return Err(ZipError::new(ER_NOENT, 0));
            }
            return Ok(Archive::allocate_new(fname, flags));
        }
        if flags & ZIP_EXCL != 0 {
            return Err(ZipError::new(ER_EXISTS, 0));
        }
        // zip_source_open()
        let data = std::fs::read(fname).map_err(|e| ZipError::from_io(ER_OPEN, &e))?;
        if flags & ZIP_TRUNCATE != 0 {
            let mut a = Archive::allocate_new(fname, flags);
            a.src_data = Some(Rc::new(data));
            return Ok(a);
        }
        Archive::open_data(fname, flags, data)
    }

    /// _zip_open()
    fn open_data(fname: &str, flags: u32, data: Vec<u8>) -> Result<Archive, ZipError> {
        let len = data.len() as u64;
        let mut za = Archive::allocate_new(fname, flags);
        za.src_data = Some(Rc::new(data));
        if len == 0 {
            return Ok(za);
        }
        let cdir = match za.find_central_dir() {
            Ok(c) => c,
            Err(mut e) => {
                if e.zip_err == ER_NOZIP {
                    let d = za.src_data.as_ref().unwrap();
                    if d.len() >= 4 && &d[..4] == LOCAL_MAGIC {
                        e = ZipError::new(ER_TRUNCATED_ZIP, 0);
                    }
                }
                return Err(e);
            }
        };
        let comment = cdir.comment.clone();
        za.entries = cdir
            .entries
            .into_iter()
            .map(|d| Entry {
                orig: Some(d),
                ..Default::default()
            })
            .collect();
        za.check_torrentzip(cdir.size, cdir.offset, comment.as_ref());
        if !za.is_torrentzip() {
            za.comment_orig = comment;
        }
        for idx in 0..za.entries.len() {
            let name = string_get(za.entries[idx].orig.as_ref().unwrap().filename.as_ref(), 0);
            if let Err(e) = za.hash_add(&name, idx as u64, FL_UNCHANGED) {
                if e.zip_err != ER_EXISTS || flags & ZIP_CHECKCONS != 0 {
                    return Err(e);
                }
            }
        }
        za.ch_flags = za.flags;
        Ok(za)
    }

    fn src(&self) -> &[u8] {
        self.src_data.as_deref().map_or(&[][..], |v| &v[..])
    }

    /// _zip_find_central_dir()
    fn find_central_dir(&mut self) -> Zr<Cdir> {
        let len = self.src().len();
        if len < EOCDLEN {
            return Err(ZipError::new(ER_NOZIP, 0));
        }
        let buflen = len.min(CDBUFSIZE);
        let buf_offset = (len - buflen) as u64;
        let data = self.src_data.clone().unwrap();
        let buffer = &data[len - buflen..];
        let min_pos = if buflen >= CDBUFSIZE { EOCD64LOCLEN } else { 0 };
        let mut error = ZipError::new(ER_NOZIP, 0);
        let mut m: Option<usize> = None;
        let mut result: Option<Cdir> = None;
        loop {
            // find_eocd() searches from the current offset to the end
            let found = find_eocd(buffer, m);
            let Some(p) = found else { break };
            if p < min_pos {
                break;
            }
            m = Some(p);
            match self.read_cdir(buffer, buf_offset, p) {
                Some(Ok(cd)) => {
                    if self.open_flags & ZIP_CHECKCONS != 0 {
                        let mut cd = cd;
                        if let Err(e) = self.checkcons(&mut cd) {
                            error = e;
                            break;
                        }
                        result = Some(cd);
                    } else {
                        result = Some(cd);
                    }
                    break;
                }
                Some(Err(e)) => {
                    error = e;
                    break;
                }
                None => continue,
            }
        }
        match result {
            Some(cd) => Ok(cd),
            None => Err(error),
        }
    }

    /// _zip_read_cdir(): None = not this EOCD (keep searching)
    fn read_cdir(&self, buffer: &[u8], buf_offset: u64, eocd_offset: usize) -> Option<Zr<Cdir>> {
        let src = self.src();
        let mut b = Rd::new(&buffer[eocd_offset..]);
        if b.left() < EOCDLEN {
            return Some(Err(ZipError::new(ER_INCONS, DETAIL_EOCD_LENGTH_INVALID)));
        }
        b.get(4);
        let mut cd = Cdir {
            entries: Vec::new(),
            this_disk: b.get16() as u32,
            eocd_disk: b.get16() as u32,
            disk_entries: b.get16() as u64,
            num_entries: b.get16() as u64,
            size: b.get32() as u64,
            offset: b.get32() as u64,
            comment: None,
            is_zip64: false,
            eocd_offset: buf_offset + eocd_offset as u64,
        };
        let mut eocd64_found = false;
        if eocd_offset >= EOCD64LOCLEN
            && &buffer[eocd_offset - EOCD64LOCLEN..eocd_offset - EOCD64LOCLEN + 4]
                == EOCD64LOC_MAGIC
        {
            eocd64_found = true;
            match self.read_eocd64(&mut cd, buffer, buf_offset, eocd_offset - EOCD64LOCLEN) {
                Ok(true) => {}
                Ok(false) => return None,
                Err(e) => return Some(Err(e)),
            }
        }
        if (cd.eocd_disk != 0 || cd.this_disk != 0) && !eocd64_found && cd.eocd_disk != cd.this_disk
        {
            if cd.this_disk < cd.eocd_disk {
                return None;
            }
            if cd.size <= cd.eocd_offset {
                return None;
            }
        }
        if !eocd64_found {
            if cd.this_disk == 0
                && cd.eocd_disk == 0
                && cd.eocd_offset == 0
                && cd.offset == 0
                && cd.num_entries == 0
            {
                // empty archive
            } else {
                let o = cd.offset as usize;
                if src.len() < o + 4 || &src[o..o + 4] != CENTRAL_MAGIC {
                    return None;
                }
            }
        }
        // check_eocd()
        if cd.disk_entries != cd.num_entries || cd.this_disk != 0 || cd.eocd_disk != 0 {
            return Some(Err(ZipError::new(ER_MULTIDISK, 0)));
        }
        if cd.offset.checked_add(cd.size).is_none() {
            return Some(Err(ZipError::new(ER_SEEK, EFBIG)));
        }
        if self.open_flags & ZIP_CHECKCONS != 0 && cd.offset + cd.size != cd.eocd_offset {
            return Some(Err(ZipError::new(ER_INCONS, DETAIL_CDIR_LENGTH_INVALID)));
        }
        let comment_len_pos = eocd_offset + 20;
        let mut comment_len =
            u16::from_le_bytes([buffer[comment_len_pos], buffer[comment_len_pos + 1]]) as usize;
        if cd.offset + cd.size > buf_offset + eocd_offset as u64 {
            return Some(Err(ZipError::new(ER_INCONS, DETAIL_CDIR_OVERLAPS_EOCD)));
        }
        if comment_len > 0 || self.open_flags & ZIP_CHECKCONS != 0 {
            let tail = &buffer[eocd_offset + EOCDLEN..];
            if tail.len() != comment_len {
                if self.open_flags & ZIP_CHECKCONS != 0 {
                    return Some(Err(ZipError::new(ER_INCONS, DETAIL_COMMENT_LENGTH_INVALID)));
                }
                if tail.len() < comment_len {
                    comment_len = tail.len();
                }
            }
            if comment_len > 0 {
                cd.comment = Some(ZString::raw_new(tail[..comment_len].to_vec()));
            }
        }
        let cd_start = cd.offset as usize;
        let cd_end = cd_start + cd.size as usize;
        if cd_end > src.len() {
            return Some(Err(ZipError::new(ER_INCONS, DETAIL_CDIR_LENGTH_INVALID)));
        }
        let cdbuf = &src[cd_start..cd_end];
        let mut pos = 0usize;
        let mut left = cd.size;
        let mut nalloc = cd.num_entries;
        let mut i: u64 = 0;
        while left > 0 {
            let mut grown = false;
            if i == nalloc {
                if cd.is_zip64 || left < CDENTRYSIZE as u64 {
                    break;
                }
                nalloc += 0x10000;
                grown = true;
            }
            match dirent_read(
                &cdbuf[pos..],
                false,
                0,
                self.open_flags & ZIP_CHECKCONS != 0,
            ) {
                Ok((de, sz)) => {
                    cd.entries.push(de);
                    pos += sz;
                    left -= sz as u64;
                }
                Err(mut e) => {
                    if e.zip_err == ER_INCONS {
                        e = ZipError::new(ER_INCONS, add_index_to_detail(e.sys_err, i));
                    } else if grown && e.zip_err == ER_NOZIP {
                        e = ZipError::new(
                            ER_INCONS,
                            detail_with_index(DETAIL_CDIR_ENTRY_INVALID, i),
                        );
                    }
                    return Some(Err(e));
                }
            }
            i += 1;
        }
        if i != nalloc || left > 0 {
            return Some(Err(ZipError::new(
                ER_INCONS,
                DETAIL_CDIR_WRONG_ENTRIES_COUNT,
            )));
        }
        Some(Ok(cd))
    }

    /// _zip_read_eocd64(): Ok(false) = CDIR_NOT_FOUND
    fn read_eocd64(&self, cd: &mut Cdir, buffer: &[u8], buf_offset: u64, loc: usize) -> Zr<bool> {
        let src = self.src();
        let mut b = Rd::new(&buffer[loc..]);
        b.get(4);
        let eocd_disk = b.get32();
        let eocd_offset = b.get64();
        let num_disks = b.get32();
        let eo = eocd_offset as usize;
        if src.len() < eo.saturating_add(4) || &src[eo..eo + 4] != EOCD64_MAGIC {
            return Ok(false);
        }
        if num_disks != 1 {
            return Err(ZipError::new(ER_MULTIDISK, 0));
        }
        if eocd_offset > i64::MAX as u64 {
            return Err(ZipError::new(ER_SEEK, EFBIG));
        }
        if eocd_offset + EOCD64LEN as u64 > loc as u64 + buf_offset {
            return Err(ZipError::new(ER_INCONS, DETAIL_EOCD64_OVERLAPS_EOCD));
        }
        let mut e = Rd::new(&src[eo..]);
        if e.get(4) != Some(EOCD64_MAGIC) {
            return Err(ZipError::new(ER_INCONS, DETAIL_EOCD64_WRONG_MAGIC));
        }
        let size = e.get64();
        if self.open_flags & ZIP_CHECKCONS != 0
            && size + eocd_offset + 12 != buf_offset + loc as u64
        {
            return Err(ZipError::new(ER_INCONS, DETAIL_EOCD64_OVERLAPS_EOCD));
        }
        e.get(4);
        let this_disk = e.get32();
        if e.get32() != eocd_disk {
            return Err(ZipError::new(ER_INCONS, DETAIL_EOCD64_LOCATOR_MISMATCH));
        }
        let i = e.get64();
        let nentry = e.get64();
        if nentry != i {
            return Err(ZipError::new(ER_MULTIDISK, 0));
        }
        let size = e.get64();
        let offset = e.get64();
        if !e.ok {
            return Err(ZipError::new(ER_INTERNAL, 0));
        }
        if offset > i64::MAX as u64 || offset.checked_add(size).is_none() {
            return Err(ZipError::new(ER_SEEK, EFBIG));
        }
        if nentry > size / CDENTRYSIZE as u64 {
            return Err(ZipError::new(ER_INCONS, DETAIL_CDIR_INVALID));
        }
        if (cd.size != 0xffffffff && cd.size != size)
            || (cd.offset != 0xffffffff && cd.offset != offset)
            || (cd.num_entries != 0xffff && cd.num_entries != nentry)
            || (cd.disk_entries != 0xffff && cd.disk_entries != i)
            || (cd.this_disk != 0xffff && cd.this_disk != this_disk)
            || (cd.eocd_disk != 0xffff && cd.eocd_disk != eocd_disk)
        {
            return Err(ZipError::new(ER_INCONS, DETAIL_EOCD64_MISMATCH));
        }
        cd.is_zip64 = true;
        cd.size = size;
        cd.offset = offset;
        cd.disk_entries = i;
        cd.num_entries = nentry;
        cd.this_disk = this_disk;
        cd.eocd_disk = eocd_disk;
        Ok(true)
    }

    /// _zip_checkcons()
    fn checkcons(&self, cd: &mut Cdir) -> Zr<()> {
        let src = self.src();
        let (mut min, mut max) = match cd.entries.first() {
            Some(e) => (e.offset, e.offset),
            None => (0, 0),
        };
        for i in 0..cd.entries.len() {
            let e = &cd.entries[i];
            if e.offset < min {
                min = e.offset;
            }
            if min > cd.offset {
                return Err(ZipError::new(ER_NOZIP, 0));
            }
            let j =
                e.offset + e.comp_size + string_len(e.filename.as_ref()) as u64 + LENTRYSIZE as u64;
            if j > max {
                max = j;
            }
            if max > cd.offset {
                return Err(ZipError::new(ER_NOZIP, 0));
            }
            let off = e.offset as usize;
            let local =
                match dirent_read(src.get(off..).unwrap_or_default(), true, e.comp_size, true) {
                    Ok((l, _)) => l,
                    Err(mut er) => {
                        if er.zip_err == ER_INCONS {
                            er =
                                ZipError::new(ER_INCONS, add_index_to_detail(er.sys_err, i as u64));
                        }
                        return Err(er);
                    }
                };
            if headercomp(e, &local) {
                return Err(ZipError::new(
                    ER_INCONS,
                    detail_with_index(DETAIL_ENTRY_HEADER_MISMATCH, i as u64),
                ));
            }
            let e = &mut cd.entries[i];
            e.extra_fields = ef_merge(std::mem::take(&mut e.extra_fields), local.extra_fields);
            e.local_extra_fields_read = true;
            let d = e.check_consistency();
            if d != 0 {
                return Err(ZipError::new(ER_INCONS, detail_with_index(d, i as u64)));
            }
        }
        Ok(())
    }

    /// zip_check_torrentzip()
    fn check_torrentzip(&mut self, size: u64, offset: u64, comment: Option<&ZString>) {
        let Some(c) = comment else { return };
        if c.raw.len() != TORRENTZIP_SIGNATURE.len() + 8 || !c.raw.starts_with(TORRENTZIP_SIGNATURE)
        {
            return;
        }
        let hex = &c.raw[TORRENTZIP_SIGNATURE.len()..];
        let mut crc_should: u32 = 0;
        for pair in hex.chunks(2) {
            let d = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'A'..=b'F' => Some(c - b'A' + 10),
                _ => None,
            };
            let (Some(h), Some(l)) = (d(pair[0]), d(pair[1])) else {
                return;
            };
            crc_should = (crc_should << 8) + ((h as u32) << 4) + l as u32;
        }
        let src = self.src();
        let (s, e) = (offset as usize, (offset + size) as usize);
        if e > src.len() {
            return;
        }
        if goro_zlib::crc32(0, &src[s..e]) != crc_should {
            return;
        }
        self.flags |= AFL_IS_TORRENTZIP;
    }

    // -- names ------------------------------------------------------------------

    /// _zip_hash_add()
    fn hash_add(&mut self, name: &[u8], index: u64, flags: u32) -> Zr<()> {
        let e = self.names.entry(name.to_vec()).or_insert((-1, -1));
        if (flags & FL_UNCHANGED != 0 && e.0 != -1) || e.1 != -1 {
            return Err(ZipError::new(ER_EXISTS, 0));
        }
        if flags & FL_UNCHANGED != 0 {
            e.0 = index as i64;
        }
        e.1 = index as i64;
        Ok(())
    }

    /// _zip_hash_delete()
    fn hash_delete(&mut self, name: &[u8]) -> Zr<()> {
        match self.names.get_mut(name) {
            Some(e) => {
                if e.0 == -1 {
                    self.names.remove(name);
                } else {
                    e.1 = -1;
                }
                Ok(())
            }
            None => Err(ZipError::new(ER_NOENT, 0)),
        }
    }

    fn hash_lookup(&self, name: &[u8], flags: u32) -> Zr<i64> {
        if let Some(&(o, c)) = self.names.get(name) {
            if flags & FL_UNCHANGED != 0 {
                if o != -1 {
                    return Ok(o);
                }
            } else if c != -1 {
                return Ok(c);
            }
        }
        Err(ZipError::new(ER_NOENT, 0))
    }

    fn hash_revert(&mut self) {
        self.names.retain(|_, e| e.0 != -1);
        for e in self.names.values_mut() {
            e.1 = e.0;
        }
    }

    /// _zip_get_dirent()
    pub fn get_dirent(&self, idx: u64, flags: u32) -> Zr<&Dirent> {
        let Some(e) = self.entries.get(idx as usize) else {
            return Err(ZipError::new(ER_INVAL, 0));
        };
        match &e.changes {
            Some(c) if flags & FL_UNCHANGED == 0 => Ok(c),
            _ => {
                let Some(o) = &e.orig else {
                    return Err(ZipError::new(ER_INVAL, 0));
                };
                if e.deleted && flags & FL_UNCHANGED == 0 {
                    return Err(ZipError::new(ER_DELETED, 0));
                }
                Ok(o)
            }
        }
    }

    fn get_dirent_mut(&mut self, idx: u64, flags: u32) -> Zr<&mut Dirent> {
        self.get_dirent(idx, flags)?;
        let e = &mut self.entries[idx as usize];
        match &mut e.changes {
            Some(c) if flags & FL_UNCHANGED == 0 => Ok(c),
            _ => Ok(e.orig.as_mut().unwrap()),
        }
    }

    /// _zip_get_name()
    pub fn get_name_e(&self, idx: u64, flags: u32) -> Zr<Vec<u8>> {
        let de = self.get_dirent(idx, flags)?;
        Ok(string_get(de.filename.as_ref(), flags))
    }

    /// zip_get_name(): sets the archive error
    pub fn get_name(&mut self, idx: u64, flags: u32) -> Option<Vec<u8>> {
        match self.get_name_e(idx, flags) {
            Ok(n) => Some(n),
            Err(e) => {
                self.error = e;
                None
            }
        }
    }

    /// _zip_name_locate()
    fn name_locate_e(&self, fname: &[u8], flags: u32) -> Zr<i64> {
        let fname = c_str(fname);
        if fname.len() > 0xffff {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        let mut name = fname.to_vec();
        if flags & (FL_ENC_UTF_8 | FL_ENC_RAW) == 0 && !fname.is_empty() {
            let s = ZString::new(fname, flags)?;
            name = string_get(s.as_ref(), 0);
        }
        if flags & (FL_NOCASE | FL_NODIR | FL_ENC_RAW | FL_ENC_STRICT) != 0 {
            for i in 0..self.entries.len() {
                let Ok(fnm) = self.get_name_e(i as u64, flags) else {
                    continue;
                };
                let mut f = &fnm[..];
                if flags & FL_NODIR != 0 {
                    if let Some(p) = f.iter().rposition(|&c| c == b'/') {
                        f = &f[p + 1..];
                    }
                }
                let eq = if flags & FL_NOCASE != 0 {
                    c_str(&name).eq_ignore_ascii_case(c_str(f))
                } else {
                    c_str(&name) == c_str(f)
                };
                if eq {
                    return Ok(i as i64);
                }
            }
            return Err(ZipError::new(ER_NOENT, 0));
        }
        self.hash_lookup(&name, flags)
    }

    /// zip_name_locate()
    pub fn name_locate(&mut self, fname: &[u8], flags: u32) -> i64 {
        match self.name_locate_e(fname, flags) {
            Ok(i) => {
                if flags & (FL_NOCASE | FL_NODIR | FL_ENC_RAW | FL_ENC_STRICT) != 0 {
                    self.error.clear();
                }
                i
            }
            Err(e) => {
                self.error = e;
                -1
            }
        }
    }

    /// zip_get_num_entries()
    pub fn num_entries(&self, flags: u32) -> i64 {
        if flags & FL_UNCHANGED != 0 {
            let mut n = self.entries.len();
            while n > 0 && self.entries[n - 1].orig.is_none() {
                n -= 1;
            }
            return n as i64;
        }
        self.entries.len() as i64
    }

    /// zip_stat_index()
    pub fn stat_index(&mut self, index: u64, flags: u32) -> Option<Stat> {
        match self.stat_index_e(index, flags) {
            Ok(s) => Some(s),
            Err(e) => {
                self.error = e;
                None
            }
        }
    }

    fn stat_index_e(&mut self, index: u64, flags: u32) -> Zr<Stat> {
        self.get_dirent(index, flags)?;
        let name = self.get_name_e(index, flags)?;
        let data_changed = self.entries[index as usize].data_changed();
        let mut st;
        if flags & FL_UNCHANGED == 0 && data_changed {
            let e = &self.entries[index as usize];
            st = e.source.as_ref().unwrap().stat();
            let de_comp = self.get_dirent(index, flags)?.comp_method;
            if ZipInt::cm_is_default(de_comp) {
                if st.valid & STAT_COMP_METHOD == 0 || st.comp_method == CM_STORE {
                    st.valid &= !(STAT_COMP_SIZE | STAT_COMP_METHOD);
                }
            } else {
                if st.valid & STAT_COMP_METHOD != 0 && st.comp_method as i32 != de_comp {
                    st.valid &= !STAT_COMP_SIZE;
                }
                st.valid |= STAT_COMP_METHOD;
                st.comp_method = de_comp as u16;
            }
            if st.valid & (STAT_COMP_METHOD | STAT_SIZE) == (STAT_COMP_METHOD | STAT_SIZE)
                && st.comp_method == CM_STORE
            {
                st.valid |= STAT_COMP_SIZE;
                st.comp_size = st.size;
            }
            let lm = self.entries[index as usize].changed(DIRENT_LAST_MOD);
            if lm {
                let de = self.get_dirent_mut(index, flags)?;
                st.mtime = de.mtime();
                st.valid |= STAT_MTIME;
            }
        } else {
            let comp_changed = self.entries[index as usize].changed(DIRENT_COMP_METHOD);
            let de = self.get_dirent_mut(index, flags)?;
            st = Stat {
                crc: de.crc,
                size: de.uncomp_size,
                mtime: de.mtime(),
                comp_size: de.comp_size,
                comp_method: de.comp_method as u16,
                encryption_method: de.encryption_method,
                valid: if de.crc_valid { STAT_CRC } else { 0 }
                    | STAT_SIZE
                    | STAT_MTIME
                    | STAT_COMP_SIZE
                    | STAT_COMP_METHOD
                    | STAT_ENCRYPTION_METHOD,
                ..Default::default()
            };
            if comp_changed {
                st.valid &= !STAT_COMP_SIZE;
            }
        }
        if self.ch_flags & AFL_WANT_TORRENTZIP != 0 && flags & FL_UNCHANGED == 0 {
            if self.torrent_mtime == 0 {
                self.torrent_mtime = d2u_time(DosTime {
                    time: 0xbc00,
                    date: 0x2198,
                });
            }
            st.comp_method = CM_DEFLATE;
            st.mtime = self.torrent_mtime;
            st.valid |= STAT_MTIME | STAT_COMP_METHOD;
            st.valid &= !STAT_COMP_SIZE;
        }
        st.index = index;
        st.name = name;
        st.valid |= STAT_INDEX | STAT_NAME;
        Ok(st)
    }

    /// zip_stat()
    pub fn stat_name(&mut self, fname: &[u8], flags: u32) -> Option<Stat> {
        let idx = self.name_locate(fname, flags);
        if idx < 0 {
            return None;
        }
        self.stat_index(idx as u64, flags)
    }

    // -- changes ------------------------------------------------------------------

    fn set_err<T>(&mut self, r: Zr<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.error = e;
                None
            }
        }
    }

    /// _zip_set_name()
    fn set_name(&mut self, idx: u64, name: Option<&[u8]>, flags: u32) -> Zr<()> {
        if idx as usize >= self.entries.len() {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        if self.is_rdonly() {
            return Err(ZipError::new(ER_RDONLY, 0));
        }
        let name = name.map(c_str);
        let s = match name {
            Some(n) if !n.is_empty() => {
                let s = ZString::new(n, flags)?;
                if let Some(s) = &s {
                    if flags & FL_ENCODING_ALL == FL_ENC_GUESS
                        && s.guess(Enc::Unknown) == Enc::Utf8Guessed
                    {
                        s.set_enc(Enc::Utf8Known);
                    }
                }
                s
            }
            _ => None,
        };
        let loc = self.name_locate_e(name.unwrap_or_default(), 0);
        if let Ok(i) = loc {
            if i as u64 != idx {
                return Err(ZipError::new(ER_EXISTS, 0));
            }
            return Ok(());
        }
        let e = &self.entries[idx as usize];
        let same_as_orig = match &e.orig {
            Some(o) => string_equal(o.filename.as_ref(), s.as_ref()),
            None => false,
        };
        if !same_as_orig && e.changes.is_none() {
            let c = Dirent::clone_of(e.orig.as_ref());
            self.entries[idx as usize].changes = Some(c);
        }
        let e = &self.entries[idx as usize];
        let new_name = string_get(
            if same_as_orig {
                e.orig.as_ref().unwrap().filename.as_ref()
            } else {
                s.as_ref()
            },
            0,
        );
        let old_str = match (&e.changes, &e.orig) {
            (Some(c), _) => c.filename.clone(),
            (None, Some(o)) => o.filename.clone(),
            _ => None,
        };
        let old_name = old_str.as_ref().map(|o| string_get(Some(o), 0));
        self.hash_add(&new_name, idx, 0)?;
        if let Some(on) = old_name {
            let _ = self.hash_delete(&on);
        }
        let e = &mut self.entries[idx as usize];
        if same_as_orig {
            if let Some(c) = &mut e.changes {
                if c.changed & DIRENT_FILENAME != 0 {
                    c.changed &= !DIRENT_FILENAME;
                    if c.changed == 0 {
                        e.changes = None;
                    } else {
                        c.filename = e.orig.as_ref().unwrap().filename.clone();
                    }
                }
            }
        } else {
            let c = e.changes.as_mut().unwrap();
            c.changed |= DIRENT_FILENAME;
            c.filename = s;
        }
        Ok(())
    }

    /// _zip_file_extra_field_prepare_for_change() + delete all
    fn extra_field_delete_all(&mut self, idx: u64, flags: u16) -> Zr<()> {
        self.get_dirent(idx, 0)?;
        if self.is_rdonly() {
            return Err(ZipError::new(ER_RDONLY, 0));
        }
        let has = self.entries[idx as usize].changed(DIRENT_EXTRA_FIELD);
        if !has {
            if self.entries[idx as usize].orig.is_some() {
                self.read_local_ef(idx)?;
            }
            let e = &mut self.entries[idx as usize];
            if e.changes.is_none() {
                e.changes = Some(Dirent::clone_of(e.orig.as_ref()));
            }
            let oef = e
                .orig
                .as_ref()
                .map(|o| o.extra_fields.clone())
                .unwrap_or_default();
            let c = e.changes.as_mut().unwrap();
            if !oef.is_empty() {
                c.extra_fields = oef;
            }
            c.changed |= DIRENT_EXTRA_FIELD;
        }
        let c = self.entries[idx as usize].changes.as_mut().unwrap();
        c.extra_fields = ef_delete_all(std::mem::take(&mut c.extra_fields), flags);
        Ok(())
    }

    /// _zip_read_local_ef()
    fn read_local_ef(&mut self, idx: u64) -> Zr<()> {
        let Some(e) = self.entries.get(idx as usize) else {
            return Err(ZipError::new(ER_INVAL, 0));
        };
        let Some(o) = &e.orig else { return Ok(()) };
        if o.local_extra_fields_read {
            return Ok(());
        }
        let off = o.offset as usize;
        let src = self.src();
        if src.len() < off + 30 {
            return Err(ZipError::new(ER_READ, 0));
        }
        let fname_len = u16::from_le_bytes([src[off + 26], src[off + 27]]) as usize;
        let ef_len = u16::from_le_bytes([src[off + 28], src[off + 29]]) as usize;
        let mut ef = Vec::new();
        if ef_len > 0 {
            let s = off + 30 + fname_len;
            if src.len() < s + ef_len {
                return Err(ZipError::new(ER_EOF, 0));
            }
            ef = ef_parse(&src[s..s + ef_len], EF_LOCAL)?;
            ef.retain(|e| !ef_is_internal(e.id));
        }
        let e = &mut self.entries[idx as usize];
        let o = e.orig.as_mut().unwrap();
        o.extra_fields = ef_merge(std::mem::take(&mut o.extra_fields), ef);
        o.local_extra_fields_read = true;
        let oef = o.extra_fields.clone();
        if let Some(c) = &mut e.changes {
            if !c.local_extra_fields_read {
                c.extra_fields = oef;
                c.local_extra_fields_read = true;
            }
        }
        Ok(())
    }

    /// _zip_file_replace(): the index
    pub fn file_replace(
        &mut self,
        idx: Option<u64>,
        name: Option<&[u8]>,
        source: Source,
        flags: u32,
    ) -> i64 {
        let r = self.file_replace_e(idx, name, source, flags);
        self.set_err(r).unwrap_or(-1)
    }

    fn file_replace_e(
        &mut self,
        idx: Option<u64>,
        name: Option<&[u8]>,
        source: Source,
        flags: u32,
    ) -> Zr<i64> {
        if self.is_rdonly() {
            return Err(ZipError::new(ER_RDONLY, 0));
        }
        let prev = self.entries.len();
        let idx = match idx {
            Some(i) => i,
            None => {
                let mut i = -1i64;
                if flags & FL_OVERWRITE != 0 {
                    i = self
                        .name_locate_e(name.unwrap_or_default(), flags)
                        .unwrap_or(-1);
                }
                if i == -1 {
                    self.entries.push(Entry::default());
                    i = (self.entries.len() - 1) as i64;
                }
                i as u64
            }
        };
        if let Some(n) = name {
            if let Err(e) = self.set_name(idx, Some(n), flags) {
                if self.entries.len() != prev {
                    self.entries.truncate(prev);
                }
                return Err(e);
            }
        }
        self.extra_field_delete_all(idx, EF_BOTH)?;
        let e = &mut self.entries[idx as usize];
        e.unchange_data();
        if e.orig.is_some() && !e.changed(DIRENT_COMP_METHOD) {
            if e.changes.is_none() {
                e.changes = Some(Dirent::clone_of(e.orig.as_ref()));
            }
            let c = e.changes.as_mut().unwrap();
            c.comp_method = CM_REPLACED_DEFAULT;
            c.changed |= DIRENT_COMP_METHOD;
        }
        e.source = Some(source);
        Ok(idx as i64)
    }

    /// zip_dir_add()
    pub fn dir_add(&mut self, name: &[u8], flags: u32) -> i64 {
        if self.is_rdonly() {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        let name = c_str(name);
        let mut s = name.to_vec();
        if name.last() != Some(&b'/') {
            s.push(b'/');
        }
        let idx = self.file_replace(None, Some(&s), Source::buffer(Vec::new()), flags);
        if idx >= 0
            && self.file_set_external_attributes(
                idx as u64,
                0,
                OPSYS_DEFAULT,
                EXT_ATTRIB_DEFAULT_DIR,
            ) < 0
        {
            self.delete(idx as u64);
            return -1;
        }
        idx
    }

    /// zip_delete()
    pub fn delete(&mut self, idx: u64) -> i32 {
        let r = self.delete_e(idx);
        self.set_err(r).map_or(-1, |_| 0)
    }

    fn delete_e(&mut self, idx: u64) -> Zr<()> {
        if idx as usize >= self.entries.len() {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        if self.is_rdonly() {
            return Err(ZipError::new(ER_RDONLY, 0));
        }
        let name = self.get_name_e(idx, 0)?;
        self.hash_delete(&name)?;
        self.unchange_e(idx, true)?;
        self.entries[idx as usize].deleted = true;
        Ok(())
    }

    /// _zip_unchange()
    fn unchange_e(&mut self, idx: u64, allow_duplicates: bool) -> Zr<()> {
        if idx as usize >= self.entries.len() {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        let e = &self.entries[idx as usize];
        let renamed = e.changed(DIRENT_FILENAME);
        if !allow_duplicates && (renamed || e.deleted) {
            let mut orig_name = None;
            if e.orig.is_some() {
                let on = self.get_name_e(idx, FL_UNCHANGED)?;
                if let Ok(i) = self.name_locate_e(&on, 0) {
                    if i as u64 != idx {
                        return Err(ZipError::new(ER_EXISTS, 0));
                    }
                }
                orig_name = Some(on);
            }
            let changed_name = if renamed {
                Some(self.get_name_e(idx, 0)?)
            } else {
                None
            };
            if let Some(on) = &orig_name {
                self.hash_add(on, idx, 0)?;
            }
            if let Some(cn) = &changed_name {
                if let Err(e) = self.hash_delete(cn) {
                    if let Some(on) = &orig_name {
                        let _ = self.hash_delete(on);
                    }
                    return Err(e);
                }
            }
        }
        let e = &mut self.entries[idx as usize];
        e.changes = None;
        e.unchange_data();
        Ok(())
    }

    pub fn unchange(&mut self, idx: u64) -> i32 {
        let r = self.unchange_e(idx, false);
        self.set_err(r).map_or(-1, |_| 0)
    }

    /// zip_unchange_all()
    pub fn unchange_all(&mut self) -> i32 {
        self.hash_revert();
        let mut ret = 0;
        for i in 0..self.entries.len() {
            if self.unchange_e(i as u64, true).is_err() {
                ret = -1;
            }
        }
        self.unchange_archive();
        ret
    }

    /// zip_unchange_archive()
    pub fn unchange_archive(&mut self) -> i32 {
        if self.comment_changed {
            self.comment_changes = None;
            self.comment_changed = false;
        }
        self.ch_flags = self.flags;
        0
    }

    /// zip_file_rename()
    pub fn file_rename(&mut self, idx: u64, name: &[u8], flags: u32) -> i32 {
        let name = c_str(name);
        if idx as usize >= self.entries.len() || name.len() > 0xffff {
            self.error.set(ER_INVAL, 0);
            return -1;
        }
        if self.is_rdonly() {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        let Some(old) = self.get_name(idx, 0) else {
            return -1;
        };
        let new_is_dir = name.last() == Some(&b'/');
        let old_is_dir = c_str(&old).last() == Some(&b'/');
        if new_is_dir != old_is_dir {
            self.error.set(ER_INVAL, 0);
            return -1;
        }
        let r = self.set_name(idx, Some(name), flags);
        self.set_err(r).map_or(-1, |_| 0)
    }

    /// zip_file_set_comment()
    pub fn file_set_comment(&mut self, idx: u64, comment: &[u8], flags: u32) -> i32 {
        let r = self.file_set_comment_e(idx, comment, flags);
        self.set_err(r).map_or(-1, |_| 0)
    }

    fn file_set_comment_e(&mut self, idx: u64, comment: &[u8], flags: u32) -> Zr<()> {
        self.get_dirent(idx, 0)?;
        if self.is_rdonly() {
            return Err(ZipError::new(ER_RDONLY, 0));
        }
        if self.want_torrentzip() {
            return Err(ZipError::new(ER_NOT_ALLOWED, 0));
        }
        let cstr = if !comment.is_empty() {
            let s = ZString::new(comment, flags)?;
            if let Some(s) = &s {
                if flags & FL_ENCODING_ALL == FL_ENC_GUESS
                    && s.guess(Enc::Unknown) == Enc::Utf8Guessed
                {
                    s.set_enc(Enc::Utf8Known);
                }
            }
            s
        } else {
            None
        };
        let e = &mut self.entries[idx as usize];
        if let Some(c) = &mut e.changes {
            c.comment = None;
            c.changed &= !DIRENT_COMMENT;
        }
        let changed = match e.orig.as_ref().and_then(|o| o.comment.as_ref()) {
            Some(oc) => !string_equal(Some(oc), cstr.as_ref()),
            None => cstr.is_some(),
        };
        if changed {
            if e.changes.is_none() {
                e.changes = Some(Dirent::clone_of(e.orig.as_ref()));
            }
            let c = e.changes.as_mut().unwrap();
            c.comment = cstr;
            c.changed |= DIRENT_COMMENT;
        } else if e.changes.as_ref().is_some_and(|c| c.changed == 0) {
            e.changes = None;
        }
        Ok(())
    }

    /// zip_file_get_comment()
    pub fn file_get_comment(&mut self, idx: u64, flags: u32) -> Option<Vec<u8>> {
        let r = self
            .get_dirent(idx, flags)
            .map(|de| string_get(de.comment.as_ref(), flags));
        self.set_err(r)
    }

    /// zip_set_archive_comment()
    pub fn set_archive_comment(&mut self, comment: &[u8]) -> i32 {
        if self.is_rdonly() {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        if self.want_torrentzip() {
            self.error.set(ER_NOT_ALLOWED, 0);
            return -1;
        }
        let cstr = if !comment.is_empty() {
            match ZString::new(comment, FL_ENC_GUESS) {
                Ok(s) => {
                    if s.as_ref()
                        .is_some_and(|s| s.guess(Enc::Unknown) == Enc::Cp437)
                    {
                        self.error.set(ER_INVAL, 0);
                        return -1;
                    }
                    s
                }
                Err(e) => {
                    self.error = e;
                    return -1;
                }
            }
        } else {
            None
        };
        self.comment_changes = None;
        if string_equal(self.comment_orig.as_ref(), cstr.as_ref()) {
            self.comment_changed = false;
        } else {
            self.comment_changes = cstr;
            self.comment_changed = true;
        }
        0
    }

    /// zip_get_archive_comment()
    pub fn get_archive_comment(&self, flags: u32) -> Vec<u8> {
        let c = if flags & FL_UNCHANGED != 0 || self.comment_changes.is_none() {
            self.comment_orig.as_ref()
        } else {
            self.comment_changes.as_ref()
        };
        string_get(c, flags)
    }

    /// zip_set_archive_flag()
    pub fn set_archive_flag(&mut self, flag: u32, value: bool) -> i32 {
        if flag == AFL_IS_TORRENTZIP {
            self.error.set(ER_INVAL, 0);
            return -1;
        }
        let new = if value {
            self.ch_flags | flag
        } else {
            self.ch_flags & !flag
        };
        if new == self.ch_flags {
            return 0;
        }
        if self.flags & AFL_RDONLY != 0 {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        if flag & AFL_RDONLY != 0 && value && self.ch_flags & AFL_RDONLY == 0 && self.changed().0 {
            self.error.set(ER_CHANGED, 0);
            return -1;
        }
        self.ch_flags = new;
        0
    }

    pub fn get_archive_flag(&self, flag: u32, flags: u32) -> i64 {
        let fl = if flags & FL_UNCHANGED != 0 {
            self.flags
        } else {
            self.ch_flags
        };
        (fl & flag != 0) as i64
    }

    /// zip_set_file_compression()
    pub fn set_file_compression(&mut self, idx: u64, method: i32, flags: u32) -> i32 {
        if idx as usize >= self.entries.len() {
            self.error.set(ER_INVAL, 0);
            return -1;
        }
        if self.is_rdonly() {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        if self.want_torrentzip() {
            self.error.set(ER_NOT_ALLOWED, 0);
            return -1;
        }
        if method != CM_STORE as i32 && !codec::compression_supported(method, true) {
            self.error.set(ER_COMPNOTSUPP, 0);
            return -1;
        }
        let e = &mut self.entries[idx as usize];
        if e.changes.is_none() {
            e.changes = Some(Dirent::clone_of(e.orig.as_ref()));
        }
        let c = e.changes.as_mut().unwrap();
        c.comp_method = method;
        c.compression_level = flags & 0xffff;
        c.changed |= DIRENT_COMP_METHOD;
        0
    }

    /// zip_file_set_encryption()
    pub fn file_set_encryption(&mut self, idx: u64, method: u16, password: Option<&[u8]>) -> i32 {
        if idx as usize >= self.entries.len() {
            self.error.set(ER_INVAL, 0);
            return -1;
        }
        if self.is_rdonly() {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        if self.want_torrentzip() {
            self.error.set(ER_NOT_ALLOWED, 0);
            return -1;
        }
        if method != EM_NONE && !codec::encryption_supported(method, true) {
            self.error.set(ER_ENCRNOTSUPP, 0);
            return -1;
        }
        let e = &mut self.entries[idx as usize];
        if e.changes.is_none() {
            e.changes = Some(Dirent::clone_of(e.orig.as_ref()));
        }
        let orig_pw = e.orig.as_ref().and_then(|o| o.password.clone());
        let c = e.changes.as_mut().unwrap();
        c.encryption_method = method;
        c.changed |= DIRENT_ENCRYPTION_METHOD;
        match password {
            Some(p) => {
                c.password = Some(c_str(p).to_vec());
                c.changed |= DIRENT_PASSWORD;
            }
            None => {
                if c.changed & DIRENT_PASSWORD != 0 {
                    c.password = orig_pw;
                    c.changed &= !DIRENT_PASSWORD;
                }
            }
        }
        0
    }

    /// zip_file_set_mtime()
    pub fn file_set_mtime(&mut self, idx: u64, mtime: i64) -> i32 {
        let d = match u2d_time(mtime) {
            Ok(d) => d,
            Err(e) => {
                self.error = e;
                return -1;
            }
        };
        if let Err(e) = self.get_dirent(idx, 0) {
            self.error = e;
            return -1;
        }
        if self.is_rdonly() {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        if self.want_torrentzip() {
            self.error.set(ER_NOT_ALLOWED, 0);
            return -1;
        }
        let e = &mut self.entries[idx as usize];
        if e.orig
            .as_ref()
            .is_some_and(|o| o.encryption_method == EM_TRAD_PKWARE)
            && !e.changed(DIRENT_ENCRYPTION_METHOD)
            && !e.data_changed()
        {
            self.error.set(ER_OPNOTSUPP, 0);
            return -1;
        }
        if e.changes.is_none() {
            e.changes = Some(Dirent::clone_of(e.orig.as_ref()));
        }
        let c = e.changes.as_mut().unwrap();
        c.last_mod = d;
        c.last_mod_mtime = Some(mtime);
        c.changed |= DIRENT_LAST_MOD;
        0
    }

    /// zip_file_set_external_attributes()
    pub fn file_set_external_attributes(
        &mut self,
        idx: u64,
        _flags: u32,
        opsys: u8,
        attributes: u32,
    ) -> i32 {
        if let Err(e) = self.get_dirent(idx, 0) {
            self.error = e;
            return -1;
        }
        if self.is_rdonly() {
            self.error.set(ER_RDONLY, 0);
            return -1;
        }
        if self.want_torrentzip() {
            self.error.set(ER_NOT_ALLOWED, 0);
            return -1;
        }
        let e = &mut self.entries[idx as usize];
        let uo = e
            .orig
            .as_ref()
            .map_or(OPSYS_DEFAULT, |o| (o.version_madeby >> 8) as u8);
        let ua = e.orig.as_ref().map_or(EXT_ATTRIB_DEFAULT, |o| o.ext_attrib);
        let changed = opsys != uo || attributes != ua;
        if changed {
            if e.changes.is_none() {
                e.changes = Some(Dirent::clone_of(e.orig.as_ref()));
            }
            let c = e.changes.as_mut().unwrap();
            c.version_madeby = ((opsys as u16) << 8) | (c.version_madeby & 0xff);
            c.ext_attrib = attributes;
            c.changed |= DIRENT_ATTRIBUTES;
        } else if let Some(c) = &mut e.changes {
            c.changed &= !DIRENT_ATTRIBUTES;
            if c.changed == 0 {
                e.changes = None;
            } else {
                c.version_madeby = ((uo as u16) << 8) | (c.version_madeby & 0xff);
                c.ext_attrib = ua;
            }
        }
        0
    }

    /// zip_file_get_external_attributes()
    pub fn file_get_external_attributes(&mut self, idx: u64, flags: u32) -> Option<(u8, u32)> {
        let r = self
            .get_dirent(idx, flags)
            .map(|de| ((de.version_madeby >> 8) as u8, de.ext_attrib));
        self.set_err(r)
    }

    /// zip_set_default_password()
    pub fn set_default_password(&mut self, pw: Option<&[u8]>) {
        self.default_password = match pw.map(c_str) {
            Some(p) if !p.is_empty() => Some(p.to_vec()),
            _ => None,
        };
    }

    /// _zip_changed(): (changed, survivors)
    pub fn changed(&self) -> (bool, usize) {
        let mut changed = self.comment_changed || (self.want_torrentzip() && !self.is_torrentzip());
        let mut survivors = 0;
        for e in &self.entries {
            if e.has_changes() {
                changed = true;
            }
            if !e.deleted {
                survivors += 1;
            }
        }
        (changed, survivors)
    }

    // -- reading entries -------------------------------------------------------------

    /// _zip_file_get_offset(): data offset of an original entry
    fn file_get_offset(&self, idx: u64) -> Zr<u64> {
        let Some(o) = self.entries[idx as usize].orig.as_ref() else {
            return Err(ZipError::new(ER_INTERNAL, 0));
        };
        let off = o.offset as usize;
        let src = self.src();
        if src.len() < off + 30 {
            return Err(ZipError::new(ER_READ, 0));
        }
        let fl = u16::from_le_bytes([src[off + 26], src[off + 27]]) as u64;
        let el = u16::from_le_bytes([src[off + 28], src[off + 29]]) as u64;
        Ok(o.offset + 30 + fl + el)
    }

    /// zip_fopen_index_encrypted()
    pub fn fopen_index(
        &mut self,
        idx: u64,
        flags: u32,
        password: Option<&[u8]>,
    ) -> Option<ZipFile> {
        let pw = password.filter(|p| !p.is_empty());
        let r = self.source_zip_file(idx, flags, pw);
        self.set_err(r)
    }

    /// zip_fopen()
    pub fn fopen(&mut self, fname: &[u8], flags: u32) -> Option<ZipFile> {
        let idx = self.name_locate(fname, flags);
        if idx < 0 {
            return None;
        }
        let pw = self.default_password.clone();
        self.fopen_index(idx as u64, flags, pw.as_deref())
    }

    /// zip_source_zip_file_create() + zip_source_open()
    fn source_zip_file(&mut self, idx: u64, flags: u32, password: Option<&[u8]>) -> Zr<ZipFile> {
        if idx as usize >= self.entries.len() {
            return Err(ZipError::new(ER_INVAL, 0));
        }
        let mut flags = flags;
        if flags & FL_ENCRYPTED != 0 {
            flags |= FL_COMPRESSED;
        }
        let mut changed_data = false;
        if flags & FL_UNCHANGED == 0 {
            let e = &self.entries[idx as usize];
            if e.data_changed() {
                if flags & FL_COMPRESSED != 0 {
                    return Err(ZipError::new(ER_CHANGED, 0));
                }
                changed_data = true;
            } else if e.deleted {
                return Err(ZipError::new(ER_CHANGED, 0));
            }
        }
        let mut stat_flags = flags;
        if !changed_data {
            stat_flags |= FL_UNCHANGED;
        }
        let st = self
            .stat_index_e(idx, stat_flags)
            .map_err(|_| ZipError::new(ER_INTERNAL, 0))?;
        if changed_data {
            let src = self.entries[idx as usize].source.clone().unwrap();
            let data = src.read_all()?;
            return Ok(ZipFile {
                data,
                pos: 0,
                end_error: None,
                error: ZipError::default(),
                eof: false,
                seekable: true,
                alive: self.alive.clone(),
            });
        }
        let encrypted = st.valid & STAT_ENCRYPTION_METHOD != 0 && st.encryption_method != EM_NONE;
        let needs_decrypt = flags & FL_ENCRYPTED == 0 && encrypted;
        let compressed = st.valid & STAT_COMP_METHOD != 0 && st.comp_method != CM_STORE;
        let needs_decompress = flags & FL_COMPRESSED == 0 && compressed;
        let needs_crc = (flags & FL_COMPRESSED == 0 || !compressed) && st.valid & STAT_CRC != 0;
        let mut password = password.map(|p| p.to_vec());
        if needs_decrypt {
            if password.is_none() {
                password = self.default_password.clone();
            }
            if password.is_none() {
                return Err(ZipError::new(ER_NOPASSWD, 0));
            }
        }
        let (de_lastmod, de_bitflags) = {
            let de = self.get_dirent(idx, flags)?;
            (de.last_mod, de.bitflags)
        };
        let off = self.file_get_offset(idx)? as usize;
        let src = self.src();
        let end = off.saturating_add(st.comp_size as usize);
        let raw: Vec<u8> = if end > src.len() {
            // reading past the end of the archive
            let avail = src.get(off..).unwrap_or_default().to_vec();
            let _ = avail;
            return Ok(ZipFile {
                data: Vec::new(),
                pos: 0,
                end_error: Some(ZipError::new(ER_EOF, 0)),
                error: ZipError::default(),
                eof: false,
                seekable: false,
                alive: self.alive.clone(),
            });
        } else {
            src[off..end].to_vec()
        };
        let mut data = raw;
        let mut end_error: Option<ZipError> = None;
        if needs_decrypt {
            let pw = password.unwrap();
            match st.encryption_method {
                EM_TRAD_PKWARE => {
                    let check = if st.valid & STAT_CRC != 0 {
                        Some(((de_lastmod.time >> 8) as u8, (st.crc >> 24) as u8))
                    } else {
                        None
                    };
                    data = codec::pkware_decrypt(&data, &pw, check)?;
                }
                EM_AES_128 | EM_AES_192 | EM_AES_256 => {
                    let (d, e) = codec::aes_decrypt(&data, &pw, st.encryption_method)?;
                    data = d;
                    end_error = e;
                }
                _ => return Err(ZipError::new(ER_ENCRNOTSUPP, 0)),
            }
        }
        if needs_decompress {
            if !codec::compression_supported(st.comp_method as i32, false) {
                return Err(ZipError::new(ER_COMPNOTSUPP, 0));
            }
            // zip_algorithm_xz.c start(): the real size when general
            // purpose bits 1 and 2 are clear (no LZMA end-of-stream marker)
            let size = (de_bitflags & 0x6 == 0 && st.valid & STAT_SIZE != 0).then_some(st.size);
            let (d, e) = codec::decompress(&data, st.comp_method, size);
            data = d;
            if end_error.is_none() {
                end_error = e;
            }
        }
        if needs_crc && end_error.is_none() {
            if goro_zlib::crc32(0, &data) != st.crc {
                end_error = Some(ZipError::new(ER_CRC, 0));
            } else if st.size != data.len() as u64 {
                end_error = Some(ZipError::new(
                    ER_INCONS,
                    detail_with_index(DETAIL_INVALID_FILE_LENGTH, MAX_DETAIL_INDEX as u64),
                ));
            }
        }
        let seekable = !needs_decrypt && !needs_decompress;
        Ok(ZipFile {
            data,
            pos: 0,
            end_error,
            error: ZipError::default(),
            eof: false,
            seekable,
            alive: self.alive.clone(),
        })
    }

    // -- writing ------------------------------------------------------------------------

    /// zip_discard(): nothing to release here
    pub fn discard(self) {}

    /// zip_close(): Ok on success; on failure the archive is kept with its
    /// error set (the caller discards it).
    pub fn close(&mut self, hooks: Option<&mut dyn ProgressHooks>) -> Result<(), ()> {
        let (changed, survivors) = self.changed();
        if survivors == 0 && self.ch_flags & AFL_CREATE_OR_KEEP_FILE_FOR_EMPTY_ARCHIVE == 0 {
            if self.open_flags & ZIP_TRUNCATE != 0 || changed {
                if let Err(e) = std::fs::remove_file(&self.fname) {
                    if e.raw_os_error() != Some(ENOENT) {
                        self.error = ZipError::from_io(ER_REMOVE, &e);
                        return Err(());
                    }
                }
            }
            return Ok(());
        }
        if !changed && survivors > 0 {
            return Ok(());
        }
        let mut filelist: Vec<(usize, Option<Vec<u8>>)> = Vec::new();
        for i in 0..self.entries.len() {
            if self.entries[i].deleted {
                continue;
            }
            let n = self.get_name_e(i as u64, 0).ok();
            filelist.push((i, n));
        }
        if self.want_torrentzip() {
            filelist.sort_by(|a, b| match (&a.1, &b.1) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, _) => std::cmp::Ordering::Less,
                (_, None) => std::cmp::Ordering::Greater,
                (Some(x), Some(y)) => {
                    let lx: Vec<u8> = c_str(x).to_ascii_lowercase();
                    let ly: Vec<u8> = c_str(y).to_ascii_lowercase();
                    lx.cmp(&ly)
                }
            });
        }
        // zip_source_begin_write(): the temporary file
        let tmp = match self.begin_write() {
            Ok(t) => t,
            Err(e) => {
                self.error = e;
                return Err(());
            }
        };
        let mut dummy = NoHooks;
        let hooks: &mut dyn ProgressHooks = match hooks {
            Some(h) => h,
            None => &mut dummy,
        };
        let mut progress = Progress {
            hooks,
            last_update: 0.0,
            start: 0.0,
            end: 0.0,
        };
        let mut out = Wr(Vec::new());
        let r = self.write_all(&mut out, &filelist, &mut progress);
        match r {
            Ok(()) => {}
            Err(e) => {
                self.error = e;
                let _ = std::fs::remove_file(&tmp.0);
                return Err(());
            }
        }
        // zip_source_commit_write()
        use std::io::Write;
        let mut f = tmp.1;
        if let Err(e) = f.write_all(&out.0) {
            self.error = ZipError::from_io(ER_WRITE, &e);
            let _ = std::fs::remove_file(&tmp.0);
            return Err(());
        }
        drop(f);
        if let Err(e) = std::fs::rename(&tmp.0, &self.fname) {
            self.error = ZipError::from_io(ER_RENAME, &e);
            let _ = std::fs::remove_file(&tmp.0);
            return Err(());
        }
        progress.update(1.0);
        Ok(())
    }

    /// create_temp_file()
    fn begin_write(&self) -> Zr<(String, std::fs::File)> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(&self.fname)
            .ok()
            .map(|m| m.permissions().mode());
        for _ in 0..100 {
            let mut buf = [0u8; 4];
            {
                use purecrypto::rng::RngCore;
                purecrypto::rng::OsRng.fill_bytes(&mut buf);
            }
            let mut value = u32::from_le_bytes(buf);
            let mut x = String::new();
            for _ in 0..6 {
                let d = (value % 36) as u8;
                x.push(if d < 10 {
                    (b'0' + d) as char
                } else {
                    (b'a' + d - 10) as char
                });
                value /= 36;
            }
            let name = format!("{}.{}.part", self.fname, x);
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(mode.unwrap_or(0o666))
                .open(&name)
            {
                Ok(f) => {
                    if let Some(m) = mode {
                        let _ = f.set_permissions(std::fs::Permissions::from_mode(m));
                    }
                    return Ok((name, f));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(ZipError::from_io(ER_TMPOPEN, &e)),
            }
        }
        Err(ZipError::new(ER_TMPOPEN, 17))
    }

    fn write_all(
        &mut self,
        out: &mut Wr,
        filelist: &[(usize, Option<Vec<u8>>)],
        progress: &mut Progress,
    ) -> Zr<()> {
        if !progress.start() {
            return Err(ZipError::new(ER_CANCELLED, 0));
        }
        let n = filelist.len();
        let want_tz = self.want_torrentzip();
        let is_tz = self.is_torrentzip();
        for (j, (i, _)) in filelist.iter().enumerate() {
            if !progress.subrange(j as f64 / n as f64, (j + 1) as f64 / n as f64) {
                return Err(ZipError::new(ER_CANCELLED, 0));
            }
            let i = *i;
            let entry = &self.entries[i];
            let new_data = entry.data_changed()
                || entry.changed(DIRENT_COMP_METHOD)
                || entry.changed(DIRENT_ENCRYPTION_METHOD)
                || (want_tz && !is_tz);
            let data_changed = entry.data_changed();
            {
                let e = &mut self.entries[i];
                let orig = e.orig.clone();
                match &mut e.changes {
                    None => e.changes = Some(Dirent::clone_of(orig.as_ref())),
                    Some(c) => {
                        if let Some(o) = orig {
                            c.merge(&o, data_changed);
                        }
                    }
                }
            }
            self.read_local_ef(i as u64)?;
            if want_tz {
                self.entries[i]
                    .changes
                    .as_mut()
                    .unwrap()
                    .torrentzip_normalize();
            }
            let offset = out.0.len() as u64;
            self.entries[i].changes.as_mut().unwrap().offset = offset;
            if new_data {
                let (src_data, st, attrs, src_dos) = if !data_changed {
                    // zip_source_zip_file_create(ZIP_FL_UNCHANGED)
                    let mut zf = self.source_zip_file(i as u64, FL_UNCHANGED, None)?;
                    let data = std::mem::take(&mut zf.data);
                    if let Some(e) = zf.end_error {
                        return Err(e);
                    }
                    let mut o = self.entries[i].orig.clone().unwrap();
                    let st = Stat {
                        size: data.len() as u64,
                        mtime: o.mtime(),
                        valid: STAT_SIZE | STAT_MTIME | STAT_COMP_METHOD | STAT_CRC,
                        crc: o.crc,
                        ..Default::default()
                    };
                    let attrs = Attributes {
                        ascii: Some(o.int_attrib & 1 != 0),
                        host_system: Some((o.version_madeby >> 8) as u8),
                        external_file_attributes: Some(o.ext_attrib),
                        gpbf: Some((o.bitflags, GPBF_ALLOWED_MASK)),
                        ..Default::default()
                    };
                    (data, st, attrs, Some(o.last_mod))
                } else {
                    let src = self.entries[i].source.clone().unwrap();
                    let st = src.stat();
                    let data = src.read_all()?;
                    (data, st, src.attributes(), None)
                };
                self.add_data(out, i, src_data, st, attrs, src_dos, progress)?;
            } else {
                let de = self.entries[i].changes.as_mut().unwrap();
                if de.encryption_method != EM_TRAD_PKWARE {
                    de.bitflags &= !GPBF_DATA_DESCRIPTOR;
                }
                let mut de2 = de.clone();
                dirent_write(out, &mut de2, FL_LOCAL, want_tz)?;
                *self.entries[i].changes.as_mut().unwrap() = de2;
                let doff = self.file_get_offset(i as u64)? as usize;
                let comp_size = self.entries[i].changes.as_ref().unwrap().comp_size as usize;
                let src = self.src();
                if src.len() < doff + comp_size {
                    return Err(ZipError::new(ER_EOF, 0));
                }
                // copy_data()
                let total = comp_size as f64;
                let mut left = comp_size;
                let mut pos = doff;
                let data = self.src_data.clone().unwrap();
                while left > 0 {
                    let k = left.min(BUFSIZE);
                    out.put(&data[pos..pos + k]);
                    pos += k;
                    left -= k;
                    if !progress.update((total - left as f64) / total) {
                        return Err(ZipError::new(ER_CANCELLED, 0));
                    }
                }
                let de = self.entries[i].changes.as_ref().unwrap();
                if de.bitflags & GPBF_DATA_DESCRIPTOR != 0 {
                    write_data_descriptor(out, de, de.needs_zip64(0));
                }
            }
        }
        self.cdir_write(out, filelist)
    }

    /// add_data()
    #[allow(clippy::too_many_arguments)]
    fn add_data(
        &mut self,
        out: &mut Wr,
        i: usize,
        data: Vec<u8>,
        mut st: Stat,
        attrs: Attributes,
        src_dos: Option<DosTime>,
        progress: &mut Progress,
    ) -> Zr<()> {
        let want_tz = self.want_torrentzip();
        let default_pw = self.default_password.clone();
        let de = self.entries[i].changes.as_mut().unwrap();
        de.bitflags &= !GPBF_DATA_DESCRIPTOR;
        if st.valid & STAT_COMP_METHOD == 0 {
            st.valid |= STAT_COMP_METHOD;
            st.comp_method = CM_STORE;
        }
        // all our sources provide stored data
        st.comp_method = CM_STORE;
        if ZipInt::cm_is_default(de.comp_method) && st.comp_method != CM_STORE {
            de.comp_method = st.comp_method as i32;
        }
        let mut flags = FL_LOCAL;
        if st.valid & STAT_SIZE == 0 {
            flags |= FL_FORCE_ZIP64;
        } else {
            de.uncomp_size = st.size;
            let cm = ZipInt::cm_actual(de.comp_method);
            let max = if cm == CM_STORE {
                st.size
            } else if cm == CM_DEFLATE {
                st.size + st.size.div_ceil(16384) * 5 + 6
            } else if cm == CM_BZIP2 {
                (st.size as f64 * 1.006) as u64
            } else if cm == CM_LZMA || cm == CM_XZ {
                // zip_algorithm_xz.c
                let m = (st.size as f64 * 1.1) as u64 + 64 * 1024 + 13;
                if m < st.size { u64::MAX } else { m }
            } else if cm == CM_ZSTD {
                // ZSTD_compressBound()
                let s = st.size;
                s + (s >> 8)
                    + if s < (128 << 10) {
                        ((128 << 10) - s) >> 11
                    } else {
                        0
                    }
            } else {
                u64::MAX
            };
            if max > 0xffff_ffff {
                flags |= FL_FORCE_ZIP64;
            }
        }
        let mut have_dos_time = false;
        let mut mtime_before = 0i64;
        if de.changed & DIRENT_LAST_MOD == 0 {
            if let Some(d) = src_dos {
                de.last_mod = d;
                have_dos_time = true;
            } else {
                mtime_before = if st.valid & STAT_MTIME != 0 {
                    st.mtime
                } else {
                    now()
                };
                de.last_mod = u2d_time(mtime_before)?;
            }
        }
        let offstart = out.0.len();
        let needs_recompress = want_tz || st.comp_method != ZipInt::cm_actual(de.comp_method);
        let needs_compress = needs_recompress && de.comp_method != CM_STORE as i32;
        let st_enc = EM_NONE;
        let needs_reencrypt =
            needs_recompress || de.changed & DIRENT_PASSWORD != 0 || de.encryption_method != st_enc;
        let needs_encrypt = needs_reencrypt && de.encryption_method != EM_NONE;
        let crc = goro_zlib::crc32(0, &data);
        let uncomp_size = data.len() as u64;
        let mut comp_method = CM_STORE;
        let mut payload = data;
        let mut gp: Option<(u16, u16)> = None;
        let mut vn: Option<u16> = None;
        if needs_compress {
            let level = de.compression_level;
            let c = codec::compress(&payload, de.comp_method, level)?;
            comp_method = c.method;
            gp = Some((c.gpbf, GPBF_ALLOWED_MASK));
            vn = Some(c.version_needed);
            payload = c.data;
        }
        if needs_encrypt {
            let password = de.password.clone().or(default_pw);
            let Some(pw) = password else {
                return Err(ZipError::new(ER_INVAL, 0));
            };
            match de.encryption_method {
                EM_TRAD_PKWARE => {
                    de.bitflags |= GPBF_DATA_DESCRIPTOR;
                    let dos = if de.changed & DIRENT_LAST_MOD != 0 {
                        de.last_mod
                    } else {
                        src_dos.unwrap_or(u2d_time(if st.valid & STAT_MTIME != 0 {
                            st.mtime
                        } else {
                            now()
                        })?)
                    };
                    payload = codec::pkware_encrypt(&payload, &pw, dos.time);
                    let (f, m) = gp.unwrap_or((0, 0));
                    gp = Some((f | GPBF_DATA_DESCRIPTOR, m | GPBF_DATA_DESCRIPTOR));
                    vn = Some(vn.unwrap_or(0).max(20));
                }
                EM_AES_128 | EM_AES_192 | EM_AES_256 => {
                    payload = codec::aes_encrypt(&payload, &pw, de.encryption_method)?;
                    vn = Some(vn.unwrap_or(0).max(51));
                }
                _ => return Err(ZipError::new(ER_ENCRNOTSUPP, 0)),
            }
        }
        let mut attributes = attrs;
        if let Some(g) = gp {
            attributes.gpbf = Some(match attributes.gpbf {
                Some((f, m)) => ((f & !g.1) | (g.0 & g.1), m | g.1),
                None => g,
            });
        }
        if let Some(v) = vn {
            attributes.version_needed = Some(attributes.version_needed.map_or(v, |o| o.max(v)));
        }
        // the preliminary header: comp method / sizes are not final yet
        if !want_tz {
            de.apply_attributes(&attributes, flags & FL_FORCE_ZIP64 != 0);
        }
        let mut pre = de.clone();
        let is_zip64 = dirent_write(out, &mut pre, flags, want_tz)?;
        *de = pre;
        let offdata = out.0.len();
        // copy_source(): progress per BUFSIZE chunk of the source
        let data_length = uncomp_size;
        let mut written = 0usize;
        while written < payload.len() {
            let k = (payload.len() - written).min(BUFSIZE);
            out.put(&payload[written..written + k]);
            written += k;
            if k == BUFSIZE && data_length > 0 {
                let cur = ((written as f64 / payload.len() as f64) * data_length as f64) as i64;
                if !progress.update(cur as f64 / data_length as f64) {
                    return Err(ZipError::new(ER_CANCELLED, 0));
                }
            }
        }
        let offend = out.0.len();
        let mut dirent_changed = ZipInt::cm_actual(de.comp_method) != comp_method
            || de.crc != crc
            || de.uncomp_size != uncomp_size
            || de.comp_size != (offend - offdata) as u64;
        de.comp_method = comp_method as i32;
        de.crc = crc;
        de.uncomp_size = uncomp_size;
        de.comp_size = (offend - offdata) as u64;
        if !want_tz {
            dirent_changed |= de.apply_attributes(&attributes, flags & FL_FORCE_ZIP64 != 0);
            if de.changed & DIRENT_LAST_MOD == 0
                && !have_dos_time
                && st.valid & STAT_MTIME != 0
                && st.mtime != mtime_before
            {
                de.last_mod = u2d_time(st.mtime)?;
                dirent_changed = true;
            }
        }
        if dirent_changed {
            let mut w = Wr(Vec::new());
            let mut d2 = de.clone();
            let z = dirent_write(&mut w, &mut d2, flags, want_tz)?;
            if z != is_zip64 {
                return Err(ZipError::new(ER_INTERNAL, 0));
            }
            *de = d2;
            let n = w.0.len().min(offdata - offstart);
            out.0[offstart..offstart + n].copy_from_slice(&w.0[..n]);
        }
        if de.bitflags & GPBF_DATA_DESCRIPTOR != 0 {
            let d = de.clone();
            write_data_descriptor(out, &d, is_zip64);
        }
        Ok(())
    }

    /// _zip_cdir_write()
    fn cdir_write(&mut self, out: &mut Wr, filelist: &[(usize, Option<Vec<u8>>)]) -> Zr<()> {
        let want_tz = self.want_torrentzip();
        let offset = out.0.len() as u64;
        let mut cd = Wr(Vec::new());
        for (i, _) in filelist {
            let e = &mut self.entries[*i];
            let de = e.changes.as_mut().or(e.orig.as_mut()).unwrap();
            dirent_write(&mut cd, de, FL_CENTRAL, want_tz)?;
        }
        let cdir_crc = goro_zlib::crc32(0, &cd.0);
        out.put(&cd.0);
        let size = out.0.len() as u64 - offset;
        let survivors = filelist.len() as u64;
        if survivors > 0xffff || offset > u32::MAX as u64 || size > u32::MAX as u64 {
            out.put(EOCD64_MAGIC);
            out.p64((EOCD64LEN - 12) as u64);
            out.p16(45);
            out.p16(45);
            out.p32(0);
            out.p32(0);
            out.p64(survivors);
            out.p64(survivors);
            out.p64(size);
            out.p64(offset);
            out.put(EOCD64LOC_MAGIC);
            out.p32(0);
            out.p64(offset + size);
            out.p32(1);
        }
        out.put(EOCD_MAGIC);
        out.p32(0);
        let s16 = survivors.min(0xffff) as u16;
        out.p16(s16);
        out.p16(s16);
        out.p32(size.min(u32::MAX as u64) as u32);
        out.p32(offset.min(u32::MAX as u64) as u32);
        let comment = if self.comment_changed {
            self.comment_changes.clone()
        } else {
            self.comment_orig.clone()
        };
        if want_tz {
            out.p16((TORRENTZIP_SIGNATURE.len() + 8) as u16);
            out.put(TORRENTZIP_SIGNATURE);
            out.put(format!("{:08X}", cdir_crc).as_bytes());
        } else {
            out.p16(string_len(comment.as_ref()) as u16);
            if let Some(c) = comment {
                out.put(&c.raw);
            }
        }
        Ok(())
    }
}

struct NoHooks;

impl ProgressHooks for NoHooks {
    fn progress(&mut self, _state: f64) {}
    fn cancel(&mut self) -> i64 {
        0
    }
    fn has_progress(&self) -> bool {
        false
    }
    fn has_cancel(&self) -> bool {
        false
    }
    fn precision(&self) -> f64 {
        0.0
    }
}

/// write_data_descriptor()
fn write_data_descriptor(out: &mut Wr, de: &Dirent, is_zip64: bool) {
    out.put(DATADES_MAGIC);
    out.p32(de.crc);
    if is_zip64 {
        out.p64(de.comp_size);
        out.p64(de.uncomp_size);
    } else {
        out.p32(de.comp_size as u32);
        out.p32(de.uncomp_size as u32);
    }
}

/// _zip_headercomp(): true when inconsistent
fn headercomp(central: &Dirent, local: &Dirent) -> bool {
    if central.version_needed < local.version_needed
        || central.comp_method != local.comp_method
        || central.last_mod != local.last_mod
        || !string_equal(central.filename.as_ref(), local.filename.as_ref())
    {
        return true;
    }
    if central.crc != local.crc
        || central.comp_size != local.comp_size
        || central.uncomp_size != local.uncomp_size
    {
        if local.bitflags & GPBF_DATA_DESCRIPTOR == 0 {
            return true;
        }
        if (local.crc != 0 && central.crc != local.crc)
            || (local.comp_size != 0 && central.comp_size != local.comp_size)
            || (local.uncomp_size != 0 && central.uncomp_size != local.uncomp_size)
        {
            return true;
        }
    }
    false
}
