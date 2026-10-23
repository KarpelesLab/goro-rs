//! Port of Oniguruma 6.9.10's encoding layer (regenc.h/regenc.c and the
//! per-encoding files used by PHP's mbregex: ascii.c, iso8859_*.c,
//! utf8.c, utf16_*.c, utf32_*.c, euc_jp.c, sjis.c, big5.c, euc_tw.c,
//! euc_kr.c (EUC-KR and EUC-CN), koi8.c, koi8_r.c).
//!
//! Strings are byte slices; a position is an index into the slice and
//! `end` is an explicit end index (C pointer arithmetic is mapped to index
//! arithmetic). Reads at or past `s.len()` behave as if a NUL byte were
//! there (PHP strings are NUL-terminated, which the C code relies on).

#![allow(dead_code)]

/* ===== constants (oniguruma.h / regenc.h) ===== */

pub const ONIGENC_CTYPE_NEWLINE: u32 = 0;
pub const ONIGENC_CTYPE_ALPHA: u32 = 1;
pub const ONIGENC_CTYPE_BLANK: u32 = 2;
pub const ONIGENC_CTYPE_CNTRL: u32 = 3;
pub const ONIGENC_CTYPE_DIGIT: u32 = 4;
pub const ONIGENC_CTYPE_GRAPH: u32 = 5;
pub const ONIGENC_CTYPE_LOWER: u32 = 6;
pub const ONIGENC_CTYPE_PRINT: u32 = 7;
pub const ONIGENC_CTYPE_PUNCT: u32 = 8;
pub const ONIGENC_CTYPE_SPACE: u32 = 9;
pub const ONIGENC_CTYPE_UPPER: u32 = 10;
pub const ONIGENC_CTYPE_XDIGIT: u32 = 11;
pub const ONIGENC_CTYPE_WORD: u32 = 12;
pub const ONIGENC_CTYPE_ALNUM: u32 = 13;
pub const ONIGENC_CTYPE_ASCII: u32 = 14;
pub const ONIGENC_MAX_STD_CTYPE: u32 = ONIGENC_CTYPE_ASCII;

pub const ONIGENC_CASE_FOLD_ASCII_ONLY: u32 = 1;
pub const ONIGENC_CASE_FOLD_TURKISH_AZERI: u32 = 1 << 20;
pub const INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR: u32 = 1 << 30;
pub const ONIGENC_CASE_FOLD_MIN: u32 = INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR;
/// OnigDefaultCaseFoldFlag (never changed by PHP)
pub const ONIGENC_CASE_FOLD_DEFAULT: u32 = ONIGENC_CASE_FOLD_MIN;

pub const ONIGENC_MAX_COMP_CASE_FOLD_CODE_LEN: usize = 3;
pub const ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM: usize = 13;
pub const ONIGENC_CODE_TO_MBC_MAXLEN: usize = 7;
pub const ONIGENC_MBC_CASE_FOLD_MAXLEN: usize = 18;

pub const ENC_FLAG_ASCII_COMPATIBLE: u32 = 1 << 0;
pub const ENC_FLAG_UNICODE: u32 = 1 << 1;
pub const ENC_FLAG_SKIP_OFFSET_MASK: u32 = 7 << 2;
pub const ENC_SKIP_OFFSET_1_OR_0: u32 = 7;

pub const MAX_CODE_POINT: u32 = !0u32;
pub const ASCII_LIMIT: u32 = 127;
pub const NEWLINE_CODE: u32 = 0x0a;

/// Returned by get_ctype_code_range() when the encoding has no range table
pub const ONIG_NO_SUPPORT_CONFIG: i32 = -2;

/// OnigCaseFoldCodeItem
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaseFoldCodeItem {
    /// argument (original) character(s) byte length
    pub byte_len: i32,
    /// number of codes
    pub code_len: i32,
    pub code: [u32; ONIGENC_MAX_COMP_CASE_FOLD_CODE_LEN],
}

/// The encodings PHP's mbregex can select (php_mbregex.c enc_name_map)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EncKind {
    Ascii,
    /// ISO-8859-n (n = 1..=16, without 12)
    Iso8859(u8),
    Utf8,
    Utf16Be,
    Utf16Le,
    Utf32Be,
    Utf32Le,
    EucJp,
    Sjis,
    Big5,
    EucTw,
    EucKr,
    EucCn,
    Koi8,
    Koi8R,
}

/// OnigEncodingType
#[derive(Debug)]
pub struct Encoding {
    pub kind: EncKind,
    pub name: &'static str,
    pub max_enc_len: usize,
    pub min_enc_len: usize,
    pub flag: u32,
    pub sb_range: u32,
}

use super::enc_tables as et;
use super::unicode_data as ud;
use std::collections::HashMap;
use std::sync::OnceLock;

const ONIGERR_TYPE_BUG: i32 = -6;
const ONIGERR_INVALID_CHAR_PROPERTY_NAME: i32 = -223;
const ONIGERR_INVALID_CODE_POINT_VALUE: i32 = -400;
const ONIGERR_TOO_BIG_WIDE_CHAR_VALUE: i32 = -401;

const ENC_FLAG_SKIP_OFFSET_1: u32 = 1 << 2;
const ENC_FLAG_SKIP_OFFSET_2: u32 = 2 << 2;
const ENC_FLAG_SKIP_OFFSET_4: u32 = 4 << 2;
const ENC_FLAG_SKIP_OFFSET_1_OR_0: u32 = ENC_SKIP_OFFSET_1_OR_0 << 2;

const LARGE_S: u8 = 0x53;
const SMALL_S: u8 = 0x73;

/// Byte at index i, 0 past the end of the slice (NUL terminator)
#[inline]
fn b(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

#[inline]
fn case_fold_is_ascii_only(flag: u32) -> bool {
    flag & ONIGENC_CASE_FOLD_ASCII_ONLY != 0
}

#[inline]
fn ctype_is_word_graph_print(ctype: u32) -> bool {
    ctype == ONIGENC_CTYPE_WORD || ctype == ONIGENC_CTYPE_GRAPH || ctype == ONIGENC_CTYPE_PRINT
}

#[inline]
fn table_ctype(table: &[u16; 256], code: u32, ctype: u32) -> bool {
    ctype < 16 && (table[code as usize] & (1u16 << ctype)) != 0
}

/// ONIGENC_IS_ASCII_CODE_CTYPE
#[inline]
pub fn is_ascii_code_ctype(code: u32, ctype: u32) -> bool {
    code < 256 && table_ctype(&et::ASCII_CTYPE, code, ctype)
}

/// ONIGENC_ASCII_CODE_TO_LOWER_CASE
#[inline]
pub fn ascii_code_to_lower_case(c: u8) -> u8 {
    et::ASCII_LOWER[c as usize]
}

/// onig_is_in_code_range: `ranges` in the C layout [n, from1, to1, ...]
pub fn is_in_code_range(ranges: &[u32], code: u32) -> bool {
    let n = ranges[0];
    let data = &ranges[1..];
    let (mut low, mut high) = (0u32, n);
    while low < high {
        let x = (low + high) >> 1;
        if code > data[(x * 2 + 1) as usize] {
            low = x + 1;
        } else {
            high = x;
        }
    }
    low < n && code >= data[(low * 2) as usize]
}

macro_rules! enc {
    ($kind:expr, $name:expr, $max:expr, $min:expr, $flag:expr) => {
        Encoding {
            kind: $kind,
            name: $name,
            max_enc_len: $max,
            min_enc_len: $min,
            flag: $flag,
            sb_range: 0,
        }
    };
}

const AC_SB: u32 = ENC_FLAG_ASCII_COMPATIBLE | ENC_FLAG_SKIP_OFFSET_1;

static ENC_ASCII: Encoding = enc!(EncKind::Ascii, "US-ASCII", 1, 1, AC_SB);
static ENC_ISO: [Encoding; 16] = [
    enc!(EncKind::Iso8859(1), "ISO-8859-1", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(2), "ISO-8859-2", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(3), "ISO-8859-3", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(4), "ISO-8859-4", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(5), "ISO-8859-5", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(6), "ISO-8859-6", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(7), "ISO-8859-7", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(8), "ISO-8859-8", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(9), "ISO-8859-9", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(10), "ISO-8859-10", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(11), "ISO-8859-11", 1, 1, AC_SB),
    // ISO-8859-12 does not exist; slot kept so that n indexes directly
    enc!(EncKind::Iso8859(1), "ISO-8859-1", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(13), "ISO-8859-13", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(14), "ISO-8859-14", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(15), "ISO-8859-15", 1, 1, AC_SB),
    enc!(EncKind::Iso8859(16), "ISO-8859-16", 1, 1, AC_SB),
];
static ENC_UTF8: Encoding = enc!(
    EncKind::Utf8,
    "UTF-8",
    4,
    1,
    ENC_FLAG_ASCII_COMPATIBLE | ENC_FLAG_UNICODE | ENC_FLAG_SKIP_OFFSET_1_OR_0
);
static ENC_UTF16BE: Encoding = enc!(
    EncKind::Utf16Be,
    "UTF-16BE",
    4,
    2,
    ENC_FLAG_UNICODE | ENC_FLAG_SKIP_OFFSET_2
);
static ENC_UTF16LE: Encoding = enc!(
    EncKind::Utf16Le,
    "UTF-16LE",
    4,
    2,
    ENC_FLAG_UNICODE | ENC_FLAG_SKIP_OFFSET_1
);
static ENC_UTF32BE: Encoding = enc!(
    EncKind::Utf32Be,
    "UTF-32BE",
    4,
    4,
    ENC_FLAG_UNICODE | ENC_FLAG_SKIP_OFFSET_4
);
static ENC_UTF32LE: Encoding = enc!(
    EncKind::Utf32Le,
    "UTF-32LE",
    4,
    4,
    ENC_FLAG_UNICODE | ENC_FLAG_SKIP_OFFSET_1
);
static ENC_EUCJP: Encoding = enc!(
    EncKind::EucJp,
    "EUC-JP",
    3,
    1,
    ENC_FLAG_ASCII_COMPATIBLE | ENC_FLAG_SKIP_OFFSET_1_OR_0
);
static ENC_SJIS: Encoding = enc!(
    EncKind::Sjis,
    "Shift_JIS",
    2,
    1,
    ENC_FLAG_ASCII_COMPATIBLE | ENC_FLAG_SKIP_OFFSET_1_OR_0
);
static ENC_BIG5: Encoding = enc!(EncKind::Big5, "Big5", 2, 1, AC_SB);
static ENC_EUCTW: Encoding = enc!(EncKind::EucTw, "EUC-TW", 4, 1, AC_SB);
static ENC_EUCKR: Encoding = enc!(
    EncKind::EucKr,
    "EUC-KR",
    2,
    1,
    ENC_FLAG_ASCII_COMPATIBLE | ENC_FLAG_SKIP_OFFSET_1_OR_0
);
static ENC_EUCCN: Encoding = enc!(
    EncKind::EucCn,
    "EUC-CN",
    2,
    1,
    ENC_FLAG_ASCII_COMPATIBLE | ENC_FLAG_SKIP_OFFSET_1_OR_0
);
static ENC_KOI8: Encoding = enc!(EncKind::Koi8, "KOI8", 1, 1, AC_SB);
static ENC_KOI8R: Encoding = enc!(EncKind::Koi8R, "KOI8-R", 1, 1, AC_SB);

/// How a single byte encoding implements the case fold functions
enum SbFold {
    /// onigenc_ascii_* functions (US-ASCII, ISO-8859-6/8/11)
    Ascii,
    /// iso8859_1.c's own get_case_fold_codes_by_str
    Latin1,
    /// onigenc_*_with_map
    Map,
}

impl Encoding {
    /// Look up the static encoding object for a kind
    pub fn get(kind: EncKind) -> &'static Encoding {
        match kind {
            EncKind::Ascii => &ENC_ASCII,
            EncKind::Iso8859(n) => {
                if (1..=16).contains(&n) && n != 12 {
                    &ENC_ISO[n as usize - 1]
                } else {
                    &ENC_ISO[0]
                }
            }
            EncKind::Utf8 => &ENC_UTF8,
            EncKind::Utf16Be => &ENC_UTF16BE,
            EncKind::Utf16Le => &ENC_UTF16LE,
            EncKind::Utf32Be => &ENC_UTF32BE,
            EncKind::Utf32Le => &ENC_UTF32LE,
            EncKind::EucJp => &ENC_EUCJP,
            EncKind::Sjis => &ENC_SJIS,
            EncKind::Big5 => &ENC_BIG5,
            EncKind::EucTw => &ENC_EUCTW,
            EncKind::EucKr => &ENC_EUCKR,
            EncKind::EucCn => &ENC_EUCCN,
            EncKind::Koi8 => &ENC_KOI8,
            EncKind::Koi8R => &ENC_KOI8R,
        }
    }

    /// Tables of a single byte encoding (None for US-ASCII and multibyte ones)
    fn sb_tables(&self) -> Option<(&'static et::SingleByteTables, SbFold)> {
        Some(match self.kind {
            EncKind::Iso8859(n) => match n {
                2 => (&et::ISO_8859_2, SbFold::Map),
                3 => (&et::ISO_8859_3, SbFold::Map),
                4 => (&et::ISO_8859_4, SbFold::Map),
                5 => (&et::ISO_8859_5, SbFold::Map),
                6 => (&et::ISO_8859_6, SbFold::Ascii),
                7 => (&et::ISO_8859_7, SbFold::Map),
                8 => (&et::ISO_8859_8, SbFold::Ascii),
                9 => (&et::ISO_8859_9, SbFold::Map),
                10 => (&et::ISO_8859_10, SbFold::Map),
                11 => (&et::ISO_8859_11, SbFold::Ascii),
                13 => (&et::ISO_8859_13, SbFold::Map),
                14 => (&et::ISO_8859_14, SbFold::Map),
                15 => (&et::ISO_8859_15, SbFold::Map),
                16 => (&et::ISO_8859_16, SbFold::Map),
                _ => (&et::ISO_8859_1, SbFold::Latin1),
            },
            EncKind::Koi8 => (&et::KOI8, SbFold::Map),
            EncKind::Koi8R => (&et::KOI8_R, SbFold::Map),
            _ => return None,
        })
    }

    /// ONIGENC_MBC_ENC_LEN: byte length of the char starting at s[p]
    /// (only looks at the bytes needed, like the C version; never 0)
    pub fn mbc_enc_len(&self, s: &[u8], p: usize) -> usize {
        (match self.kind {
            EncKind::Utf8 => et::ENCLEN_UTF8[b(s, p) as usize],
            EncKind::Utf16Be => et::ENCLEN_UTF16[b(s, p) as usize],
            EncKind::Utf16Le => et::ENCLEN_UTF16[b(s, p + 1) as usize],
            EncKind::Utf32Be | EncKind::Utf32Le => 4,
            EncKind::EucJp => et::ENCLEN_EUCJP[b(s, p) as usize],
            EncKind::Sjis => et::ENCLEN_SJIS[b(s, p) as usize],
            EncKind::Big5 => et::ENCLEN_BIG5[b(s, p) as usize],
            EncKind::EucTw => et::ENCLEN_EUCTW[b(s, p) as usize],
            EncKind::EucKr | EncKind::EucCn => et::ENCLEN_EUCKR[b(s, p) as usize],
            _ => 1,
        }) as usize
    }

    /// ONIGENC_IS_MBC_NEWLINE
    pub fn is_mbc_newline(&self, s: &[u8], p: usize, end: usize) -> bool {
        match self.kind {
            EncKind::Utf16Be => p + 1 < end && b(s, p + 1) == 0x0a && b(s, p) == 0,
            EncKind::Utf16Le => p + 1 < end && b(s, p) == 0x0a && b(s, p + 1) == 0,
            EncKind::Utf32Be => {
                p + 3 < end
                    && b(s, p + 3) == 0x0a
                    && b(s, p + 2) == 0
                    && b(s, p + 1) == 0
                    && b(s, p) == 0
            }
            EncKind::Utf32Le => {
                p + 3 < end
                    && b(s, p) == 0x0a
                    && b(s, p + 1) == 0
                    && b(s, p + 2) == 0
                    && b(s, p + 3) == 0
            }
            _ => p < end && b(s, p) == 0x0a,
        }
    }

    /// onigenc_mbn_mbc_to_code
    fn mbn_mbc_to_code(&self, s: &[u8], p: usize, end: usize) -> u32 {
        let len = self.mbc_enc_len(s, p);
        let mut n = b(s, p) as u32;
        if len == 1 {
            return n;
        }
        for i in 1..len {
            if p + i >= end {
                break;
            }
            n = (n << 8).wrapping_add(b(s, p + i) as u32);
        }
        n
    }

    /// ONIGENC_MBC_TO_CODE
    pub fn mbc_to_code(&self, s: &[u8], p: usize, end: usize) -> u32 {
        match self.kind {
            EncKind::Utf8 => {
                let mut len = self.mbc_enc_len(s, p) as isize;
                let avail = end as isize - p as isize;
                if len > avail {
                    len = avail;
                }
                let c = b(s, p) as u32;
                if len > 1 {
                    let mut len = len - 1;
                    let mut n = c & ((1u32 << (6 - len)) - 1);
                    let mut q = p + 1;
                    while len > 0 {
                        let c = b(s, q) as u32;
                        q += 1;
                        n = (n << 6) | (c & 0x3f);
                        len -= 1;
                    }
                    n
                } else {
                    c
                }
            }
            EncKind::Utf16Be => {
                let (p0, p1, p2, p3) = (
                    b(s, p) as i64,
                    b(s, p + 1) as i64,
                    b(s, p + 2) as i64,
                    b(s, p + 3) as i64,
                );
                if (p0 & 0xfc) == 0xd8 {
                    (((((p0 - 0xd8) << 2) + ((p1 & 0xc0) >> 6) + 1) << 16)
                        + ((((p1 & 0x3f) << 2) + (p2 - 0xdc)) << 8)
                        + p3) as u32
                } else {
                    (p0 * 256 + p1) as u32
                }
            }
            EncKind::Utf16Le => {
                let (c0, c1, p2, p3) = (
                    b(s, p) as i64,
                    b(s, p + 1) as i64,
                    b(s, p + 2) as i64,
                    b(s, p + 3) as i64,
                );
                if (c1 & 0xfc) == 0xd8 {
                    (((((c1 - 0xd8) << 2) + ((c0 & 0xc0) >> 6) + 1) << 16)
                        + ((((c0 & 0x3f) << 2) + (p3 - 0xdc)) << 8)
                        + p2) as u32
                } else {
                    (c1 * 256 + c0) as u32
                }
            }
            EncKind::Utf32Be => {
                ((((b(s, p) as u32 & 0x7f) * 256 + b(s, p + 1) as u32) * 256 + b(s, p + 2) as u32)
                    * 256)
                    + b(s, p + 3) as u32
            }
            EncKind::Utf32Le => {
                ((((b(s, p + 3) as u32 & 0x7f) * 256 + b(s, p + 2) as u32) * 256
                    + b(s, p + 1) as u32)
                    * 256)
                    + b(s, p) as u32
            }
            EncKind::EucJp
            | EncKind::Sjis
            | EncKind::Big5
            | EncKind::EucTw
            | EncKind::EucKr
            | EncKind::EucCn => self.mbn_mbc_to_code(s, p, end),
            _ => b(s, p) as u32,
        }
    }

    /// ONIGENC_CODE_TO_MBCLEN: byte length or a negative ONIGERR_* code
    pub fn code_to_mbclen(&self, code: u32) -> i32 {
        match self.kind {
            EncKind::Utf8 => {
                if code & 0xffffff80 == 0 {
                    1
                } else if code & 0xfffff800 == 0 {
                    2
                } else if code & 0xffff0000 == 0 {
                    3
                } else if code & 0xffe00000 == 0 {
                    4
                } else {
                    ONIGERR_INVALID_CODE_POINT_VALUE
                }
            }
            EncKind::Utf16Be | EncKind::Utf16Le => {
                if code > 0xffff {
                    if code > 0x10ffff {
                        ONIGERR_INVALID_CODE_POINT_VALUE
                    } else {
                        4
                    }
                } else {
                    2
                }
            }
            EncKind::Utf32Be | EncKind::Utf32Le => 4,
            EncKind::EucJp => {
                let t = &et::ENCLEN_EUCJP;
                if code < 0x80 {
                    return 1;
                } else if code & 0xff0000 != 0 {
                    if t[((code >> 16) & 0xff) as usize] == 3 {
                        return 3;
                    }
                } else if code & 0xff00 != 0 {
                    if t[((code >> 8) & 0xff) as usize] == 2 {
                        return 2;
                    }
                } else if code < 256 && t[(code & 0xff) as usize] == 1 {
                    return 1;
                }
                ONIGERR_INVALID_CODE_POINT_VALUE
            }
            EncKind::Sjis => {
                let t = &et::ENCLEN_SJIS;
                if code < 256 {
                    if t[code as usize] == 1 {
                        return 1;
                    }
                } else if code < 0x10000 && t[((code >> 8) & 0xff) as usize] == 2 {
                    return 2;
                }
                ONIGERR_INVALID_CODE_POINT_VALUE
            }
            EncKind::Big5 | EncKind::EucKr | EncKind::EucCn => {
                let t = if self.kind == EncKind::Big5 {
                    &et::ENCLEN_BIG5
                } else {
                    &et::ENCLEN_EUCKR
                };
                if code & !0xffff != 0 {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                if code & 0xff00 != 0 {
                    if t[((code >> 8) & 0xff) as usize] == 2 {
                        return 2;
                    }
                } else if t[(code & 0xff) as usize] == 1 {
                    return 1;
                }
                ONIGERR_INVALID_CODE_POINT_VALUE
            }
            EncKind::EucTw => {
                let t = &et::ENCLEN_EUCTW;
                if code & 0xff000000 != 0 {
                    if t[((code >> 24) & 0xff) as usize] == 4 {
                        return 4;
                    }
                } else if code & 0xff0000 != 0 {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                } else if code & 0xff00 != 0 {
                    if t[((code >> 8) & 0xff) as usize] == 2 {
                        return 2;
                    }
                } else if t[(code & 0xff) as usize] == 1 {
                    return 1;
                }
                ONIGERR_INVALID_CODE_POINT_VALUE
            }
            _ => {
                if code < 0x100 {
                    1
                } else {
                    ONIGERR_INVALID_CODE_POINT_VALUE
                }
            }
        }
    }

    /// ONIGENC_CODE_TO_MBC: writes the encoding of `code` into buf and
    /// returns its length (or a negative ONIGERR_* code)
    pub fn code_to_mbc(&self, code: u32, buf: &mut [u8; ONIGENC_CODE_TO_MBC_MAXLEN]) -> i32 {
        match self.kind {
            EncKind::Utf8 => {
                if code & 0xffffff80 == 0 {
                    buf[0] = code as u8;
                    return 1;
                }
                let trail = |sh: u32| (((code >> sh) & 0x3f) | 0x80) as u8;
                let n = if code & 0xfffff800 == 0 {
                    buf[0] = (((code >> 6) & 0x1f) | 0xc0) as u8;
                    1
                } else if code & 0xffff0000 == 0 {
                    buf[0] = (((code >> 12) & 0x0f) | 0xe0) as u8;
                    buf[1] = trail(6);
                    2
                } else if code & 0xffe00000 == 0 {
                    buf[0] = (((code >> 18) & 0x07) | 0xf0) as u8;
                    buf[1] = trail(12);
                    buf[2] = trail(6);
                    3
                } else {
                    return ONIGERR_TOO_BIG_WIDE_CHAR_VALUE;
                };
                buf[n] = ((code & 0x3f) | 0x80) as u8;
                (n + 1) as i32
            }
            EncKind::Utf16Be | EncKind::Utf16Le => {
                let be = self.kind == EncKind::Utf16Be;
                if code > 0xffff {
                    let plane = (code >> 16).wrapping_sub(1);
                    let high = (code & 0xff00) >> 8;
                    let a = ((plane >> 2) + 0xd8) as u8;
                    let bb = (((plane & 0x03) << 6) + (high >> 2)) as u8;
                    let c = ((high & 0x03) + 0xdc) as u8;
                    let d = (code & 0xff) as u8;
                    if be {
                        buf[..4].copy_from_slice(&[a, bb, c, d]);
                    } else {
                        buf[..4].copy_from_slice(&[bb, a, d, c]);
                    }
                    4
                } else {
                    let (h, l) = (((code & 0xff00) >> 8) as u8, (code & 0xff) as u8);
                    if be {
                        buf[0] = h;
                        buf[1] = l;
                    } else {
                        buf[0] = l;
                        buf[1] = h;
                    }
                    2
                }
            }
            EncKind::Utf32Be => {
                buf[..4].copy_from_slice(&code.to_be_bytes());
                4
            }
            EncKind::Utf32Le => {
                buf[..4].copy_from_slice(&code.to_le_bytes());
                4
            }
            EncKind::EucJp => {
                let mut n = 0;
                if code & 0xff0000 != 0 {
                    buf[0] = ((code >> 16) & 0xff) as u8;
                    buf[1] = ((code >> 8) & 0xff) as u8;
                    n = 2;
                } else if code & 0xff00 != 0 {
                    buf[0] = ((code >> 8) & 0xff) as u8;
                    n = 1;
                }
                buf[n] = (code & 0xff) as u8;
                n += 1;
                if self.mbc_enc_len(&buf[..], 0) != n {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                n as i32
            }
            EncKind::Sjis => {
                let mut n = 0;
                if code & 0xff00 != 0 {
                    buf[0] = ((code >> 8) & 0xff) as u8;
                    n = 1;
                }
                buf[n] = (code & 0xff) as u8;
                (n + 1) as i32
            }
            EncKind::Big5 | EncKind::EucKr | EncKind::EucCn => {
                // onigenc_mb2_code_to_mbc
                let mut n = 0;
                if code & 0xff00 != 0 {
                    buf[0] = ((code >> 8) & 0xff) as u8;
                    n = 1;
                }
                buf[n] = (code & 0xff) as u8;
                n += 1;
                if self.mbc_enc_len(&buf[..], 0) != n {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                n as i32
            }
            EncKind::EucTw => {
                // onigenc_mb4_code_to_mbc
                let mut n = 0;
                if code & 0xff000000 != 0 {
                    buf[n] = ((code >> 24) & 0xff) as u8;
                    n += 1;
                }
                if code & 0xff0000 != 0 || n != 0 {
                    buf[n] = ((code >> 16) & 0xff) as u8;
                    n += 1;
                }
                if code & 0xff00 != 0 || n != 0 {
                    buf[n] = ((code >> 8) & 0xff) as u8;
                    n += 1;
                }
                buf[n] = (code & 0xff) as u8;
                n += 1;
                if self.mbc_enc_len(&buf[..], 0) != n {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                n as i32
            }
            _ => {
                buf[0] = (code & 0xff) as u8;
                1
            }
        }
    }

    /// ONIGENC_MBC_CASE_FOLD: folds the char at s[*pp], advances *pp past
    /// it, writes the folded bytes into `to` and returns their length
    pub fn mbc_case_fold(
        &self,
        flag: u32,
        s: &[u8],
        pp: &mut usize,
        end: usize,
        to: &mut [u8; ONIGENC_MBC_CASE_FOLD_MAXLEN],
    ) -> usize {
        let p = *pp;
        match self.kind {
            EncKind::Utf8 => {
                if b(s, p) < 0x80 {
                    to[0] = ascii_code_to_lower_case(b(s, p));
                    *pp += 1;
                    1
                } else {
                    unicode_mbc_case_fold(self, flag, s, pp, end, to)
                }
            }
            EncKind::Utf16Be => {
                if b(s, p + 1) < 0x80 && b(s, p) == 0 {
                    to[0] = 0;
                    to[1] = ascii_code_to_lower_case(b(s, p + 1));
                    *pp += 2;
                    2
                } else {
                    unicode_mbc_case_fold(self, flag, s, pp, end, to)
                }
            }
            EncKind::Utf16Le => {
                if b(s, p) < 0x80 && b(s, p + 1) == 0 {
                    to[0] = ascii_code_to_lower_case(b(s, p));
                    to[1] = 0;
                    *pp += 2;
                    2
                } else {
                    unicode_mbc_case_fold(self, flag, s, pp, end, to)
                }
            }
            EncKind::Utf32Be => {
                if b(s, p + 3) < 0x80 && b(s, p + 2) == 0 && b(s, p + 1) == 0 && b(s, p) == 0 {
                    to[0] = 0;
                    to[1] = 0;
                    to[2] = 0;
                    to[3] = ascii_code_to_lower_case(b(s, p + 3));
                    *pp += 4;
                    4
                } else {
                    unicode_mbc_case_fold(self, flag, s, pp, end, to)
                }
            }
            EncKind::Utf32Le => {
                if b(s, p) < 0x80 && b(s, p + 1) == 0 && b(s, p + 2) == 0 && b(s, p + 3) == 0 {
                    to[0] = ascii_code_to_lower_case(b(s, p));
                    to[1] = 0;
                    to[2] = 0;
                    to[3] = 0;
                    *pp += 4;
                    4
                } else {
                    unicode_mbc_case_fold(self, flag, s, pp, end, to)
                }
            }
            EncKind::EucJp
            | EncKind::Sjis
            | EncKind::Big5
            | EncKind::EucTw
            | EncKind::EucKr
            | EncKind::EucCn => {
                // onigenc_mbn_mbc_case_fold (and the identical euc_jp/sjis ones)
                if b(s, p) < 0x80 {
                    to[0] = ascii_code_to_lower_case(b(s, p));
                    *pp += 1;
                    1
                } else {
                    let len = self.mbc_enc_len(s, p);
                    for (i, t) in to.iter_mut().enumerate().take(len) {
                        *t = b(s, p + i);
                    }
                    *pp += len;
                    len
                }
            }
            _ => {
                let c = b(s, p);
                match self.sb_tables() {
                    Some((t, _)) if t.lower.is_some() => {
                        let lower = t.lower.unwrap();
                        if t.df_ss && c == 0xdf && flag & INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR != 0
                        {
                            to[0] = b's';
                            to[1] = b's';
                            *pp += 1;
                            return 2;
                        }
                        to[0] = if !case_fold_is_ascii_only(flag) || c < 0x80 {
                            lower[c as usize]
                        } else {
                            c
                        };
                        *pp += 1;
                        1
                    }
                    _ => {
                        // onigenc_ascii_mbc_case_fold
                        to[0] = ascii_code_to_lower_case(c);
                        *pp += 1;
                        1
                    }
                }
            }
        }
    }

    /// ONIGENC_APPLY_ALL_CASE_FOLD: calls f(from, to) for every case fold
    /// pair the C function enumerates (same order). A non-zero return of f
    /// stops and is returned.
    pub fn apply_all_case_fold(&self, flag: u32, f: &mut dyn FnMut(u32, &[u32]) -> i32) -> i32 {
        if self.is_unicode() {
            return unicode_apply_all_case_fold(flag, f);
        }
        match self.sb_tables() {
            Some((t, SbFold::Map)) | Some((t, SbFold::Latin1)) => {
                apply_all_case_fold_with_map(t.map, t.ess_tsett, flag, f)
            }
            _ => ascii_apply_all_case_fold(f),
        }
    }

    /// ONIGENC_GET_CASE_FOLD_CODES_BY_STR: fills items (same order as C)
    /// and returns their number
    pub fn get_case_fold_codes_by_str(
        &self,
        flag: u32,
        s: &[u8],
        p: usize,
        end: usize,
        items: &mut [CaseFoldCodeItem; ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM],
    ) -> i32 {
        if self.is_unicode() {
            return unicode_get_case_fold_codes_by_str(self, flag, s, p, end, items);
        }
        match self.sb_tables() {
            Some((_, SbFold::Latin1)) => latin1_get_case_fold_codes_by_str(flag, s, p, end, items),
            Some((t, SbFold::Map)) => {
                get_case_fold_codes_by_str_with_map(t.map, t.ess_tsett, flag, s, p, end, items)
            }
            _ => ascii_get_case_fold_codes_by_str(s, p, items),
        }
    }

    /// ONIGENC_PROPERTY_NAME_TO_CTYPE: the name is the raw pattern bytes
    /// in this encoding. Returns the ctype or
    /// ONIGERR_INVALID_CHAR_PROPERTY_NAME / ONIGERR_TOO_LONG_PROPERTY_NAME.
    pub fn property_name_to_ctype(&self, name: &[u8]) -> i32 {
        if self.is_unicode() {
            return unicode_property_name_to_ctype(self, name);
        }
        match self.kind {
            EncKind::EucJp | EncKind::Sjis => {
                const NAMES: [(&[u8], i32); 16] = [
                    (b"Word", 12),
                    (b"Alpha", 1),
                    (b"Hiragana", 15),
                    (b"Alnum", 13),
                    (b"Katakana", 16),
                    (b"ASCII", 14),
                    (b"XDigit", 11),
                    (b"Cntrl", 3),
                    (b"Blank", 2),
                    (b"Punct", 8),
                    (b"Print", 7),
                    (b"Upper", 10),
                    (b"Space", 9),
                    (b"Lower", 6),
                    (b"Graph", 5),
                    (b"Digit", 4),
                ];
                if name.len() < 31 {
                    for (n, c) in NAMES {
                        if n == name {
                            return c;
                        }
                    }
                }
                ONIGERR_INVALID_CHAR_PROPERTY_NAME
            }
            _ => minimum_property_name_to_ctype(self, name),
        }
    }

    /// ONIGENC_IS_CODE_CTYPE
    pub fn is_code_ctype(&self, code: u32, ctype: u32) -> bool {
        if self.is_unicode() {
            return unicode_is_code_ctype(code, ctype);
        }
        match self.kind {
            EncKind::Ascii => code < 128 && is_ascii_code_ctype(code, ctype),
            EncKind::EucJp | EncKind::Sjis => {
                if ctype <= ONIGENC_MAX_STD_CTYPE {
                    if code < 128 {
                        is_ascii_code_ctype(code, ctype)
                    } else {
                        ctype_is_word_graph_print(ctype) && self.code_to_mbclen(code) > 1
                    }
                } else {
                    match self.jp_property_list(ctype - (ONIGENC_MAX_STD_CTYPE + 1)) {
                        Some(r) => is_in_code_range(r, code),
                        None => true, /* ONIGERR_TYPE_BUG (non-zero) */
                    }
                }
            }
            EncKind::Big5 | EncKind::EucTw | EncKind::EucKr | EncKind::EucCn => {
                // onigenc_mb2_is_code_ctype / onigenc_mb4_is_code_ctype
                if code < 128 {
                    is_ascii_code_ctype(code, ctype)
                } else {
                    ctype_is_word_graph_print(ctype) && self.code_to_mbclen(code) > 1
                }
            }
            _ => {
                let (t, _) = self.sb_tables().unwrap();
                code < 256 && table_ctype(t.ctype, code, ctype)
            }
        }
    }

    fn jp_property_list(&self, idx: u32) -> Option<&'static [u32]> {
        static EUCJP: [&[u32]; 2] = [
            &[1, 0xa4a1, 0xa4f3],
            &[3, 0xa5a1, 0xa5f6, 0xaaa6, 0xaaaf, 0xaab1, 0xaadd],
        ];
        static SJIS: [&[u32]; 2] = [
            &[1, 0x829f, 0x82f1],
            &[
                4, 0x00a6, 0x00af, 0x00b1, 0x00dd, 0x8340, 0x837e, 0x8380, 0x8396,
            ],
        ];
        let list = if self.kind == EncKind::EucJp {
            &EUCJP
        } else {
            &SJIS
        };
        list.get(idx as usize).copied()
    }

    /// ONIGENC_GET_CTYPE_CODE_RANGE: Ok((sb_out, ranges)) where ranges is
    /// the C layout [n, from1, to1, from2, to2, ...]; or Err(code)
    /// (ONIG_NO_SUPPORT_CONFIG when the encoding has no table)
    pub fn get_ctype_code_range(&self, ctype: u32) -> Result<(u32, &'static [u32]), i32> {
        match self.kind {
            EncKind::Utf8
            | EncKind::Utf16Be
            | EncKind::Utf16Le
            | EncKind::Utf32Be
            | EncKind::Utf32Le => {
                let sb_out = if self.kind == EncKind::Utf8 { 0x80 } else { 0 };
                match ud::CODE_RANGES.get(ctype as usize) {
                    Some(r) => Ok((sb_out, r)),
                    None => Err(ONIGERR_TYPE_BUG),
                }
            }
            EncKind::EucJp | EncKind::Sjis => {
                if ctype <= ONIGENC_MAX_STD_CTYPE {
                    Err(ONIG_NO_SUPPORT_CONFIG)
                } else {
                    match self.jp_property_list(ctype - (ONIGENC_MAX_STD_CTYPE + 1)) {
                        Some(r) => Ok((0x80, r)),
                        None => Err(ONIGERR_TYPE_BUG),
                    }
                }
            }
            _ => Err(ONIG_NO_SUPPORT_CONFIG),
        }
    }

    /// ONIGENC_LEFT_ADJUST_CHAR_HEAD(enc, start, s)
    pub fn left_adjust_char_head(&self, s: &[u8], start: usize, p: usize) -> usize {
        if p <= start {
            return p;
        }
        match self.kind {
            EncKind::Utf8 => {
                let mut q = p;
                while (b(s, q) & 0xc0) == 0x80 && q > start {
                    q -= 1;
                }
                q
            }
            EncKind::Utf16Be => {
                let mut q = p;
                if (q - start) % 2 == 1 {
                    q -= 1;
                }
                if (b(s, q) & 0xfc) == 0xdc && q > start + 1 && (b(s, q - 2) & 0xfc) == 0xd8 {
                    q -= 2;
                }
                q
            }
            EncKind::Utf16Le => {
                let mut q = p;
                if !(q - start).is_multiple_of(2) {
                    q -= 1;
                }
                if (b(s, q + 1) & 0xfc) == 0xdc && q > start + 1 && (b(s, q - 1) & 0xfc) == 0xd8 {
                    q -= 2;
                }
                q
            }
            EncKind::Utf32Be | EncKind::Utf32Le => p - (p - start) % 4,
            EncKind::EucJp | EncKind::EucTw | EncKind::EucKr | EncKind::EucCn => {
                let islead = |c: u8| {
                    if matches!(self.kind, EncKind::EucKr | EncKind::EucCn) {
                        c < 0xa1 || c == 0xff
                    } else {
                        c.wrapping_sub(0xa1) > 0xfe - 0xa1
                    }
                };
                let mut q = p;
                while !islead(b(s, q)) && q > start {
                    q -= 1;
                }
                let len = self.mbc_enc_len(s, q);
                if q + len > p {
                    return q;
                }
                q += len;
                q + ((p - q) & !1)
            }
            EncKind::Sjis | EncKind::Big5 => {
                let (first, trail) = if self.kind == EncKind::Sjis {
                    (&et::ENCLEN_SJIS, &et::SJIS_CAN_BE_TRAIL)
                } else {
                    (&et::ENCLEN_BIG5, &et::BIG5_CAN_BE_TRAIL)
                };
                let mut q = p;
                if trail[b(s, q) as usize] != 0 {
                    while q > start {
                        q -= 1;
                        if first[b(s, q) as usize] <= 1 {
                            q += 1;
                            break;
                        }
                    }
                }
                let len = self.mbc_enc_len(s, q);
                if q + len > p {
                    return q;
                }
                q += len;
                q + ((p - q) & !1)
            }
            _ => p,
        }
    }

    /// ONIGENC_IS_ALLOWED_REVERSE_MATCH
    pub fn is_allowed_reverse_match(&self, s: &[u8], p: usize, end: usize) -> bool {
        let _ = end;
        let c = b(s, p);
        match self.kind {
            EncKind::Utf16Be | EncKind::Utf16Le | EncKind::Utf32Be | EncKind::Utf32Le => false,
            EncKind::EucJp => c <= 0x7e || c == 0x8e || c == 0x8f,
            EncKind::Sjis => et::SJIS_CAN_BE_TRAIL[c as usize] == 0,
            EncKind::Big5 => et::BIG5_CAN_BE_TRAIL[c as usize] == 0,
            EncKind::EucTw | EncKind::EucKr | EncKind::EucCn => c <= 0x7e,
            _ => true,
        }
    }

    /// ONIGENC_IS_VALID_MBC_STRING
    pub fn is_valid_mbc_string(&self, s: &[u8], start: usize, end: usize) -> bool {
        let mut p = start;
        match self.kind {
            EncKind::Utf8 => {
                while p < end {
                    if (b(s, p) & 0xc0) == 0x80 {
                        return false;
                    }
                    let len = self.mbc_enc_len(s, p);
                    p += 1;
                    for _ in 1..len {
                        if p == end {
                            return false;
                        }
                        if (b(s, p) & 0xc0) != 0x80 {
                            return false;
                        }
                        p += 1;
                    }
                }
                true
            }
            EncKind::Utf16Be => {
                while p < end {
                    let len = self.mbc_enc_len(s, p);
                    if len == 4 {
                        if p + 2 >= end {
                            return false;
                        }
                        if (b(s, p + 2) & 0xfc) != 0xdc {
                            return false;
                        }
                    } else if (b(s, p) & 0xfc) == 0xdc {
                        return false;
                    }
                    p += len;
                }
                p == end
            }
            EncKind::Utf16Le => {
                // end1 = end - 1 (pointer arithmetic; end > start here or loop skipped)
                while end >= 1 && p < end - 1 {
                    let len = self.mbc_enc_len(s, p);
                    if len == 4 {
                        if p + 3 < end && (b(s, p + 3) & 0xfc) != 0xdc {
                            return false;
                        }
                    } else if (b(s, p + 1) & 0xfc) == 0xdc {
                        return false;
                    }
                    p += len;
                }
                p == end
            }
            EncKind::Utf32Be | EncKind::Utf32Le => {
                // onigenc_length_check_is_valid_mbc_string
                while p < end {
                    p += 4;
                }
                p == end
            }
            EncKind::EucJp => {
                while p < end {
                    let c = b(s, p);
                    if c < 0x80 {
                        p += 1;
                    } else if c > 0xa0 {
                        if c == 0xff {
                            return false;
                        }
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if c < 0xa1 || c == 0xff {
                            return false;
                        }
                        p += 1;
                    } else if c == 0x8e {
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if !(0xa1..=0xdf).contains(&c) {
                            return false;
                        }
                        p += 1;
                    } else if c == 0x8f {
                        for _ in 0..2 {
                            p += 1;
                            if p >= end {
                                return false;
                            }
                            let c = b(s, p);
                            if c < 0xa1 || c == 0xff {
                                return false;
                            }
                        }
                        p += 1;
                    } else {
                        return false;
                    }
                }
                true
            }
            EncKind::Sjis => {
                while p < end {
                    let c = b(s, p);
                    if c < 0x80 {
                        p += 1;
                    } else if c < 0xa1 {
                        if c == 0xa0 || c == 0x80 {
                            return false;
                        }
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if !(0x40..=0xfc).contains(&c) || c == 0x7f {
                            return false;
                        }
                        p += 1;
                    } else if c < 0xe0 {
                        p += 1;
                    } else if c < 0xfd {
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if !(0x40..=0xfc).contains(&c) || c == 0x7f {
                            return false;
                        }
                        p += 1;
                    } else {
                        return false;
                    }
                }
                true
            }
            EncKind::Big5 => {
                while p < end {
                    let c = b(s, p);
                    if c < 0x80 {
                        p += 1;
                    } else if c < 0xa1 {
                        return false;
                    } else if c < 0xff {
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if c < 0x40 || (c > 0x7e && c < 0xa1) || c == 0xff {
                            return false;
                        }
                        p += 1;
                    } else {
                        return false;
                    }
                }
                true
            }
            EncKind::EucTw => {
                while p < end {
                    let c = b(s, p);
                    if c < 0x80 {
                        p += 1;
                    } else if c < 0xa1 {
                        if c != 0x8e {
                            return false;
                        }
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if !(0xa1..=0xb0).contains(&c) {
                            return false;
                        }
                        for _ in 0..2 {
                            p += 1;
                            if p >= end {
                                return false;
                            }
                            let c = b(s, p);
                            if c < 0xa1 || c == 0xff {
                                return false;
                            }
                        }
                        p += 1;
                    } else if c < 0xff {
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if c < 0xa1 || c == 0xff {
                            return false;
                        }
                        p += 1;
                    } else {
                        return false;
                    }
                }
                true
            }
            EncKind::EucKr | EncKind::EucCn => {
                while p < end {
                    let c = b(s, p);
                    if c < 0x80 {
                        p += 1;
                    } else if c < 0xa1 {
                        return false;
                    } else if c < 0xff {
                        p += 1;
                        if p >= end {
                            return false;
                        }
                        let c = b(s, p);
                        if c < 0xa1 || c == 0xff {
                            return false;
                        }
                        p += 1;
                    } else {
                        return false;
                    }
                }
                true
            }
            _ => true,
        }
    }

    #[inline]
    pub fn is_unicode(&self) -> bool {
        self.flag & ENC_FLAG_UNICODE != 0
    }
    #[inline]
    pub fn is_ascii_compatible(&self) -> bool {
        self.flag & ENC_FLAG_ASCII_COMPATIBLE != 0
    }
    #[inline]
    pub fn is_singlebyte(&self) -> bool {
        self.max_enc_len == 1
    }
    #[inline]
    pub fn skip_offset(&self) -> u32 {
        (self.flag & ENC_FLAG_SKIP_OFFSET_MASK) >> 2
    }
    #[inline]
    pub fn is_mbc_head(&self, s: &[u8], p: usize) -> bool {
        self.mbc_enc_len(s, p) != 1
    }
    #[inline]
    pub fn is_code_word(&self, code: u32) -> bool {
        self.is_code_ctype(code, ONIGENC_CTYPE_WORD)
    }
    #[inline]
    pub fn is_code_newline(&self, code: u32) -> bool {
        self.is_code_ctype(code, ONIGENC_CTYPE_NEWLINE)
    }
    #[inline]
    pub fn is_code_digit(&self, code: u32) -> bool {
        self.is_code_ctype(code, ONIGENC_CTYPE_DIGIT)
    }
    #[inline]
    pub fn is_mbc_word(&self, s: &[u8], p: usize, end: usize) -> bool {
        self.is_code_word(self.mbc_to_code(s, p, end))
    }
}

/* ===== regenc.c case fold helpers ===== */

/// OnigAsciiLowerMap pairs
fn ascii_apply_all_case_fold(f: &mut dyn FnMut(u32, &[u32]) -> i32) -> i32 {
    for from in 0x41u32..=0x5a {
        let to = from + 0x20;
        let r = f(from, &[to]);
        if r != 0 {
            return r;
        }
        let r = f(to, &[from]);
        if r != 0 {
            return r;
        }
    }
    0
}

fn ascii_get_case_fold_codes_by_str(
    s: &[u8],
    p: usize,
    items: &mut [CaseFoldCodeItem; ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM],
) -> i32 {
    let c = b(s, p) as u32;
    if (0x41..=0x5a).contains(&c) {
        items[0] = item(1, &[c + 0x20]);
        1
    } else if (0x61..=0x7a).contains(&c) {
        items[0] = item(1, &[c - 0x20]);
        1
    } else {
        0
    }
}

#[inline]
fn item(byte_len: i32, codes: &[u32]) -> CaseFoldCodeItem {
    let mut it = CaseFoldCodeItem {
        byte_len,
        code_len: codes.len() as i32,
        code: [0; ONIGENC_MAX_COMP_CASE_FOLD_CODE_LEN],
    };
    it.code[..codes.len()].copy_from_slice(codes);
    it
}

/// onigenc_apply_all_case_fold_with_map
fn apply_all_case_fold_with_map(
    map: &[(u8, u8)],
    ess_tsett: bool,
    flag: u32,
    f: &mut dyn FnMut(u32, &[u32]) -> i32,
) -> i32 {
    let r = ascii_apply_all_case_fold(f);
    if r != 0 {
        return r;
    }
    if case_fold_is_ascii_only(flag) {
        return 0;
    }
    for &(from, to) in map {
        let r = f(from as u32, &[to as u32]);
        if r != 0 {
            return r;
        }
        let r = f(to as u32, &[from as u32]);
        if r != 0 {
            return r;
        }
    }
    if ess_tsett {
        return f(0xdf, &[SMALL_S as u32, SMALL_S as u32]);
    }
    0
}

/// The "SS"/"ss" combinations (items 0..4) shared by the ess-tsett encodings
fn ss_combination(
    s: &[u8],
    p: usize,
    items: &mut [CaseFoldCodeItem; ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM],
) -> i32 {
    let sa = [LARGE_S, SMALL_S];
    items[0] = item(2, &[0xdf]);
    let mut n = 1;
    for &i in &sa {
        for &j in &sa {
            if i == b(s, p) && j == b(s, p + 1) {
                continue;
            }
            items[n] = item(2, &[i as u32, j as u32]);
            n += 1;
        }
    }
    4
}

fn sharp_s_items(items: &mut [CaseFoldCodeItem; ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM]) -> i32 {
    items[0] = item(1, &[b's' as u32, b's' as u32]);
    items[1] = item(1, &[b'S' as u32, b'S' as u32]);
    items[2] = item(1, &[b's' as u32, b'S' as u32]);
    items[3] = item(1, &[b'S' as u32, b's' as u32]);
    4
}

/// onigenc_get_case_fold_codes_by_str_with_map
fn get_case_fold_codes_by_str_with_map(
    map: &[(u8, u8)],
    ess_tsett: bool,
    flag: u32,
    s: &[u8],
    p: usize,
    end: usize,
    items: &mut [CaseFoldCodeItem; ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM],
) -> i32 {
    let c = b(s, p);
    if (0x41..=0x5a).contains(&c) {
        if c == LARGE_S
            && ess_tsett
            && end > p + 1
            && (b(s, p + 1) == LARGE_S || b(s, p + 1) == SMALL_S)
            && !case_fold_is_ascii_only(flag)
        {
            return ss_combination(s, p, items);
        }
        items[0] = item(1, &[c as u32 + 0x20]);
        1
    } else if (0x61..=0x7a).contains(&c) {
        if c == SMALL_S
            && ess_tsett
            && end > p + 1
            && (b(s, p + 1) == SMALL_S || b(s, p + 1) == LARGE_S)
            && !case_fold_is_ascii_only(flag)
        {
            return ss_combination(s, p, items);
        }
        items[0] = item(1, &[c as u32 - 0x20]);
        1
    } else if c == 0xdf && ess_tsett && !case_fold_is_ascii_only(flag) {
        sharp_s_items(items)
    } else {
        if case_fold_is_ascii_only(flag) {
            return 0;
        }
        for &(from, to) in map {
            if c == from {
                items[0] = item(1, &[to as u32]);
                return 1;
            } else if c == to {
                items[0] = item(1, &[from as u32]);
                return 1;
            }
        }
        0
    }
}

/// iso8859_1.c get_case_fold_codes_by_str
fn latin1_get_case_fold_codes_by_str(
    flag: u32,
    s: &[u8],
    p: usize,
    end: usize,
    items: &mut [CaseFoldCodeItem; ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM],
) -> i32 {
    let c = b(s, p);
    let not_ascii_only = !case_fold_is_ascii_only(flag);
    if (0x41..=0x5a).contains(&c) {
        if c == LARGE_S
            && end > p + 1
            && (b(s, p + 1) == LARGE_S || b(s, p + 1) == SMALL_S)
            && not_ascii_only
        {
            return ss_combination(s, p, items);
        }
        items[0] = item(1, &[c as u32 + 0x20]);
        1
    } else if (0x61..=0x7a).contains(&c) {
        if c == SMALL_S
            && end > p + 1
            && (b(s, p + 1) == SMALL_S || b(s, p + 1) == LARGE_S)
            && not_ascii_only
        {
            return ss_combination(s, p, items);
        }
        items[0] = item(1, &[c as u32 - 0x20]);
        1
    } else if not_ascii_only {
        if (0xc0..=0xcf).contains(&c) {
            items[0] = item(1, &[c as u32 + 0x20]);
            1
        } else if (0xd0..=0xdf).contains(&c) {
            if c == 0xdf {
                sharp_s_items(items)
            } else if c != 0xd7 {
                items[0] = item(1, &[c as u32 + 0x20]);
                1
            } else {
                0
            }
        } else if (0xe0..=0xef).contains(&c) || ((0xf0..=0xfe).contains(&c) && c != 0xf7) {
            items[0] = item(1, &[c as u32 - 0x20]);
            1
        } else {
            0
        }
    } else {
        0
    }
}

/// onigenc_minimum_property_name_to_ctype
fn minimum_property_name_to_ctype(enc: &Encoding, name: &[u8]) -> i32 {
    const PBS: [(&[u8], i32); 14] = [
        (b"Alnum", ONIGENC_CTYPE_ALNUM as i32),
        (b"Alpha", ONIGENC_CTYPE_ALPHA as i32),
        (b"Blank", ONIGENC_CTYPE_BLANK as i32),
        (b"Cntrl", ONIGENC_CTYPE_CNTRL as i32),
        (b"Digit", ONIGENC_CTYPE_DIGIT as i32),
        (b"Graph", ONIGENC_CTYPE_GRAPH as i32),
        (b"Lower", ONIGENC_CTYPE_LOWER as i32),
        (b"Print", ONIGENC_CTYPE_PRINT as i32),
        (b"Punct", ONIGENC_CTYPE_PUNCT as i32),
        (b"Space", ONIGENC_CTYPE_SPACE as i32),
        (b"Upper", ONIGENC_CTYPE_UPPER as i32),
        (b"XDigit", ONIGENC_CTYPE_XDIGIT as i32),
        (b"ASCII", ONIGENC_CTYPE_ASCII as i32),
        (b"Word", ONIGENC_CTYPE_WORD as i32),
    ];
    let len = strlen(enc, name, 0, name.len());
    for (pb, ctype) in PBS {
        if len == pb.len() as i32 && with_ascii_strncmp(enc, name, 0, name.len(), pb, pb.len()) == 0
        {
            return ctype;
        }
    }
    ONIGERR_INVALID_CHAR_PROPERTY_NAME
}

/* ===== unicode.c ===== */

fn unfold_key(code: u32) -> Option<(usize, usize)> {
    ud::UNFOLD_KEYS
        .binary_search_by_key(&code, |e| e.0)
        .ok()
        .map(|i| (ud::UNFOLD_KEYS[i].1 as usize, ud::UNFOLD_KEYS[i].2 as usize))
}

fn fold1_map() -> &'static HashMap<u32, usize> {
    static M: OnceLock<HashMap<u32, usize>> = OnceLock::new();
    M.get_or_init(|| {
        ud::FOLD1_KEY_INDEXES
            .iter()
            .map(|&i| (ud::FOLDS1[i as usize], i as usize))
            .collect()
    })
}

fn fold2_map() -> &'static HashMap<[u32; 2], usize> {
    static M: OnceLock<HashMap<[u32; 2], usize>> = OnceLock::new();
    M.get_or_init(|| {
        ud::FOLD2_KEY_INDEXES
            .iter()
            .map(|&i| {
                (
                    [ud::FOLDS2[i as usize], ud::FOLDS2[i as usize + 1]],
                    i as usize,
                )
            })
            .collect()
    })
}

fn fold3_map() -> &'static HashMap<[u32; 3], usize> {
    static M: OnceLock<HashMap<[u32; 3], usize>> = OnceLock::new();
    M.get_or_init(|| {
        ud::FOLD3_KEY_INDEXES
            .iter()
            .map(|&i| {
                let i = i as usize;
                ([ud::FOLDS3[i], ud::FOLDS3[i + 1], ud::FOLDS3[i + 2]], i)
            })
            .collect()
    })
}

/// onigenc_unicode_fold2_key
pub fn unicode_fold2_key(codes: [u32; 2]) -> Option<usize> {
    fold2_map().get(&codes).copied()
}

/// onigenc_unicode_fold3_key
pub fn unicode_fold3_key(codes: [u32; 3]) -> Option<usize> {
    fold3_map().get(&codes).copied()
}

/// onigenc_unicode_unfold_key: (index into the Folds<fold_len> array, fold_len)
pub fn unicode_unfold_key(code: u32) -> Option<(usize, usize)> {
    unfold_key(code)
}

#[inline]
fn folds1_unfolds(i: usize) -> &'static [u32] {
    let n = ud::FOLDS1[i + 1] as usize;
    &ud::FOLDS1[i + 2..i + 2 + n]
}
#[inline]
fn folds2_unfolds(i: usize) -> &'static [u32] {
    let n = ud::FOLDS2[i + 2] as usize;
    &ud::FOLDS2[i + 3..i + 3 + n]
}
#[inline]
fn folds3_unfolds(i: usize) -> &'static [u32] {
    let n = ud::FOLDS3[i + 3] as usize;
    &ud::FOLDS3[i + 4..i + 4 + n]
}
#[inline]
fn folds_fold(fold_len: usize, i: usize) -> &'static [u32] {
    match fold_len {
        1 => &ud::FOLDS1[i..i + 1],
        2 => &ud::FOLDS2[i..i + 2],
        _ => &ud::FOLDS3[i..i + 3],
    }
}

/// onigenc_unicode_mbc_case_fold
fn unicode_mbc_case_fold(
    enc: &Encoding,
    flag: u32,
    s: &[u8],
    pp: &mut usize,
    end: usize,
    fold: &mut [u8; ONIGENC_MBC_CASE_FOLD_MAXLEN],
) -> usize {
    let p = *pp;
    let code = enc.mbc_to_code(s, p, end);
    let len = enc.mbc_enc_len(s, p);
    *pp += len;
    let not_ascii_only = !case_fold_is_ascii_only(flag);
    if not_ascii_only || code < 0x80 {
        if let Some((index, fold_len)) = unfold_key(code) {
            if fold_len == 1 {
                let f = ud::FOLDS1[index];
                if not_ascii_only || f < 0x80 {
                    let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
                    let n = enc.code_to_mbc(f, &mut buf).max(0) as usize;
                    fold[..n].copy_from_slice(&buf[..n]);
                    return n;
                }
            } else {
                let mut rlen = 0;
                for &c in folds_fold(fold_len, index) {
                    let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
                    let n = enc.code_to_mbc(c, &mut buf).max(0) as usize;
                    fold[rlen..rlen + n].copy_from_slice(&buf[..n]);
                    rlen += n;
                }
                return rlen;
            }
        }
    }
    for (i, f) in fold.iter_mut().enumerate().take(len) {
        *f = b(s, p + i);
    }
    len
}

fn apply_case_fold1(
    flag: u32,
    from: usize,
    to: usize,
    f: &mut dyn FnMut(u32, &[u32]) -> i32,
) -> i32 {
    let ascii_only = case_fold_is_ascii_only(flag);
    let mut i = from;
    while i < to {
        let fold = ud::FOLDS1[i];
        if ascii_only && fold >= 0x80 {
            break;
        }
        let unfolds = folds1_unfolds(i);
        for (j, &unfold) in unfolds.iter().enumerate() {
            if ascii_only && unfold >= 0x80 {
                continue;
            }
            let r = f(fold, &[unfold]);
            if r != 0 {
                return r;
            }
            let r = f(unfold, &[fold]);
            if r != 0 {
                return r;
            }
            for &unfold2 in &unfolds[..j] {
                if ascii_only && unfold2 >= 0x80 {
                    continue;
                }
                let r = f(unfold, &[unfold2]);
                if r != 0 {
                    return r;
                }
                let r = f(unfold2, &[unfold]);
                if r != 0 {
                    return r;
                }
            }
        }
        i += 2 + unfolds.len();
    }
    0
}

fn apply_case_fold_n(
    nf: usize,
    from: usize,
    to: usize,
    f: &mut dyn FnMut(u32, &[u32]) -> i32,
) -> i32 {
    let mut i = from;
    while i < to {
        let (fold, unfolds) = if nf == 2 {
            (&ud::FOLDS2[i..i + 2], folds2_unfolds(i))
        } else {
            (&ud::FOLDS3[i..i + 3], folds3_unfolds(i))
        };
        for (j, &unfold) in unfolds.iter().enumerate() {
            let r = f(unfold, fold);
            if r != 0 {
                return r;
            }
            for &unfold2 in &unfolds[..j] {
                let r = f(unfold, &[unfold2]);
                if r != 0 {
                    return r;
                }
                let r = f(unfold2, &[unfold]);
                if r != 0 {
                    return r;
                }
            }
        }
        i += nf + 1 + unfolds.len();
    }
    0
}

/// onigenc_unicode_apply_all_case_fold
fn unicode_apply_all_case_fold(flag: u32, f: &mut dyn FnMut(u32, &[u32]) -> i32) -> i32 {
    let r = apply_case_fold1(flag, 0, ud::FOLDS1_NORMAL_END_INDEX, f);
    if r != 0 {
        return r;
    }
    let r = apply_case_fold1(flag, ud::FOLDS1_NORMAL_END_INDEX, ud::FOLDS1_END_INDEX, f);
    if r != 0 {
        return r;
    }
    if flag & INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR == 0 {
        return 0;
    }
    let r = apply_case_fold_n(2, 0, ud::FOLDS2_NORMAL_END_INDEX, f);
    if r != 0 {
        return r;
    }
    let r = apply_case_fold_n(2, ud::FOLDS2_NORMAL_END_INDEX, ud::FOLDS2_END_INDEX, f);
    if r != 0 {
        return r;
    }
    apply_case_fold_n(3, 0, ud::FOLDS3_NORMAL_END_INDEX, f)
}

/// cs[fn] = { fold code, its single-char unfolds... }
fn fold_alternatives(code: u32) -> Vec<u32> {
    let mut v = vec![code];
    if let Some(si) = unicode_fold1_key(code) {
        v.extend_from_slice(folds1_unfolds(si));
    }
    v
}

/// onigenc_unicode_get_case_fold_codes_by_str
fn unicode_get_case_fold_codes_by_str(
    enc: &Encoding,
    flag: u32,
    s: &[u8],
    mut p: usize,
    end: usize,
    items: &mut [CaseFoldCodeItem; ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM],
) -> i32 {
    let mut n = 0usize;
    let code = enc.mbc_to_code(s, p, end);
    let ascii_only = case_fold_is_ascii_only(flag);
    if ascii_only && code >= 0x80 {
        return 0;
    }
    let len = enc.mbc_enc_len(s, p);
    let mut orig_codes = [code, 0, 0];
    let mut codes = [0u32; 3];
    let mut lens = [len as i32, 0, 0];
    p += len;

    let buk1 = unfold_key(orig_codes[0]);
    codes[0] = match buk1 {
        Some((idx, 1)) => ud::FOLDS1[idx],
        _ => orig_codes[0],
    };

    let single_fold = |c: u32| match unfold_key(c) {
        Some((idx, 1)) => ud::FOLDS1[idx],
        _ => c,
    };

    if flag & INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR != 0 && p < end {
        let code = enc.mbc_to_code(s, p, end);
        orig_codes[1] = code;
        let len = enc.mbc_enc_len(s, p);
        lens[1] = lens[0] + len as i32;
        codes[1] = single_fold(code);
        p += len;
        if p < end {
            let code = enc.mbc_to_code(s, p, end);
            orig_codes[2] = code;
            let len = enc.mbc_enc_len(s, p);
            lens[2] = lens[1] + len as i32;
            codes[2] = single_fold(code);

            if let Some(index) = unicode_fold3_key(codes) {
                for &u in folds3_unfolds(index) {
                    items[n] = item(lens[2], &[u]);
                    n += 1;
                }
                let cs: Vec<Vec<u32>> = (0..3)
                    .map(|k| fold_alternatives(ud::FOLDS3[index + k]))
                    .collect();
                for &a in &cs[0] {
                    for &bb in &cs[1] {
                        for &c in &cs[2] {
                            if a == orig_codes[0] && bb == orig_codes[1] && c == orig_codes[2] {
                                continue;
                            }
                            items[n] = item(lens[2], &[a, bb, c]);
                            n += 1;
                        }
                    }
                }
                return n as i32;
            }
        }

        if let Some(index) = unicode_fold2_key([codes[0], codes[1]]) {
            for &u in folds2_unfolds(index) {
                items[n] = item(lens[1], &[u]);
                n += 1;
            }
            let cs: Vec<Vec<u32>> = (0..2)
                .map(|k| fold_alternatives(ud::FOLDS2[index + k]))
                .collect();
            for &a in &cs[0] {
                for &bb in &cs[1] {
                    if a == orig_codes[0] && bb == orig_codes[1] {
                        continue;
                    }
                    items[n] = item(lens[1], &[a, bb]);
                    n += 1;
                }
            }
            return n as i32;
        }
    }

    // fold1:
    match buk1 {
        Some((idx, 1)) => {
            let f = ud::FOLDS1[idx];
            if !ascii_only || f < 0x80 {
                items[0] = item(lens[0], &[f]);
                n += 1;
            }
            for &unfold in folds1_unfolds(idx) {
                if unfold != orig_codes[0] && (!ascii_only || unfold < 0x80) {
                    items[n] = item(lens[0], &[unfold]);
                    n += 1;
                }
            }
        }
        Some((idx, fold_len)) => {
            if flag & INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR != 0 {
                let unfolds = if fold_len == 2 {
                    folds2_unfolds(idx)
                } else {
                    folds3_unfolds(idx)
                };
                for &unfold in unfolds {
                    if unfold == orig_codes[0] {
                        continue;
                    }
                    items[n] = item(lens[0], &[unfold]);
                    n += 1;
                }
                let fold = folds_fold(fold_len, idx);
                let cs: Vec<Vec<u32>> = fold.iter().map(|&c| fold_alternatives(c)).collect();
                if fold_len == 2 {
                    for &a in &cs[0] {
                        for &bb in &cs[1] {
                            items[n] = item(lens[0], &[a, bb]);
                            n += 1;
                        }
                    }
                } else {
                    for &a in &cs[0] {
                        for &bb in &cs[1] {
                            for &c in &cs[2] {
                                items[n] = item(lens[0], &[a, bb, c]);
                                n += 1;
                            }
                        }
                    }
                }
            }
        }
        None => {
            if let Some(index) = unicode_fold1_key(orig_codes[0]) {
                for &code in folds1_unfolds(index) {
                    if !ascii_only || code < 0x80 {
                        items[n] = item(lens[0], &[code]);
                        n += 1;
                    }
                }
            }
        }
    }
    n as i32
}

/// onigenc_unicode_is_code_ctype
pub fn unicode_is_code_ctype(code: u32, ctype: u32) -> bool {
    if ctype <= ONIGENC_MAX_STD_CTYPE && code < 256 {
        return table_ctype(&ud::ISO_8859_1_CTYPE, code, ctype);
    }
    match ud::CODE_RANGES.get(ctype as usize) {
        Some(r) => is_in_code_range(r, code),
        None => true, /* ONIGERR_TYPE_BUG (non-zero) */
    }
}

/// onigenc_unicode_property_name_to_ctype
fn unicode_property_name_to_ctype(enc: &Encoding, name: &[u8]) -> i32 {
    let end = name.len();
    let mut buf: Vec<u8> = Vec::with_capacity(32);
    let mut p = 0;
    while p < end {
        let code = enc.mbc_to_code(name, p, end);
        if code >= 0x80 {
            return ONIGERR_INVALID_CHAR_PROPERTY_NAME;
        }
        if code != b' ' as u32 && code != b'-' as u32 && code != b'_' as u32 {
            buf.push(code as u8);
            if buf.len() >= ud::PROPERTY_NAME_MAX_SIZE {
                return ONIGERR_INVALID_CHAR_PROPERTY_NAME;
            }
        }
        p += enc.mbc_enc_len(name, p);
    }
    // gperf lookup: ASCII case-insensitive, an embedded NUL never matches
    if buf.contains(&0) {
        return ONIGERR_INVALID_CHAR_PROPERTY_NAME;
    }
    buf.make_ascii_lowercase();
    let key = match std::str::from_utf8(&buf) {
        Ok(k) => k,
        Err(_) => return ONIGERR_INVALID_CHAR_PROPERTY_NAME,
    };
    match ud::PROPERTY_NAMES.binary_search_by(|e| e.0.cmp(key)) {
        Ok(i) => ud::PROPERTY_NAMES[i].1 as i32,
        Err(_) => ONIGERR_INVALID_CHAR_PROPERTY_NAME,
    }
}

/* ===== regenc.c helpers ===== */

/// onigenc_get_right_adjust_char_head
pub fn get_right_adjust_char_head(enc: &Encoding, s: &[u8], start: usize, p: usize) -> usize {
    let q = enc.left_adjust_char_head(s, start, p);
    if q < p { q + enc.mbc_enc_len(s, q) } else { q }
}

/// onigenc_get_right_adjust_char_head_with_prev: (head, prev) where prev
/// is None for C's NULL
pub fn get_right_adjust_char_head_with_prev(
    enc: &Encoding,
    s: &[u8],
    start: usize,
    p: usize,
) -> (usize, Option<usize>) {
    let q = enc.left_adjust_char_head(s, start, p);
    if q < p {
        (q + enc.mbc_enc_len(s, q), Some(q))
    } else {
        (q, get_prev_char_head(enc, s, start, q))
    }
}

/// onigenc_get_prev_char_head: None for C's NULL (p <= start)
pub fn get_prev_char_head(enc: &Encoding, s: &[u8], start: usize, p: usize) -> Option<usize> {
    if p <= start {
        return None;
    }
    Some(enc.left_adjust_char_head(s, start, p - 1))
}

/// onigenc_step_back: None for C's NULL
pub fn step_back(enc: &Encoding, s: &[u8], start: usize, p: usize, n: i32) -> Option<usize> {
    let mut p = p;
    let mut n = n;
    while n > 0 {
        n -= 1;
        if p <= start {
            return None;
        }
        p = enc.left_adjust_char_head(s, start, p - 1);
    }
    Some(p)
}

/// onigenc_step
pub fn step(enc: &Encoding, s: &[u8], p: usize, end: usize, n: i32) -> Option<usize> {
    let mut q = p;
    for _ in 0..n.max(0) {
        q += enc.mbc_enc_len(s, q);
    }
    if q <= end { Some(q) } else { None }
}

/// onigenc_strlen
pub fn strlen(enc: &Encoding, s: &[u8], p: usize, end: usize) -> i32 {
    let mut n = 0;
    let mut q = p;
    while q < end {
        q += enc.mbc_enc_len(s, q);
        n += 1;
    }
    n
}

/// onigenc_with_ascii_strncmp: compares chars at s[p..end] to the ascii
/// string `sascii` (n chars); 0 when equal
pub fn with_ascii_strncmp(
    enc: &Encoding,
    s: &[u8],
    p: usize,
    end: usize,
    sascii: &[u8],
    n: usize,
) -> i32 {
    let mut p = p;
    for i in 0..n {
        let a = sascii.get(i).copied().unwrap_or(0) as i32;
        if p >= end {
            return a;
        }
        let c = enc.mbc_to_code(s, p, end) as i32;
        let x = a.wrapping_sub(c);
        if x != 0 {
            return x;
        }
        p += enc.mbc_enc_len(s, p);
    }
    0
}

/// onigenc_is_mbc_word_ascii
pub fn is_mbc_word_ascii(enc: &Encoding, s: &[u8], p: usize, end: usize) -> bool {
    let code = enc.mbc_to_code(s, p, end);
    code <= ASCII_LIMIT && is_ascii_code_ctype(code, ONIGENC_CTYPE_WORD)
}

/// ONIGENC_IS_MBC_CRNL
pub fn is_mbc_crnl(enc: &Encoding, s: &[u8], p: usize, end: usize) -> bool {
    enc.mbc_to_code(s, p, end) == 13 && enc.is_mbc_newline(s, p + enc.mbc_enc_len(s, p), end)
}

/* ===== unicode.c helpers used directly by the parser/matcher ===== */

/// onigenc_unicode_fold1_key(&code): index into OnigUnicodeFolds1 or None
pub fn unicode_fold1_key(code: u32) -> Option<usize> {
    fold1_map().get(&code).copied()
}

/// FOLDS1_UNFOLDS(index)[0..FOLDS1_UNFOLDS_NUM(index)]
pub fn unicode_folds1_unfolds(index: usize) -> &'static [u32] {
    folds1_unfolds(index)
}

/* EGCB types (unicode.c enum EGCB_TYPE) */
const EGCB_OTHER: u8 = 0;
const EGCB_CR: u8 = 1;
const EGCB_LF: u8 = 2;
const EGCB_CONTROL: u8 = 3;
const EGCB_EXTEND: u8 = 4;
const EGCB_PREPEND: u8 = 5;
const EGCB_REGIONAL_INDICATOR: u8 = 6;
const EGCB_SPACING_MARK: u8 = 7;
const EGCB_ZWJ: u8 = 8;
const EGCB_L: u8 = 13;
const EGCB_LV: u8 = 14;
const EGCB_LVT: u8 = 15;
const EGCB_T: u8 = 16;
const EGCB_V: u8 = 17;

fn range_type(ranges: &[(u32, u32, u8)], code: u32) -> u8 {
    let (mut low, mut high) = (0usize, ranges.len());
    while low < high {
        let x = (low + high) >> 1;
        if code > ranges[x].1 {
            low = x + 1;
        } else {
            high = x;
        }
    }
    if low < ranges.len() && code >= ranges[low].0 {
        ranges[low].2
    } else {
        0
    }
}

#[derive(PartialEq, Eq)]
enum EgcbBreak {
    NotBreak,
    Break,
    UndefGb11,
    UndefRiRi,
}

fn unicode_egcb_is_break_2code(from_code: u32, to_code: u32) -> EgcbBreak {
    let from = range_type(&ud::EGCB_RANGES, from_code);
    let to = range_type(&ud::EGCB_RANGES, to_code);
    if from == EGCB_OTHER && to == EGCB_OTHER {
        return EgcbBreak::Break;
    }
    if from == EGCB_CR && to == EGCB_LF {
        return EgcbBreak::NotBreak;
    }
    let is_ctl = |t: u8| (EGCB_CR..=EGCB_CONTROL).contains(&t);
    if is_ctl(from) || is_ctl(to) {
        return EgcbBreak::Break;
    }
    if from >= EGCB_L && to >= EGCB_L {
        if from == EGCB_L && to != EGCB_T {
            return EgcbBreak::NotBreak;
        }
        if (from == EGCB_LV || from == EGCB_V) && (to == EGCB_V || to == EGCB_T) {
            return EgcbBreak::NotBreak;
        }
        if to == EGCB_T && (from == EGCB_LVT || from == EGCB_T) {
            return EgcbBreak::NotBreak;
        }
        return EgcbBreak::Break;
    }
    if to == EGCB_EXTEND || to == EGCB_ZWJ {
        return EgcbBreak::NotBreak;
    }
    if to == EGCB_SPACING_MARK {
        return EgcbBreak::NotBreak;
    }
    if from == EGCB_PREPEND {
        return EgcbBreak::NotBreak;
    }
    if from == EGCB_ZWJ {
        if unicode_is_code_ctype(to_code, ud::PROP_INDEX_EXTENDEDPICTOGRAPHIC) {
            return EgcbBreak::UndefGb11;
        }
        return EgcbBreak::Break;
    }
    if from == EGCB_REGIONAL_INDICATOR && to == EGCB_REGIONAL_INDICATOR {
        return EgcbBreak::UndefRiRi;
    }
    EgcbBreak::Break
}

/// onigenc_egcb_is_break_position(enc, p, prev, start, end)
pub fn egcb_is_break_position(
    enc: &Encoding,
    s: &[u8],
    p: usize,
    prev: Option<usize>,
    start: usize,
    end: usize,
) -> bool {
    if p == start || p == end {
        return true;
    }
    let mut prev = match prev {
        Some(q) => q,
        None => match get_prev_char_head(enc, s, start, p) {
            Some(q) => q,
            None => return true,
        },
    };
    let from = enc.mbc_to_code(s, prev, end);
    let to = enc.mbc_to_code(s, p, end);
    if !enc.is_unicode() {
        return from != 0x0d || to != NEWLINE_CODE;
    }
    match unicode_egcb_is_break_2code(from, to) {
        EgcbBreak::NotBreak => false,
        EgcbBreak::Break => true,
        EgcbBreak::UndefGb11 => {
            while let Some(q) = get_prev_char_head(enc, s, start, prev) {
                prev = q;
                let from = enc.mbc_to_code(s, prev, end);
                if unicode_is_code_ctype(from, ud::PROP_INDEX_EXTENDEDPICTOGRAPHIC) {
                    return false;
                }
                if range_type(&ud::EGCB_RANGES, from) != EGCB_EXTEND {
                    break;
                }
            }
            true
        }
        EgcbBreak::UndefRiRi => {
            let mut n = 0;
            while let Some(q) = get_prev_char_head(enc, s, start, prev) {
                prev = q;
                let from = enc.mbc_to_code(s, prev, end);
                if range_type(&ud::EGCB_RANGES, from) != EGCB_REGIONAL_INDICATOR {
                    break;
                }
                n += 1;
            }
            n % 2 != 0
        }
    }
}

/* WB types (unicode.c enum WB_TYPE) */
const WB_ANY: u8 = 0;
const WB_ALETTER: u8 = 1;
const WB_CR: u8 = 2;
const WB_DOUBLE_QUOTE: u8 = 3;
const WB_EXTEND: u8 = 4;
const WB_EXTEND_NUM_LET: u8 = 5;
const WB_FORMAT: u8 = 6;
const WB_HEBREW_LETTER: u8 = 7;
const WB_KATAKANA: u8 = 8;
const WB_LF: u8 = 9;
const WB_MID_LETTER: u8 = 10;
const WB_MID_NUM: u8 = 11;
const WB_MID_NUM_LET: u8 = 12;
const WB_NEWLINE: u8 = 13;
const WB_NUMERIC: u8 = 14;
const WB_REGIONAL_INDICATOR: u8 = 15;
const WB_SINGLE_QUOTE: u8 = 16;
const WB_WSEG_SPACE: u8 = 17;
const WB_ZWJ: u8 = 18;

#[inline]
fn wb_type(code: u32) -> u8 {
    range_type(&ud::WB_RANGES, code)
}
#[inline]
fn is_wb_ignore_tail(t: u8) -> bool {
    t == WB_EXTEND || t == WB_FORMAT || t == WB_ZWJ
}
#[inline]
fn is_wb_ahletter(t: u8) -> bool {
    t == WB_ALETTER || t == WB_HEBREW_LETTER
}
#[inline]
fn is_wb_mid_num_let_q(t: u8) -> bool {
    t == WB_MID_NUM_LET || t == WB_SINGLE_QUOTE
}

/// wb_get_next_main_code: the type of the next char after p that is not
/// Extend/Format/ZWJ
fn wb_get_next_main_type(enc: &Encoding, s: &[u8], p: usize, end: usize) -> Option<u8> {
    let mut p = p;
    loop {
        p += enc.mbc_enc_len(s, p);
        if p >= end {
            return None;
        }
        let t = wb_type(enc.mbc_to_code(s, p, end));
        if !is_wb_ignore_tail(t) {
            return Some(t);
        }
    }
}

/// The type of the char before `prev` skipping Extend/Format/ZWJ (WB_Any
/// when there is none); `prev` is moved like the C loop does
fn wb_prev_main_type(enc: &Encoding, s: &[u8], start: usize, end: usize, prev: &mut usize) -> u8 {
    let mut from2 = WB_ANY;
    while let Some(q) = get_prev_char_head(enc, s, start, *prev) {
        *prev = q;
        from2 = wb_type(enc.mbc_to_code(s, q, end));
        if !is_wb_ignore_tail(from2) {
            break;
        }
    }
    from2
}

/// onigenc_wb_is_break_position(enc, p, prev, start, end)
pub fn wb_is_break_position(
    enc: &Encoding,
    s: &[u8],
    p: usize,
    prev: Option<usize>,
    start: usize,
    end: usize,
) -> bool {
    if p == start || p == end {
        return true;
    }
    let mut prev = match prev {
        Some(q) => q,
        None => match get_prev_char_head(enc, s, start, p) {
            Some(q) => q,
            None => return true,
        },
    };
    let cfrom = enc.mbc_to_code(s, prev, end);
    let cto = enc.mbc_to_code(s, p, end);
    let mut from = wb_type(cfrom);
    let to = wb_type(cto);

    if from == WB_ANY && to == WB_ANY {
        return true;
    }
    // WB3
    if from == WB_CR && to == WB_LF {
        return false;
    }
    // WB3a, WB3b
    if from == WB_NEWLINE || from == WB_CR || from == WB_LF {
        return true;
    }
    if to == WB_NEWLINE || to == WB_CR || to == WB_LF {
        return true;
    }
    // WB3c
    if from == WB_ZWJ && unicode_is_code_ctype(cto, ud::PROP_INDEX_EXTENDEDPICTOGRAPHIC) {
        return false;
    }
    // WB3d
    if from == WB_WSEG_SPACE && to == WB_WSEG_SPACE {
        return false;
    }
    // WB4
    if is_wb_ignore_tail(to) {
        return false;
    }
    if is_wb_ignore_tail(from) {
        while let Some(q) = get_prev_char_head(enc, s, start, prev) {
            prev = q;
            from = wb_type(enc.mbc_to_code(s, prev, end));
            if !is_wb_ignore_tail(from) {
                break;
            }
        }
    }

    if is_wb_ahletter(from) {
        // WB5
        if is_wb_ahletter(to) {
            return false;
        }
        // WB6
        if (to == WB_MID_LETTER || is_wb_mid_num_let_q(to))
            && wb_get_next_main_type(enc, s, p, end).is_some_and(is_wb_ahletter)
        {
            return false;
        }
    }

    // WB7
    if (from == WB_MID_LETTER || is_wb_mid_num_let_q(from))
        && is_wb_ahletter(to)
        && is_wb_ahletter(wb_prev_main_type(enc, s, start, end, &mut prev))
    {
        return false;
    }

    if from == WB_HEBREW_LETTER {
        // WB7a
        if to == WB_SINGLE_QUOTE {
            return false;
        }
        // WB7b
        if to == WB_DOUBLE_QUOTE && wb_get_next_main_type(enc, s, p, end) == Some(WB_HEBREW_LETTER)
        {
            return false;
        }
    }

    // WB7c
    if from == WB_DOUBLE_QUOTE
        && to == WB_HEBREW_LETTER
        && wb_prev_main_type(enc, s, start, end, &mut prev) == WB_HEBREW_LETTER
    {
        return false;
    }

    if to == WB_NUMERIC {
        // WB8
        if from == WB_NUMERIC {
            return false;
        }
        // WB9
        if is_wb_ahletter(from) {
            return false;
        }
        // WB11
        if (from == WB_MID_NUM || is_wb_mid_num_let_q(from))
            && wb_prev_main_type(enc, s, start, end, &mut prev) == WB_NUMERIC
        {
            return false;
        }
    }

    if from == WB_NUMERIC {
        // WB10
        if is_wb_ahletter(to) {
            return false;
        }
        // WB12
        if (to == WB_MID_NUM || is_wb_mid_num_let_q(to))
            && wb_get_next_main_type(enc, s, p, end) == Some(WB_NUMERIC)
        {
            return false;
        }
    }

    // WB13
    if from == WB_KATAKANA && to == WB_KATAKANA {
        return false;
    }
    // WB13a
    if (is_wb_ahletter(from)
        || from == WB_NUMERIC
        || from == WB_KATAKANA
        || from == WB_EXTEND_NUM_LET)
        && to == WB_EXTEND_NUM_LET
    {
        return false;
    }
    // WB13b
    if from == WB_EXTEND_NUM_LET && (is_wb_ahletter(to) || to == WB_NUMERIC || to == WB_KATAKANA) {
        return false;
    }

    // WB15, WB16
    if from == WB_REGIONAL_INDICATOR && to == WB_REGIONAL_INDICATOR {
        let mut n = 0;
        let mut q = prev;
        while let Some(r) = get_prev_char_head(enc, s, start, q) {
            q = r;
            if wb_type(enc.mbc_to_code(s, q, end)) != WB_REGIONAL_INDICATOR {
                break;
            }
            n += 1;
        }
        if n % 2 == 0 {
            return false;
        }
    }

    true
}
