//! UTF-8-Mobile#DOCOMO / #KDDI-A / #KDDI-B / #SOFTBANK (C: mbfilter_utf8.c)

use super::basic::push_utf8;
use super::common::*;
use super::mobile::*;
use super::tables::filt_utf8::{
    mbfl_docomo2uni_pua, mbfl_kddi2uni_pua, mbfl_kddi2uni_pua_b, mbfl_sb2uni_pua, mblen_table_utf8,
};
use super::*;

fn conv_map_tbl(c: u32, n: usize, map: &[[u16; 3]]) -> Option<u32> {
    for m in &map[..n] {
        if m[0] as u32 <= c && c <= m[1] as u32 {
            return Some(c - m[0] as u32 + m[2] as u32);
        }
    }
    None
}

fn conv_r_map_tbl(c: u32, n: usize, map: &[[u16; 3]]) -> Option<u32> {
    /* Convert in reverse direction */
    for m in &map[..n] {
        let lo = m[2] as i64;
        let hi = m[2] as i64 - m[0] as i64 + m[1] as i64;
        if lo <= c as i64 && c as i64 <= hi {
            return Some((c as i64 + m[0] as i64 - m[2] as i64) as u32);
        }
    }
    None
}

fn mobile_utf8_to_wchar(
    input: &[u8],
    out: &mut Vec<u32>,
    emoji_map: &[[u16; 3]],
    convert_emoji: fn(i32, &mut i32) -> i32,
    n: usize,
) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        let s: u32;
        if c <= 0x7F {
            out.push(c as u32);
            continue;
        } else if (0xC2..=0xDF).contains(&c) && p < e {
            let c2 = input[p];
            p += 1;
            if (c2 & 0xC0) == 0x80 {
                s = (((c & 0x1F) as u32) << 6) | (c2 & 0x3F) as u32;
            } else {
                out.push(MBFL_BAD_INPUT);
                p -= 1;
                continue;
            }
        } else if (0xE0..=0xEF).contains(&c) {
            if e - p < 2 {
                out.push(MBFL_BAD_INPUT);
                if p < e
                    && (c != 0xE0 || input[p] >= 0xA0)
                    && (c != 0xED || input[p] < 0xA0)
                    && (input[p] & 0xC0) == 0x80
                {
                    p += 1;
                    if p < e && (input[p] & 0xC0) == 0x80 {
                        p += 1;
                    }
                }
                continue;
            }
            let c2 = input[p];
            let c3 = input[p + 1];
            p += 2;
            if (c2 & 0xC0) != 0x80 || (c == 0xE0 && c2 < 0xA0) || (c == 0xED && c2 >= 0xA0) {
                out.push(MBFL_BAD_INPUT);
                p -= 2;
                continue;
            } else if (c3 & 0xC0) != 0x80 {
                out.push(MBFL_BAD_INPUT);
                p -= 1;
                continue;
            } else {
                s = (((c & 0xF) as u32) << 12) | (((c2 & 0x3F) as u32) << 6) | (c3 & 0x3F) as u32;
            }
        } else if (0xF0..=0xF4).contains(&c) {
            if e - p < 3 {
                out.push(MBFL_BAD_INPUT);
                if p < e {
                    let c2 = input[p];
                    if (c == 0xF0 && c2 >= 0x90)
                        || (c == 0xF4 && c2 < 0x90)
                        || (0xF1..=0xF3).contains(&c)
                    {
                        while p < e && (input[p] & 0xC0) == 0x80 {
                            p += 1;
                        }
                    }
                }
                continue;
            }
            let c2 = input[p];
            let c3 = input[p + 1];
            let c4 = input[p + 2];
            p += 3;
            if (c2 & 0xC0) != 0x80 || (c == 0xF0 && c2 < 0x90) || (c == 0xF4 && c2 >= 0x90) {
                out.push(MBFL_BAD_INPUT);
                p -= 3;
                continue;
            } else if (c3 & 0xC0) != 0x80 {
                out.push(MBFL_BAD_INPUT);
                p -= 2;
                continue;
            } else if (c4 & 0xC0) != 0x80 {
                out.push(MBFL_BAD_INPUT);
                p -= 1;
                continue;
            } else {
                s = (((c & 0x7) as u32) << 18)
                    | (((c2 & 0x3F) as u32) << 12)
                    | (((c3 & 0x3F) as u32) << 6)
                    | (c4 & 0x3F) as u32;
            }
        } else {
            out.push(MBFL_BAD_INPUT);
            continue;
        }

        let mut s = s;
        if let Some(s1) = conv_r_map_tbl(s, n, emoji_map) {
            let mut snd = 0;
            s = convert_emoji(s1 as i32, &mut snd) as u32;
            if snd != 0 {
                out.push(snd as u32);
            }
        }
        out.push(s);
    }
}

fn mb_utf8_docomo_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    mobile_utf8_to_wchar(
        input,
        out,
        &mbfl_docomo2uni_pua,
        mbfilter_sjis_emoji_docomo2unicode,
        4,
    )
}
fn mb_utf8_kddi_a_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    mobile_utf8_to_wchar(
        input,
        out,
        &mbfl_kddi2uni_pua,
        mbfilter_sjis_emoji_kddi2unicode,
        7,
    )
}
fn mb_utf8_kddi_b_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    mobile_utf8_to_wchar(
        input,
        out,
        &mbfl_kddi2uni_pua_b,
        mbfilter_sjis_emoji_kddi2unicode,
        8,
    )
}
fn mb_utf8_sb_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    mobile_utf8_to_wchar(
        input,
        out,
        &mbfl_sb2uni_pua,
        mbfilter_sjis_emoji_sb2unicode,
        6,
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Vendor {
    Docomo,
    Kddi,
    Sb,
}

fn wchar_to_utf8_mobile(
    vendor: Vendor,
    input: &[u32],
    buf: &mut ConvBuf,
    emoji_map: &[[u16; 3]],
    n: usize,
    error_handler: FromWcharFn,
) {
    let len = input.len();
    let mut i = 0usize;
    let tables = match vendor {
        Vendor::Docomo => docomo_tables(),
        Vendor::Kddi => kddi_tables(),
        Vendor::Sb => sb_tables(),
    };
    let (keypad_hash, keypad0, keypad1, copyright, registered) = match vendor {
        Vendor::Docomo => (0x2964u32, 0x296F, 0x2966, 0x29B5, 0x29BA),
        Vendor::Kddi => (0x25BC, 0x2830, 0x27A6, 0x27DC, 0x27DD),
        Vendor::Sb => (0x2817, 0x282C, 0x2823, 0x2855, 0x2856),
    };
    'main: while i < len {
        let mut w = input[i];
        i += 1;
        let mut s: u32 = 0;
        if w < 0x110000 {
            'kuten: {
                if (w == '#' as u32 || (w >= '0' as u32 && w <= '9' as u32)) && i < len {
                    let w2 = input[i];
                    i += 1;
                    if w2 == 0x20E3 {
                        s = if w == '#' as u32 {
                            keypad_hash
                        } else if w == '0' as u32 {
                            keypad0
                        } else {
                            keypad1 + (w - '1' as u32)
                        };
                    } else {
                        i -= 1;
                    }
                } else if vendor != Vendor::Docomo && w >= nflags(b'C') && w <= nflags(b'U') {
                    if i < len {
                        let w2 = input[i];
                        i += 1;
                        if w2 >= nflags(b'B') && w2 <= nflags(b'U') {
                            for k in 0..10 {
                                if w == nflags(nflags_s[k][0]) && w2 == nflags(nflags_s[k][1]) {
                                    s = if vendor == Vendor::Kddi {
                                        nflags_code_kddi[k]
                                    } else {
                                        nflags_code_sb[k]
                                    } as u32;
                                    break 'kuten;
                                }
                            }
                        }
                        i -= 1;
                    }
                    mb_illegal_output(w, error_handler, buf);
                    continue 'main;
                } else if w == 0xA9 {
                    s = copyright;
                } else if w == 0xAE {
                    s = registered;
                } else {
                    s = emoji_lookup3(w, &tables);
                }
            }

            if s != 0 {
                if let Some(c1) = conv_map_tbl(s, n, emoji_map) {
                    w = c1;
                }
            }
            push_utf8(&mut buf.out, w);
        } else {
            mb_illegal_output(w, error_handler, buf);
        }
    }
}

fn mb_wchar_to_utf8_docomo(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    wchar_to_utf8_mobile(
        Vendor::Docomo,
        input,
        buf,
        &mbfl_docomo2uni_pua,
        4,
        mb_wchar_to_utf8_docomo,
    )
}
fn mb_wchar_to_utf8_kddi_a(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    wchar_to_utf8_mobile(
        Vendor::Kddi,
        input,
        buf,
        &mbfl_kddi2uni_pua,
        7,
        mb_wchar_to_utf8_kddi_a,
    )
}
fn mb_wchar_to_utf8_kddi_b(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    wchar_to_utf8_mobile(
        Vendor::Kddi,
        input,
        buf,
        &mbfl_kddi2uni_pua_b,
        8,
        mb_wchar_to_utf8_kddi_b,
    )
}
fn mb_wchar_to_utf8_sb(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    wchar_to_utf8_mobile(
        Vendor::Sb,
        input,
        buf,
        &mbfl_sb2uni_pua,
        6,
        mb_wchar_to_utf8_sb,
    )
}

fn mb_cut_utf8(s: &[u8], from: usize, len: usize) -> Vec<u8> {
    let end = s.len();
    let mut start = from;
    while start > 0 && start < end && (s[start] as i8) < -64 {
        start -= 1;
    }
    let mut e = start + len;
    if e >= end {
        return s[start.min(end)..end].to_vec();
    }
    while e > start && (s[e] as i8) < -64 {
        e -= 1;
    }
    s[start..e].to_vec()
}

pub static ENC_UTF8_DOCOMO: Encoding = Encoding {
    no: EncNo::Utf8Docomo,
    name: "UTF-8-Mobile#DOCOMO",
    mime_name: Some("UTF-8"),
    aliases: &["UTF-8-DOCOMO", "UTF8-DOCOMO"],
    mblen_table: Some(&mblen_table_utf8),
    flag: 0,
    to_wchar: Some(mb_utf8_docomo_to_wchar),
    from_wchar: Some(mb_wchar_to_utf8_docomo),
    check: None,
    cut: Some(mb_cut_utf8),
};

pub static ENC_UTF8_KDDI_A: Encoding = Encoding {
    no: EncNo::Utf8KddiA,
    name: "UTF-8-Mobile#KDDI-A",
    mime_name: Some("UTF-8"),
    aliases: &[],
    mblen_table: Some(&mblen_table_utf8),
    flag: 0,
    to_wchar: Some(mb_utf8_kddi_a_to_wchar),
    from_wchar: Some(mb_wchar_to_utf8_kddi_a),
    check: None,
    cut: Some(mb_cut_utf8),
};

pub static ENC_UTF8_KDDI_B: Encoding = Encoding {
    no: EncNo::Utf8KddiB,
    name: "UTF-8-Mobile#KDDI-B",
    mime_name: Some("UTF-8"),
    aliases: &["UTF-8-Mobile#KDDI", "UTF-8-KDDI", "UTF8-KDDI"],
    mblen_table: Some(&mblen_table_utf8),
    flag: 0,
    to_wchar: Some(mb_utf8_kddi_b_to_wchar),
    from_wchar: Some(mb_wchar_to_utf8_kddi_b),
    check: None,
    cut: Some(mb_cut_utf8),
};

pub static ENC_UTF8_SB: Encoding = Encoding {
    no: EncNo::Utf8Sb,
    name: "UTF-8-Mobile#SOFTBANK",
    mime_name: Some("UTF-8"),
    aliases: &["UTF-8-SOFTBANK", "UTF8-SOFTBANK"],
    mblen_table: Some(&mblen_table_utf8),
    flag: 0,
    to_wchar: Some(mb_utf8_sb_to_wchar),
    from_wchar: Some(mb_wchar_to_utf8_sb),
    check: None,
    cut: Some(mb_cut_utf8),
};

pub static ENCODINGS: &[&Encoding] = &[
    &ENC_UTF8_DOCOMO,
    &ENC_UTF8_KDDI_A,
    &ENC_UTF8_KDDI_B,
    &ENC_UTF8_SB,
];
