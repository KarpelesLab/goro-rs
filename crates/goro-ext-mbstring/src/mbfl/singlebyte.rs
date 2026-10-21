//! Single-byte encodings (C: mbfilter_singlebyte.c)

use super::tables::filt_singlebyte::*;
use super::*;

#[inline]
fn coalesce(a: u32, b: u32) -> u32 {
    if a != 0 { a } else { b }
}

macro_rules! def_sb_tbl {
    ($enc:ident, $to:ident, $from:ident, $no:expr, $name:expr, $mime:expr, $aliases:expr, $tbl_min:expr, $tbl:expr) => {
        fn $to(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
            for &c in input {
                out.push(if (c as u32) < $tbl_min { c as u32 } else { coalesce($tbl[c as usize - $tbl_min as usize] as u32, MBFL_BAD_INPUT) });
            }
        }
        fn $from(input: &[u32], buf: &mut ConvBuf, _end: bool) {
            'outer: for &w in input {
                if w < $tbl_min {
                    buf.add((w & 0xFF) as u8);
                } else {
                    for i in 0..(256 - $tbl_min as usize) {
                        if w == $tbl[i] as u32 {
                            buf.add((i + $tbl_min as usize) as u8);
                            continue 'outer;
                        }
                    }
                    mb_illegal_output(w, $from, buf);
                }
            }
        }
        pub static $enc: Encoding = Encoding {
            no: $no,
            name: $name,
            mime_name: Some($mime),
            aliases: $aliases,
            mblen_table: None,
            flag: MBFL_ENCTYPE_SBCS,
            to_wchar: Some($to),
            from_wchar: Some($from),
            check: None,
            cut: None,
        };
    };
}

/* ASCII */

fn mb_ascii_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    out.extend(input.iter().map(|&c| if c < 0x80 { c as u32 } else { MBFL_BAD_INPUT }));
}

fn mb_wchar_to_ascii(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < 0x80 {
            buf.add((w & 0xFF) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_ascii, buf);
        }
    }
}

pub static ENC_ASCII: Encoding = Encoding {
    no: EncNo::Ascii,
    name: "ASCII",
    mime_name: Some("US-ASCII"),
    aliases: &[
        "ANSI_X3.4-1968",
        "iso-ir-6",
        "ANSI_X3.4-1986",
        "ISO_646.irv:1991",
        "US-ASCII",
        "ISO646-US",
        "us",
        "IBM367",
        "IBM-367",
        "cp367",
        "csASCII",
    ],
    mblen_table: None,
    flag: MBFL_ENCTYPE_SBCS,
    to_wchar: Some(mb_ascii_to_wchar),
    from_wchar: Some(mb_wchar_to_ascii),
    check: None,
    cut: None,
};

/* ISO-8859-1 */

fn mb_8859_1_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    out.extend(input.iter().map(|&c| c as u32));
}

fn mb_wchar_to_8859_1(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < 0x100 {
            buf.add(w as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_8859_1, buf);
        }
    }
}

pub static ENC_8859_1: Encoding = Encoding {
    no: EncNo::Iso8859_1,
    name: "ISO-8859-1",
    mime_name: Some("ISO-8859-1"),
    aliases: &["ISO8859-1", "latin1"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_SBCS,
    to_wchar: Some(mb_8859_1_to_wchar),
    from_wchar: Some(mb_wchar_to_8859_1),
    check: None,
    cut: None,
};

def_sb_tbl!(ENC_8859_2, mb_8859_2_to_wchar, mb_wchar_to_8859_2, EncNo::Iso8859_2, "ISO-8859-2", "ISO-8859-2", &["ISO8859-2", "latin2"], 0xA0u32, iso8859_2_ucs_table);
def_sb_tbl!(ENC_8859_3, mb_8859_3_to_wchar, mb_wchar_to_8859_3, EncNo::Iso8859_3, "ISO-8859-3", "ISO-8859-3", &["ISO8859-3", "latin3"], 0xA0u32, iso8859_3_ucs_table);
def_sb_tbl!(ENC_8859_4, mb_8859_4_to_wchar, mb_wchar_to_8859_4, EncNo::Iso8859_4, "ISO-8859-4", "ISO-8859-4", &["ISO8859-4", "latin4"], 0xA0u32, iso8859_4_ucs_table);
def_sb_tbl!(ENC_8859_5, mb_8859_5_to_wchar, mb_wchar_to_8859_5, EncNo::Iso8859_5, "ISO-8859-5", "ISO-8859-5", &["ISO8859-5", "cyrillic"], 0xA0u32, iso8859_5_ucs_table);
def_sb_tbl!(ENC_8859_6, mb_8859_6_to_wchar, mb_wchar_to_8859_6, EncNo::Iso8859_6, "ISO-8859-6", "ISO-8859-6", &["ISO8859-6", "arabic"], 0xA0u32, iso8859_6_ucs_table);
def_sb_tbl!(ENC_8859_7, mb_8859_7_to_wchar, mb_wchar_to_8859_7, EncNo::Iso8859_7, "ISO-8859-7", "ISO-8859-7", &["ISO8859-7", "greek"], 0xA0u32, iso8859_7_ucs_table);
def_sb_tbl!(ENC_8859_8, mb_8859_8_to_wchar, mb_wchar_to_8859_8, EncNo::Iso8859_8, "ISO-8859-8", "ISO-8859-8", &["ISO8859-8", "hebrew"], 0xA0u32, iso8859_8_ucs_table);
def_sb_tbl!(ENC_8859_9, mb_8859_9_to_wchar, mb_wchar_to_8859_9, EncNo::Iso8859_9, "ISO-8859-9", "ISO-8859-9", &["ISO8859-9", "latin5"], 0xA0u32, iso8859_9_ucs_table);
def_sb_tbl!(ENC_8859_10, mb_8859_10_to_wchar, mb_wchar_to_8859_10, EncNo::Iso8859_10, "ISO-8859-10", "ISO-8859-10", &["ISO8859-10", "latin6"], 0xA0u32, iso8859_10_ucs_table);
def_sb_tbl!(ENC_8859_13, mb_8859_13_to_wchar, mb_wchar_to_8859_13, EncNo::Iso8859_13, "ISO-8859-13", "ISO-8859-13", &["ISO8859-13"], 0xA0u32, iso8859_13_ucs_table);
def_sb_tbl!(ENC_8859_14, mb_8859_14_to_wchar, mb_wchar_to_8859_14, EncNo::Iso8859_14, "ISO-8859-14", "ISO-8859-14", &["ISO8859-14", "latin8"], 0xA0u32, iso8859_14_ucs_table);
def_sb_tbl!(ENC_8859_15, mb_8859_15_to_wchar, mb_wchar_to_8859_15, EncNo::Iso8859_15, "ISO-8859-15", "ISO-8859-15", &["ISO8859-15"], 0xA0u32, iso8859_15_ucs_table);
def_sb_tbl!(ENC_8859_16, mb_8859_16_to_wchar, mb_wchar_to_8859_16, EncNo::Iso8859_16, "ISO-8859-16", "ISO-8859-16", &["ISO8859-16"], 0xA0u32, iso8859_16_ucs_table);
def_sb_tbl!(ENC_CP1251, mb_cp1251_to_wchar, mb_wchar_to_cp1251, EncNo::Cp1251, "Windows-1251", "Windows-1251", &["CP1251", "CP-1251", "WINDOWS-1251"], 0x80u32, cp1251_ucs_table);
def_sb_tbl!(ENC_CP1254, mb_cp1254_to_wchar, mb_wchar_to_cp1254, EncNo::Cp1254, "Windows-1254", "Windows-1254", &["CP1254", "CP-1254", "WINDOWS-1254"], 0x80u32, cp1254_ucs_table);
def_sb_tbl!(ENC_CP866, mb_cp866_to_wchar, mb_wchar_to_cp866, EncNo::Cp866, "CP866", "CP866", &["CP-866", "IBM866", "IBM-866"], 0x80u32, cp866_ucs_table);
def_sb_tbl!(ENC_CP850, mb_cp850_to_wchar, mb_wchar_to_cp850, EncNo::Cp850, "CP850", "CP850", &["CP-850", "IBM850", "IBM-850"], 0x80u32, cp850_ucs_table);
def_sb_tbl!(ENC_KOI8R, mb_koi8r_to_wchar, mb_wchar_to_koi8r, EncNo::Koi8r, "KOI8-R", "KOI8-R", &["KOI8R"], 0x80u32, koi8r_ucs_table);
def_sb_tbl!(ENC_KOI8U, mb_koi8u_to_wchar, mb_wchar_to_koi8u, EncNo::Koi8u, "KOI8-U", "KOI8-U", &["KOI8U"], 0x80u32, koi8u_ucs_table);

/* CP1252 */

fn mb_cp1252_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    for &c in input {
        if (0x80..0xA0).contains(&c) {
            out.push(coalesce(cp1252_ucs_table[(c - 0x80) as usize] as u32, MBFL_BAD_INPUT));
        } else {
            out.push(c as u32);
        }
    }
}

fn mb_wchar_to_cp1252(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    'outer: for &w in input {
        if w >= 0x100 {
            for i in 0..32 {
                if w == cp1252_ucs_table[i] as u32 {
                    buf.add((i + 0x80) as u8);
                    continue 'outer;
                }
            }
            mb_illegal_output(w, mb_wchar_to_cp1252, buf);
        } else if w <= 0x7F || w >= 0xA0 || w == 0x81 || w == 0x8D || w == 0x8F || w == 0x90 || w == 0x9D {
            buf.add(w as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_cp1252, buf);
        }
    }
}

pub static ENC_CP1252: Encoding = Encoding {
    no: EncNo::Cp1252,
    name: "Windows-1252",
    mime_name: Some("Windows-1252"),
    aliases: &["cp1252"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_SBCS,
    to_wchar: Some(mb_cp1252_to_wchar),
    from_wchar: Some(mb_wchar_to_cp1252),
    check: None,
    cut: None,
};

/* ArmSCII-8 */

fn mb_armscii8_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    for &c in input {
        out.push(if c < 0xA0 { c as u32 } else { coalesce(armscii8_ucs_table[(c - 0xA0) as usize] as u32, MBFL_BAD_INPUT) });
    }
}

fn mb_wchar_to_armscii8(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    'outer: for &w in input {
        if (0x28..=0x2F).contains(&w) {
            buf.add(ucs_armscii8_table[(w - 0x28) as usize]);
        } else if w < 0xA0 {
            buf.add(w as u8);
        } else {
            for i in 0..0x60 {
                if w == armscii8_ucs_table[i] as u32 {
                    buf.add((0xA0 + i) as u8);
                    continue 'outer;
                }
            }
            mb_illegal_output(w, mb_wchar_to_armscii8, buf);
        }
    }
}

pub static ENC_ARMSCII8: Encoding = Encoding {
    no: EncNo::Armscii8,
    name: "ArmSCII-8",
    mime_name: Some("ArmSCII-8"),
    aliases: &["ArmSCII8", "ARMSCII-8", "ARMSCII8"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_SBCS,
    to_wchar: Some(mb_armscii8_to_wchar),
    from_wchar: Some(mb_wchar_to_armscii8),
    check: None,
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[
    &ENC_ASCII,
    &ENC_8859_1,
    &ENC_8859_2,
    &ENC_8859_3,
    &ENC_8859_4,
    &ENC_8859_5,
    &ENC_8859_6,
    &ENC_8859_7,
    &ENC_8859_8,
    &ENC_8859_9,
    &ENC_8859_10,
    &ENC_8859_13,
    &ENC_8859_14,
    &ENC_8859_15,
    &ENC_8859_16,
    &ENC_CP1251,
    &ENC_CP1252,
    &ENC_CP1254,
    &ENC_CP866,
    &ENC_CP850,
    &ENC_KOI8R,
    &ENC_KOI8U,
    &ENC_ARMSCII8,
];
