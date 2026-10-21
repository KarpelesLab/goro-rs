//! Constants shared by the PCRE2 port: the generated values from the C
//! headers plus the private definitions from pcre2_compile.c and
//! pcre2_internal.h / pcre2_intmodedep.h for the 8-bit, LINK_SIZE=2 build.

#![allow(non_upper_case_globals, dead_code)]

pub use crate::consts_gen::*;
use crate::tables;

pub const LINK_SIZE: usize = 2;
pub const IMM2_SIZE: usize = 2;
pub const MAX_PATTERN_SIZE: usize = 1 << 16;
pub const MAX_MARK: usize = 255;
pub const MAX_NAME_SIZE: usize = 128;
pub const MAX_NAME_COUNT: usize = 10000;
pub const PARENS_NEST_LIMIT: u32 = 250;
pub const MATCH_LIMIT: u32 = 10_000_000;
pub const MATCH_LIMIT_DEPTH: u32 = MATCH_LIMIT;
pub const HEAP_LIMIT: u32 = 20_000_000;
pub const MAX_VARLOOKBEHIND: u32 = 255;
pub const MAX_NON_UTF_CHAR: u32 = 0xff;
pub const LOOKBEHIND_MAX: u32 = u16::MAX as u32;
pub const PCRE2_UNSET: usize = usize::MAX;
pub const PCRE2_MAJOR: u32 = 10;
pub const PCRE2_MINOR: u32 = 44;

// Character constants (ASCII).
pub const CHAR_NUL: u32 = 0;
pub const CHAR_HT: u32 = 9;
pub const CHAR_LF: u32 = 10;
pub const CHAR_NL: u32 = 10;
pub const CHAR_VT: u32 = 11;
pub const CHAR_FF: u32 = 12;
pub const CHAR_CR: u32 = 13;
pub const CHAR_BS: u32 = 8;
pub const CHAR_BEL: u32 = 7;
pub const CHAR_ESC: u32 = 27;
pub const CHAR_NEL: u32 = 0x85;
pub const CHAR_NBSP: u32 = 0xa0;
pub const CHAR_SPACE: u32 = b' ' as u32;

// Meta codes in the parsed pattern (pcre2_compile.c).
pub const META_END: u32 = 0x80000000;
pub const META_ALT: u32 = 0x80010000;
pub const META_ATOMIC: u32 = 0x80020000;
pub const META_BACKREF: u32 = 0x80030000;
pub const META_BACKREF_BYNAME: u32 = 0x80040000;
pub const META_BIGVALUE: u32 = 0x80050000;
pub const META_CALLOUT_NUMBER: u32 = 0x80060000;
pub const META_CALLOUT_STRING: u32 = 0x80070000;
pub const META_CAPTURE: u32 = 0x80080000;
pub const META_CIRCUMFLEX: u32 = 0x80090000;
pub const META_CLASS: u32 = 0x800a0000;
pub const META_CLASS_EMPTY: u32 = 0x800b0000;
pub const META_CLASS_EMPTY_NOT: u32 = 0x800c0000;
pub const META_CLASS_END: u32 = 0x800d0000;
pub const META_CLASS_NOT: u32 = 0x800e0000;
pub const META_COND_ASSERT: u32 = 0x800f0000;
pub const META_COND_DEFINE: u32 = 0x80100000;
pub const META_COND_NAME: u32 = 0x80110000;
pub const META_COND_NUMBER: u32 = 0x80120000;
pub const META_COND_RNAME: u32 = 0x80130000;
pub const META_COND_RNUMBER: u32 = 0x80140000;
pub const META_COND_VERSION: u32 = 0x80150000;
pub const META_DOLLAR: u32 = 0x80160000;
pub const META_DOT: u32 = 0x80170000;
pub const META_ESCAPE: u32 = 0x80180000;
pub const META_KET: u32 = 0x80190000;
pub const META_NOCAPTURE: u32 = 0x801a0000;
pub const META_OPTIONS: u32 = 0x801b0000;
pub const META_POSIX: u32 = 0x801c0000;
pub const META_POSIX_NEG: u32 = 0x801d0000;
pub const META_RANGE_ESCAPED: u32 = 0x801e0000;
pub const META_RANGE_LITERAL: u32 = 0x801f0000;
pub const META_RECURSE: u32 = 0x80200000;
pub const META_RECURSE_BYNAME: u32 = 0x80210000;
pub const META_SCRIPT_RUN: u32 = 0x80220000;
pub const META_LOOKAHEAD: u32 = 0x80230000;
pub const META_LOOKAHEADNOT: u32 = 0x80240000;
pub const META_LOOKBEHIND: u32 = 0x80250000;
pub const META_LOOKBEHINDNOT: u32 = 0x80260000;
pub const META_LOOKAHEAD_NA: u32 = 0x80270000;
pub const META_LOOKBEHIND_NA: u32 = 0x80280000;
pub const META_MARK: u32 = 0x80290000;
pub const META_ACCEPT: u32 = 0x802a0000;
pub const META_FAIL: u32 = 0x802b0000;
pub const META_COMMIT: u32 = 0x802c0000;
pub const META_COMMIT_ARG: u32 = 0x802d0000;
pub const META_PRUNE: u32 = 0x802e0000;
pub const META_PRUNE_ARG: u32 = 0x802f0000;
pub const META_SKIP: u32 = 0x80300000;
pub const META_SKIP_ARG: u32 = 0x80310000;
pub const META_THEN: u32 = 0x80320000;
pub const META_THEN_ARG: u32 = 0x80330000;
pub const META_ASTERISK: u32 = 0x80340000;
pub const META_ASTERISK_PLUS: u32 = 0x80350000;
pub const META_ASTERISK_QUERY: u32 = 0x80360000;
pub const META_PLUS: u32 = 0x80370000;
pub const META_PLUS_PLUS: u32 = 0x80380000;
pub const META_PLUS_QUERY: u32 = 0x80390000;
pub const META_QUERY: u32 = 0x803a0000;
pub const META_QUERY_PLUS: u32 = 0x803b0000;
pub const META_QUERY_QUERY: u32 = 0x803c0000;
pub const META_MINMAX: u32 = 0x803d0000;
pub const META_MINMAX_PLUS: u32 = 0x803e0000;
pub const META_MINMAX_QUERY: u32 = 0x803f0000;
pub const META_FIRST_QUANTIFIER: u32 = META_ASTERISK;
pub const META_LAST_QUANTIFIER: u32 = META_MINMAX_QUERY;
pub const META_ATOMIC_SCRIPT_RUN: u32 = 0x8fff0000;

#[inline]
pub const fn meta_code(x: u32) -> u32 {
    x & 0xffff0000
}
#[inline]
pub const fn meta_data(x: u32) -> u32 {
    x & 0x0000ffff
}

/// 64-bit build: offsets in the parsed pattern take two u32 slots.
pub const SIZEOFFSET: usize = 2;

/// Extra lengths for each meta code (indexed by (meta >> 16) & 0x7fff).
pub static META_EXTRA_LENGTHS: [u8; 64] = [
    0,                    // META_END
    0,                    // META_ALT
    0,                    // META_ATOMIC
    0,                    // META_BACKREF - more if group is >= 10
    1 + SIZEOFFSET as u8, // META_BACKREF_BYNAME
    1,                    // META_BIGVALUE
    3,                    // META_CALLOUT_NUMBER
    3 + SIZEOFFSET as u8, // META_CALLOUT_STRING
    0,                    // META_CAPTURE
    0,                    // META_CIRCUMFLEX
    0,                    // META_CLASS
    0,                    // META_CLASS_EMPTY
    0,                    // META_CLASS_EMPTY_NOT
    0,                    // META_CLASS_END
    0,                    // META_CLASS_NOT
    0,                    // META_COND_ASSERT
    SIZEOFFSET as u8,     // META_COND_DEFINE
    1 + SIZEOFFSET as u8, // META_COND_NAME
    1 + SIZEOFFSET as u8, // META_COND_NUMBER
    1 + SIZEOFFSET as u8, // META_COND_RNAME
    1 + SIZEOFFSET as u8, // META_COND_RNUMBER
    3,                    // META_COND_VERSION
    0,                    // META_DOLLAR
    0,                    // META_DOT
    0,                    // META_ESCAPE - more for ESC_P, ESC_p, ESC_g, ESC_k
    0,                    // META_KET
    0,                    // META_NOCAPTURE
    1,                    // META_OPTIONS
    1,                    // META_POSIX
    1,                    // META_POSIX_NEG
    0,                    // META_RANGE_ESCAPED
    0,                    // META_RANGE_LITERAL
    SIZEOFFSET as u8,     // META_RECURSE
    1 + SIZEOFFSET as u8, // META_RECURSE_BYNAME
    0,                    // META_SCRIPT_RUN
    0,                    // META_LOOKAHEAD
    0,                    // META_LOOKAHEADNOT
    SIZEOFFSET as u8,     // META_LOOKBEHIND
    SIZEOFFSET as u8,     // META_LOOKBEHINDNOT
    0,                    // META_LOOKAHEAD_NA
    SIZEOFFSET as u8,     // META_LOOKBEHIND_NA
    1,                    // META_MARK - plus the string length
    0,                    // META_ACCEPT
    0,                    // META_FAIL
    0,                    // META_COMMIT
    1,                    // META_COMMIT_ARG - plus the string length
    0,                    // META_PRUNE
    1,                    // META_PRUNE_ARG - plus the string length
    0,                    // META_SKIP
    1,                    // META_SKIP_ARG - plus the string length
    0,                    // META_THEN
    1,                    // META_THEN_ARG - plus the string length
    0,                    // META_ASTERISK
    0,                    // META_ASTERISK_PLUS
    0,                    // META_ASTERISK_QUERY
    0,                    // META_PLUS
    0,                    // META_PLUS_PLUS
    0,                    // META_PLUS_QUERY
    0,                    // META_QUERY
    0,                    // META_QUERY_PLUS
    0,                    // META_QUERY_QUERY
    2,                    // META_MINMAX
    2,                    // META_MINMAX_PLUS
    2,                    // META_MINMAX_QUERY
];

// Compile error codes (ERRn == COMPILE_ERROR_BASE + n).
macro_rules! errs {
    ($($name:ident = $n:expr),* $(,)?) => { $(pub const $name: i32 = 100 + $n;)* };
}
errs!(
    ERR0 = 0,
    ERR1 = 1,
    ERR2 = 2,
    ERR3 = 3,
    ERR4 = 4,
    ERR5 = 5,
    ERR6 = 6,
    ERR7 = 7,
    ERR8 = 8,
    ERR9 = 9,
    ERR10 = 10,
    ERR11 = 11,
    ERR12 = 12,
    ERR13 = 13,
    ERR14 = 14,
    ERR15 = 15,
    ERR16 = 16,
    ERR17 = 17,
    ERR18 = 18,
    ERR19 = 19,
    ERR20 = 20,
    ERR21 = 21,
    ERR22 = 22,
    ERR23 = 23,
    ERR24 = 24,
    ERR25 = 25,
    ERR26 = 26,
    ERR27 = 27,
    ERR28 = 28,
    ERR29 = 29,
    ERR30 = 30,
    ERR31 = 31,
    ERR32 = 32,
    ERR33 = 33,
    ERR34 = 34,
    ERR35 = 35,
    ERR36 = 36,
    ERR37 = 37,
    ERR38 = 38,
    ERR39 = 39,
    ERR40 = 40,
    ERR41 = 41,
    ERR42 = 42,
    ERR43 = 43,
    ERR44 = 44,
    ERR45 = 45,
    ERR46 = 46,
    ERR47 = 47,
    ERR48 = 48,
    ERR49 = 49,
    ERR50 = 50,
    ERR51 = 51,
    ERR52 = 52,
    ERR53 = 53,
    ERR54 = 54,
    ERR55 = 55,
    ERR56 = 56,
    ERR57 = 57,
    ERR58 = 58,
    ERR59 = 59,
    ERR60 = 60,
    ERR61 = 61,
    ERR62 = 62,
    ERR63 = 63,
    ERR64 = 64,
    ERR65 = 65,
    ERR66 = 66,
    ERR67 = 67,
    ERR68 = 68,
    ERR69 = 69,
    ERR70 = 70,
    ERR71 = 71,
    ERR72 = 72,
    ERR73 = 73,
    ERR74 = 74,
    ERR75 = 75,
    ERR76 = 76,
    ERR77 = 77,
    ERR78 = 78,
    ERR79 = 79,
    ERR80 = 80,
    ERR81 = 81,
    ERR82 = 82,
    ERR83 = 83,
    ERR84 = 84,
    ERR85 = 85,
    ERR86 = 86,
    ERR87 = 87,
    ERR88 = 88,
    ERR89 = 89,
    ERR90 = 90,
    ERR91 = 91,
    ERR92 = 92,
    ERR93 = 93,
    ERR94 = 94,
    ERR95 = 95,
    ERR96 = 96,
    ERR97 = 97,
    ERR98 = 98,
    ERR99 = 99,
    ERR100 = 100,
    ERR101 = 101,
);

pub const REQ_UNSET: u32 = 0xffffffff;
pub const REQ_NONE: u32 = 0xfffffffe;
pub const REQ_CASELESS: u32 = 0x00000001;
pub const REQ_VARY: u32 = 0x00000002;

pub const GI_SET_FIXED_LENGTH: u32 = 0x80000000;
pub const GI_NOT_FIXED_LENGTH: u32 = 0x40000000;
pub const GI_FIXED_LENGTH_MASK: u32 = 0x0000ffff;

pub const MAX_GROUP_NUMBER: u32 = 65535;
pub const MAX_REPEAT_COUNT: u32 = 65535;
pub const REPEAT_UNLIMITED: u32 = MAX_REPEAT_COUNT + 1;
pub const COMPILE_WORK_SIZE: usize = 3000 * LINK_SIZE;
pub const GROUPINFO_DEFAULT_SIZE: usize = 256;
pub const WORK_SIZE_SAFETY_MARGIN: usize = 100;
pub const PARSED_PATTERN_DEFAULT_SIZE: usize = 1024;
pub const OFLOW_MAX: usize = (i32::MAX - 20) as usize;

pub const TABLES_LENGTH: usize = 1088;
pub const CTYPES_OFFSET: usize = 512 + 320;

/// A Unicode character database record (pcre2_internal.h ucd_record).
#[derive(Clone, Copy, Debug)]
pub struct UcdRecord {
    pub script: u8,
    pub chartype: u8,
    pub gbprop: u8,
    pub caseset: u8,
    pub other_case: i32,
    pub scriptx_bidiclass: u16,
    pub bprops: u16,
}

pub const UCD_BLOCK_SIZE: usize = 128;

#[inline]
pub fn get_ucd(ch: u32) -> &'static UcdRecord {
    let ch = ch as usize;
    if ch > 0x10ffff {
        // Only reachable for invalid input in 8-bit mode; mirror the
        // 32-bit library's dummy record (unassigned code point).
        return &DUMMY_UCD_RECORD;
    }
    let s1 = tables::UCD_STAGE1[ch / UCD_BLOCK_SIZE] as usize;
    &tables::UCD_RECORDS[tables::UCD_STAGE2[s1 * UCD_BLOCK_SIZE + ch % UCD_BLOCK_SIZE] as usize]
}

static DUMMY_UCD_RECORD: UcdRecord = UcdRecord {
    script: ucp_Unknown as u8,
    chartype: ucp_Cn as u8,
    gbprop: ucp_gbOther as u8,
    caseset: 0,
    other_case: 0,
    scriptx_bidiclass: ucp_Unknown as u16,
    bprops: 0,
};

#[inline]
pub fn ucd_chartype(ch: u32) -> u32 {
    get_ucd(ch).chartype as u32
}
#[inline]
pub fn ucd_script(ch: u32) -> u32 {
    get_ucd(ch).script as u32
}
#[inline]
pub fn ucd_category(ch: u32) -> u32 {
    tables::UCP_GENTYPE[ucd_chartype(ch) as usize]
}
#[inline]
pub fn ucd_graphbreak(ch: u32) -> u32 {
    get_ucd(ch).gbprop as u32
}
#[inline]
pub fn ucd_caseset(ch: u32) -> u32 {
    get_ucd(ch).caseset as u32
}
#[inline]
pub fn ucd_othercase(ch: u32) -> u32 {
    (ch as i32).wrapping_add(get_ucd(ch).other_case) as u32
}
#[inline]
pub fn ucd_scriptx_prop(p: &UcdRecord) -> u32 {
    (p.scriptx_bidiclass as u32) & UCD_SCRIPTX_MASK
}
#[inline]
pub fn ucd_bidiclass_prop(p: &UcdRecord) -> u32 {
    (p.scriptx_bidiclass as u32) >> UCD_BIDICLASS_SHIFT
}
#[inline]
pub fn ucd_bprops_prop(p: &UcdRecord) -> u32 {
    (p.bprops as u32) & UCD_BPROPS_MASK
}
#[inline]
pub fn mapbit(map: &[u32], n: u32) -> bool {
    map[(n / 32) as usize] & (1u32 << (n % 32)) != 0
}

/// HSPACE_CASES / VSPACE_CASES helpers.
#[inline]
pub fn is_hspace(c: u32) -> bool {
    matches!(
        c,
        0x09 | 0x20 | 0xa0 | 0x1680 | 0x180e | 0x2000..=0x200a | 0x202f | 0x205f | 0x3000
    )
}
#[inline]
pub fn is_vspace(c: u32) -> bool {
    matches!(c, 0x0a..=0x0d | 0x85 | 0x2028 | 0x2029)
}
