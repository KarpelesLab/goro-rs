//! libmbfl's legacy conversion filters (mbfl_convert_filter), which
//! mbfl_strcut() still runs for stateful encodings: a byte-at-a-time
//! encoding -> wchar filter feeding a wchar -> encoding filter, whose state
//! is copied and restored while looking for the longest output that fits.
//!
//! Ported from libmbfl/mbfl/mbfilter.c (mbfl_strcut), mbfl_convert.c
//! (mbfl_filt_conv_illegal_output) and the legacy filter functions of
//! filters/mbfilter_cjk.c (JIS, ISO-2022-JP, CP50220, CP50221) and
//! filters/mbfilter_utf7imap.c. They differ from the newer fast converters
//! in edge cases (GR kana in JIS, unmappable characters dropped by the
//! ISO-2022-JP encoder, CP50220's NUL handling, ...), which mb_strcut()
//! shows.

use super::common::{mbfl_bisec_srch, mbfl_bisec_srch2};
use super::tables::translit_kana_jisx0201_jisx0208::*;
use super::tables::unicode_table_cp932_ext::*;
use super::tables::unicode_table_cp936::*;
use super::tables::unicode_table_gb2312::*;
use super::tables::unicode_table_jis::*;
use super::tables::unicode_table_jis2004::*;
use super::tables::unicode_table_uhc::*;
use super::{EncNo, MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR, MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE};

const MBFL_BAD_INPUT: i32 = -1;
const MBFL_WCSPLANE_UCS2MAX: i32 = 0x0001_0000;
const MBFL_WCSPLANE_SUPMIN: i32 = 0x0001_0000;
const MBFL_WCSPLANE_SUPMAX: i32 = 0x0020_0000;

/// The encoding -> wchar filters
#[derive(Clone, Copy, PartialEq)]
enum ToWchar {
    Jis,
    Cp5022x,
    Utf7,
    Utf7Imap,
    Iso2022jpms,
    Iso2022kr,
    Hz,
    Iso2022jp2004,
    Iso2022jpKddi,
    Base64,
    Qprint,
    HtmlEnt,
}

/// The wchar -> encoding filters
#[derive(Clone, Copy, PartialEq)]
enum FromWchar {
    Jis,
    Iso2022jp,
    Cp50220,
    Cp50221,
    Cp50222,
    Utf7,
    Utf7Imap,
    Iso2022jpms,
    Iso2022kr,
    Hz,
    Iso2022jp2004,
    Iso2022jpKddi,
    Base64,
    Qprint,
    HtmlEnt,
}

/// The state of one mbfl_convert_filter.
#[derive(Clone)]
struct Filter {
    status: i32,
    cache: i32,
    illegal_mode: u8,
    illegal_substchar: i32,
    /// `opaque` of the HTML-ENTITIES decoder
    buffer: [u8; 17],
}

impl Filter {
    fn new() -> Filter {
        // mbfl_convert_filter_init() + mbfl_filt_conv_common_ctor()
        Filter {
            status: 0,
            cache: 0,
            illegal_mode: MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR,
            illegal_substchar: '?' as i32,
            buffer: [0; 17],
        }
    }
}

/// The legacy filters of an encoding, if mbfl_strcut() runs them here.
fn filters_for(no: EncNo) -> Option<(ToWchar, FromWchar)> {
    Some(match no {
        EncNo::Jis => (ToWchar::Jis, FromWchar::Jis),
        EncNo::Iso2022jp => (ToWchar::Jis, FromWchar::Iso2022jp),
        EncNo::Cp50220 => (ToWchar::Cp5022x, FromWchar::Cp50220),
        EncNo::Cp50221 => (ToWchar::Cp5022x, FromWchar::Cp50221),
        EncNo::Cp50222 => (ToWchar::Cp5022x, FromWchar::Cp50222),
        EncNo::Utf7 => (ToWchar::Utf7, FromWchar::Utf7),
        EncNo::Utf7imap => (ToWchar::Utf7Imap, FromWchar::Utf7Imap),
        EncNo::Iso2022jpms => (ToWchar::Iso2022jpms, FromWchar::Iso2022jpms),
        EncNo::Iso2022kr => (ToWchar::Iso2022kr, FromWchar::Iso2022kr),
        EncNo::Hz => (ToWchar::Hz, FromWchar::Hz),
        EncNo::Iso2022jp2004 => (ToWchar::Iso2022jp2004, FromWchar::Iso2022jp2004),
        EncNo::Iso2022jpKddi => (ToWchar::Iso2022jpKddi, FromWchar::Iso2022jpKddi),
        EncNo::Base64 => (ToWchar::Base64, FromWchar::Base64),
        EncNo::Qprint => (ToWchar::Qprint, FromWchar::Qprint),
        EncNo::HtmlEnt => (ToWchar::HtmlEnt, FromWchar::HtmlEnt),
        _ => return None,
    })
}

/* ===== encoding -> wchar ===== */

/// mbfl_filt_conv_jis_wchar / mbfl_filt_conv_cp5022x_wchar
fn jis_wchar(c: i32, f: &mut Filter, cp5022x: bool, out: &mut Vec<i32>) {
    loop {
        match f.status & 0xf {
            0 => {
                if c == 0x1b {
                    f.status += 2;
                } else if c == 0x0e {
                    // "kana in"
                    f.status = 0x20;
                } else if c == 0x0f {
                    // "kana out"
                    f.status = 0;
                } else if f.status == 0x10 && c == 0x5c {
                    out.push(0xa5);
                } else if f.status == 0x10 && c == 0x7e {
                    out.push(0x203e);
                } else if f.status == 0x20 && c > 0x20 && c < 0x60 {
                    out.push(0xff40 + c);
                } else if (f.status == 0x80 || f.status == 0x90)
                    && c > 0x20
                    && if cp5022x { c <= 0x97 } else { c < 0x7f }
                {
                    // kanji first char
                    f.cache = c;
                    f.status += 1;
                } else if (0..0x80).contains(&c) {
                    out.push(c);
                } else if c > 0xa0 && c < 0xe0 {
                    // GR kana
                    out.push(0xfec0 + c);
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            }
            1 => {
                f.status &= !0xf;
                let c1 = f.cache;
                if c > 0x20 && c < 0x7f {
                    let s = (c1 - 0x21) * 94 + c - 0x21;
                    let mut w;
                    if f.status == 0x80 {
                        if cp5022x {
                            w = if s >= cp932ext1_ucs_table_min && s < cp932ext1_ucs_table_max {
                                cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min) as usize] as i32
                            } else if s >= 0 && s < jisx0208_ucs_table_size {
                                jisx0208_ucs_table[s as usize] as i32
                            } else if s >= cp932ext2_ucs_table_min && s < cp932ext2_ucs_table_max {
                                cp932ext2_ucs_table[(s - cp932ext2_ucs_table_min) as usize] as i32
                            } else if s >= cp932ext3_ucs_table_min && s < cp932ext3_ucs_table_max {
                                cp932ext3_ucs_table[(s - cp932ext3_ucs_table_min) as usize] as i32
                            } else if (94 * 94..114 * 94).contains(&s) {
                                // user-defined => PUA (Microsoft extended)
                                s - 94 * 94 + 0xe000
                            } else {
                                0
                            };
                        } else {
                            w = if s >= 0 && s < jisx0208_ucs_table_size {
                                jisx0208_ucs_table[s as usize] as i32
                            } else {
                                0
                            };
                        }
                    } else {
                        w = if s >= 0 && s < jisx0212_ucs_table_size {
                            jisx0212_ucs_table[s as usize] as i32
                        } else {
                            0
                        };
                    }
                    if w <= 0 {
                        w = MBFL_BAD_INPUT;
                    }
                    out.push(w);
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            }
            2 => {
                // ESC
                if c == 0x24 {
                    f.status += 1;
                } else if c == 0x28 {
                    f.status += 3;
                } else {
                    f.status &= !0xf;
                    out.push(MBFL_BAD_INPUT);
                    continue;
                }
            }
            3 => {
                // ESC $
                if c == 0x40 || c == 0x42 {
                    f.status = 0x80;
                } else if c == 0x28 {
                    f.status += 1;
                } else {
                    f.status &= !0xf;
                    out.push(MBFL_BAD_INPUT);
                    out.push(0x24);
                    continue;
                }
            }
            4 => {
                // ESC $ (
                if c == 0x40 || c == 0x42 {
                    f.status = 0x80;
                } else if c == 0x44 {
                    f.status = 0x90;
                } else {
                    f.status &= !0xf;
                    out.push(MBFL_BAD_INPUT);
                    out.push(0x24);
                    out.push(0x28);
                    continue;
                }
            }
            5 => {
                // ESC (
                if c == 0x42 || c == 0x48 {
                    f.status = 0;
                } else if c == 0x4a {
                    f.status = 0x10;
                } else if c == 0x49 {
                    f.status = 0x20;
                } else {
                    f.status &= !0xf;
                    out.push(MBFL_BAD_INPUT);
                    out.push(0x28);
                    continue;
                }
            }
            _ => {}
        }
        return;
    }
}

/// mbfl_filt_conv_utf7imap_wchar
fn utf7imap_wchar(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    let mut n: i32 = -1;
    if f.status != 0 {
        // Modified Base64
        if (b'A' as i32..=b'Z' as i32).contains(&c) {
            n = c - 65;
        } else if (b'a' as i32..=b'z' as i32).contains(&c) {
            n = c - 71;
        } else if (b'0' as i32..=b'9' as i32).contains(&c) {
            n = c + 4;
        } else if c == b'+' as i32 {
            n = 62;
        } else if c == b',' as i32 {
            n = 63;
        }
        if !(0..=63).contains(&n) {
            if c == b'-' as i32 {
                if f.status == 1 {
                    // "&-" -> "&"
                    f.cache = 0;
                    f.status = 0;
                    out.push(b'&' as i32);
                } else if f.cache != 0 {
                    // ended abruptly, with partially encoded characters
                    f.cache = 0;
                    f.status = 0;
                    out.push(MBFL_BAD_INPUT);
                } else {
                    f.cache = 0;
                    f.status = 0;
                }
            } else {
                f.cache = 0;
                f.status = 0;
                out.push(MBFL_BAD_INPUT);
            }
            return;
        }
    }
    // A decoded UTF-16 unit `s` (`next` the bits of the following one)
    let unit = |f: &mut Filter, s: i32, next: i32, out: &mut Vec<i32>| {
        if (0xd800..0xdc00).contains(&s) {
            // 1st part of surrogate pair
            f.cache = (((s & 0x3ff) << 16) + 0x400000) | next;
        } else if (0xdc00..0xe000).contains(&s) {
            // 2nd part of surrogate pair
            if f.cache & 0xfff0000 != 0 {
                let s = (s & 0x3ff) | ((f.cache & 0xfff0000) >> 6);
                f.cache = next;
                out.push(s);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else {
            f.cache = next;
            // literal ASCII must not be Base64-encoded
            if !(0x20..=0x7e).contains(&s) || s == b'&' as i32 {
                out.push(s);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
    };
    match f.status {
        0 => {
            if c == b'&' as i32 {
                f.status += 1;
            } else if (0x20..=0x7e).contains(&c) {
                out.push(c);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        1 | 2 => {
            f.cache |= n << 10;
            f.status = 3;
        }
        3 => {
            f.cache |= n << 4;
            f.status = 4;
        }
        4 => {
            let s = ((n >> 2) & 0xf) | (f.cache & 0xffff);
            f.status = 5;
            unit(f, s, (n & 0x3) << 14, out);
        }
        5 => {
            f.cache |= n << 8;
            f.status = 6;
        }
        6 => {
            f.cache |= n << 2;
            f.status = 7;
        }
        7 => {
            let s = ((n >> 4) & 0x3) | (f.cache & 0xffff);
            f.status = 8;
            unit(f, s, (n & 0xf) << 12, out);
        }
        8 => {
            f.cache |= n << 6;
            f.status = 9;
        }
        9 => {
            let s = n | (f.cache & 0xffff);
            f.status = 2;
            unit(f, s, 0, out);
        }
        _ => {}
    }
}

fn to_wchar(k: ToWchar, c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    match k {
        ToWchar::Jis => jis_wchar(c, f, false, out),
        ToWchar::Cp5022x => jis_wchar(c, f, true, out),
        ToWchar::Utf7 => utf7_wchar(c, f, out),
        ToWchar::Utf7Imap => utf7imap_wchar(c, f, out),
        ToWchar::Iso2022jpms => jpms_wchar(c, f, out),
        ToWchar::Iso2022kr => kr_wchar(c, f, out),
        ToWchar::Hz => hz_wchar(c, f, out),
        ToWchar::Iso2022jp2004 => jis2004_wchar(c, f, out),
        ToWchar::Iso2022jpKddi => kddi_wchar(c, f, out),
        ToWchar::Base64 => base64dec(c, f, out),
        ToWchar::Qprint => qprintdec(c, f, out),
        ToWchar::HtmlEnt => html_dec(c, f, out),
    }
}

/// The flush of an encoding -> wchar filter (before the next filter's).
fn to_wchar_flush(k: ToWchar, f: &mut Filter, out: &mut Vec<i32>) {
    match k {
        ToWchar::Jis
        | ToWchar::Cp5022x
        | ToWchar::Iso2022jpms
        | ToWchar::Iso2022kr
        | ToWchar::Iso2022jp2004
        | ToWchar::Iso2022jpKddi => {
            // a truncated 2-byte character or escape sequence
            if f.status & 0xf != 0 {
                out.push(MBFL_BAD_INPUT);
            }
            f.status = 0;
        }
        ToWchar::Utf7 => {
            // a surrogate pair's 2nd half never came, or unpadded data
            if f.cache != 0 {
                f.cache = 0;
                out.push(MBFL_BAD_INPUT);
            }
        }
        ToWchar::Base64 => base64dec_flush(f, out),
        ToWchar::Qprint => qprintdec_flush(f, out),
        ToWchar::HtmlEnt => html_dec_flush(f, out),
        ToWchar::Hz => {
            // a truncated 2-byte character
            if f.status == 0x11 {
                out.push(MBFL_BAD_INPUT);
            }
            f.status = 0;
        }
        ToWchar::Utf7Imap => {
            // ending in a Base64 section is illegal
            if f.status != 0 {
                out.push(MBFL_BAD_INPUT);
                f.status = 0;
            }
        }
    }
}

/* ===== wchar -> encoding ===== */

/// The ucs_*_jis_table lookups of the JIS encoders.
fn ucs_jis(c: i32, overline: bool) -> i32 {
    if c >= ucs_a1_jis_table_min && c < ucs_a1_jis_table_max {
        ucs_a1_jis_table[(c - ucs_a1_jis_table_min) as usize] as i32
    } else if overline && c == 0x203e {
        // OVERLINE: JIS X 0201 OVERLINE
        0x1007e
    } else if c >= ucs_a2_jis_table_min && c < ucs_a2_jis_table_max {
        ucs_a2_jis_table[(c - ucs_a2_jis_table_min) as usize] as i32
    } else if c >= ucs_i_jis_table_min && c < ucs_i_jis_table_max {
        ucs_i_jis_table[(c - ucs_i_jis_table_min) as usize] as i32
    } else if c >= ucs_r_jis_table_min && c < ucs_r_jis_table_max {
        ucs_r_jis_table[(c - ucs_r_jis_table_min) as usize] as i32
    } else {
        0
    }
}

/// The codepoints the JIS encoders map outside the tables.
fn jis_fallback(c: i32) -> i32 {
    match c {
        0xa5 => 0x1005c,  // YEN SIGN
        0xff3c => 0x2140, // FULLWIDTH REVERSE SOLIDUS
        0x2225 => 0x2142, // PARALLEL TO
        0xff0d => 0x215d, // FULLWIDTH HYPHEN-MINUS
        0xffe0 => 0x2171, // FULLWIDTH CENT SIGN
        0xffe1 => 0x2172, // FULLWIDTH POUND SIGN
        0xffe2 => 0x224c, // FULLWIDTH NOT SIGN
        _ => 0,
    }
}

fn escape(out: &mut Vec<u8>, seq: &[u8]) {
    out.push(0x1b);
    out.extend_from_slice(seq);
}

/// mbfl_filt_conv_wchar_jis
fn wchar_jis(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut s = ucs_jis(c, true);
    if s <= 0 {
        s = jis_fallback(c);
        if c == 0 {
            s = 0;
        } else if s <= 0 {
            s = -1;
        }
    }
    if s >= 0 {
        if s < 0x80 {
            // ASCII
            if f.status & 0xff00 != 0 {
                escape(out, b"(B");
            }
            f.status = 0;
            out.push(s as u8);
        } else if s < 0x8080 {
            // X 0208
            if f.status & 0xff00 != 0x200 {
                escape(out, b"$B");
            }
            f.status = 0x200;
            out.push(((s >> 8) & 0x7f) as u8);
            out.push((s & 0x7f) as u8);
        } else if s < 0x10000 {
            // X 0212
            if f.status & 0xff00 != 0x300 {
                escape(out, b"$(D");
            }
            f.status = 0x300;
            out.push(((s >> 8) & 0x7f) as u8);
            out.push((s & 0x7f) as u8);
        } else {
            // X 0201 latin
            if f.status & 0xff00 != 0x400 {
                escape(out, b"(J");
            }
            f.status = 0x400;
            out.push((s & 0x7f) as u8);
        }
    } else {
        illegal_output(FromWchar::Jis, c, f, out);
    }
}

/// mbfl_filt_conv_wchar_2022jp: unmappable characters are dropped.
fn wchar_2022jp(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut s = ucs_jis(c, false);
    if s <= 0 {
        s = jis_fallback(c);
        if c == 0 {
            s = 0;
        } else if s <= 0 {
            s = -1;
        }
    } else if (0x80..0x2121).contains(&s) || s > 0x8080 {
        s = -1;
    }
    if s >= 0 {
        if s < 0x80 {
            // ASCII
            if f.status & 0xff00 != 0 {
                escape(out, b"(B");
            }
            f.status = 0;
            out.push(s as u8);
        } else if s < 0x10000 {
            // X 0208
            if f.status & 0xff00 != 0x200 {
                escape(out, b"$B");
            }
            f.status = 0x200;
            out.push(((s >> 8) & 0x7f) as u8);
            out.push((s & 0x7f) as u8);
        } else {
            // X 0201 latin
            if f.status & 0xff00 != 0x400 {
                escape(out, b"(J");
            }
            f.status = 0x400;
            out.push((s & 0x7f) as u8);
        }
    }
}

/// mbfl_filt_conv_wchar_cp50221
fn wchar_cp50221(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut s = ucs_jis(c, true);
    if s == 0 && (0xe000..=0xe757).contains(&c) {
        // 'private'/'user' codepoints
        let p = c - 0xe000;
        s = (((p / 94) + 0x7f) << 8) | ((p % 94) + 0x21);
    }
    if s <= 0 {
        let fb = jis_fallback(c);
        if fb != 0 {
            s = fb;
        }
    }
    // Not in JIS X 0208 (or a JIS X 0213 code): the CP932 extensions
    if s == 0 || ((s & 0x8000) != 0 && (s & 0x80) != 0) {
        s = -1;
        let oh = cp932ext1_ucs_table_min / 94;
        for (i, &u) in cp932ext1_ucs_table.iter().enumerate() {
            if c == u as i32 {
                let i = i as i32;
                s = ((i / 94 + oh + 0x21) << 8) + (i % 94 + 0x21);
                break;
            }
        }
        if s < 0 {
            let oh = cp932ext2_ucs_table_min / 94;
            for (i, &u) in cp932ext2_ucs_table.iter().enumerate() {
                if c == u as i32 {
                    let i = i as i32;
                    s = ((i / 94 + oh + 0x21) << 8) + (i % 94 + 0x21);
                    break;
                }
            }
        }
        if c == 0 {
            s = 0;
        } else if s <= 0 {
            s = -1;
        }
    }
    if s >= 0 {
        if s < 0x80 {
            // ASCII
            if f.status & 0xff00 != 0 {
                escape(out, b"(B");
                f.status = 0;
            }
            out.push(s as u8);
        } else if (0xa0..0xe0).contains(&s) {
            // X 0201 kana
            if f.status & 0xff00 != 0x500 {
                escape(out, b"(I");
                f.status = 0x500;
            }
            out.push((s - 0x80) as u8);
        } else if s <= 0x927e {
            // X 0208 + extensions
            if f.status & 0xff00 != 0x200 {
                escape(out, b"$B");
                f.status = 0x200;
            }
            out.push(((s >> 8) & 0xff) as u8);
            out.push((s & 0xff) as u8);
        } else if s < 0x10000 {
            // X 0212
            illegal_output(FromWchar::Cp50221, c, f, out);
        } else {
            // X 0201 latin
            if f.status & 0xff00 != 0x400 {
                escape(out, b"(J");
            }
            f.status = 0x400;
            out.push((s & 0x7f) as u8);
        }
    } else {
        illegal_output(FromWchar::Cp50221, c, f, out);
    }
}

/// mb_convert_kana_codepoint() on the cached half-width kana of CP50220.
fn cp50220_translit(c: i32, next: i32, consumed: &mut bool) -> i32 {
    let mode = (MBFL_HAN2ZEN_KATAKANA | MBFL_HAN2ZEN_GLUE) as u32;
    let mut second = 0u32;
    super::iso2022::mb_convert_kana_codepoint(c as u32, next as u32, consumed, &mut second, mode)
        as i32
}

/// mbfl_filt_conv_wchar_cp50220: half-width kana are converted to
/// full-width ones (a voiced sound mark is glued to the preceding kana).
fn wchar_cp50220(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    if f.cache != 0 {
        let mut consumed = false;
        let s = cp50220_translit(f.cache, c, &mut consumed);
        f.cache = if consumed { 0 } else { c };
        // (the error markers go through CP50221's filter function)
        wchar_cp50221(s, f, out);
        if c == 0 && !consumed {
            out.push(0);
        }
    } else if c == 0 {
        // `cache == 0` means no codepoint is cached
        out.push(0);
    } else {
        f.cache = c;
    }
}

const UTF7IMAP_BASE64: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

fn b64(v: i32) -> u8 {
    UTF7IMAP_BASE64[(v & 0x3f) as usize]
}

/// mbfl_filt_conv_wchar_utf7imap
fn wchar_utf7imap(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let n;
    if c == b'&' as i32 {
        n = 1;
    } else if (0x20..=0x7e).contains(&c) || c == 0 {
        n = 2;
    } else if (0..MBFL_WCSPLANE_UCS2MAX).contains(&c) {
        n = 0;
    } else if (MBFL_WCSPLANE_SUPMIN..MBFL_WCSPLANE_SUPMAX).contains(&c) {
        wchar_utf7imap(((c >> 10) - 0x40) | 0xd800, f, out);
        wchar_utf7imap((c & 0x3ff) | 0xdc00, f, out);
        return;
    } else {
        illegal_output(FromWchar::Utf7Imap, c, f, out);
        return;
    }
    let direct = |out: &mut Vec<u8>| {
        out.push(b'-');
        out.push(c as u8);
        if n == 1 {
            out.push(b'-');
        }
    };
    match f.status {
        0 => {
            if n != 0 {
                out.push(c as u8);
                if n == 1 {
                    out.push(b'-');
                }
            } else {
                out.push(b'&');
                f.status = 1;
                f.cache = c;
            }
        }
        1 => {
            let s = f.cache;
            out.push(b64(s >> 10));
            out.push(b64(s >> 4));
            if n != 0 {
                out.push(UTF7IMAP_BASE64[((s << 2) & 0x3c) as usize]);
                direct(out);
                f.status = 0;
            } else {
                f.status = 2;
                f.cache = ((s & 0xf) << 16) | c;
            }
        }
        2 => {
            let s = f.cache;
            out.push(b64(s >> 14));
            out.push(b64(s >> 8));
            out.push(b64(s >> 2));
            if n != 0 {
                out.push(UTF7IMAP_BASE64[((s << 4) & 0x30) as usize]);
                direct(out);
                f.status = 0;
            } else {
                f.status = 3;
                f.cache = ((s & 0x3) << 16) | c;
            }
        }
        3 => {
            let s = f.cache;
            out.push(b64(s >> 12));
            out.push(b64(s >> 6));
            out.push(b64(s));
            if n != 0 {
                direct(out);
                f.status = 0;
            } else {
                f.status = 1;
                f.cache = c;
            }
        }
        _ => {}
    }
}

fn from_wchar(k: FromWchar, c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    match k {
        FromWchar::Jis => wchar_jis(c, f, out),
        FromWchar::Iso2022jp => wchar_2022jp(c, f, out),
        FromWchar::Cp50220 => wchar_cp50220(c, f, out),
        FromWchar::Cp50221 => wchar_cp50221(c, f, out),
        FromWchar::Cp50222 => wchar_cp50222(c, f, out),
        FromWchar::Utf7 => wchar_utf7(c, f, out),
        FromWchar::Utf7Imap => wchar_utf7imap(c, f, out),
        FromWchar::Iso2022jpms => wchar_2022jpms(c, f, out),
        FromWchar::Iso2022kr => wchar_2022kr(c, f, out),
        FromWchar::Hz => wchar_hz(c, f, out),
        FromWchar::Iso2022jp2004 => wchar_jis2004(c, f, out),
        FromWchar::Iso2022jpKddi => wchar_kddi(c, f, out),
        FromWchar::Base64 => base64enc(c, f, out),
        FromWchar::Qprint => qprintenc(c, f, out),
        FromWchar::HtmlEnt => html_enc(c, f, out),
    }
}

/// mbfl_filt_conv_illegal_output: the substitution character goes through
/// the filter function `k` (with substitution off, so an unencodable one
/// is dropped).
fn illegal_output(k: FromWchar, _c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mode_backup = f.illegal_mode;
    let substchar_backup = f.illegal_substchar;
    if f.illegal_mode == MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR && f.illegal_substchar != '?' as i32 {
        f.illegal_substchar = '?' as i32;
    } else {
        f.illegal_mode = MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE;
    }
    // (the long/entity modes are not used by mbfl_strcut())
    if mode_backup == MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR {
        from_wchar(k, substchar_backup, f, out);
    }
    f.illegal_mode = mode_backup;
    f.illegal_substchar = substchar_backup;
}

/// The flush of a wchar -> encoding filter.
fn from_wchar_flush(k: FromWchar, f: &mut Filter, out: &mut Vec<u8>) {
    match k {
        FromWchar::Jis | FromWchar::Iso2022jp | FromWchar::Cp50221 | FromWchar::Iso2022jpms => {
            any_jis_flush(f, out)
        }
        FromWchar::Cp50222 => {
            // back to latin
            if f.status & 0xff00 == 0x500 {
                out.push(0x0f);
            } else if f.status & 0xff00 != 0 {
                escape(out, b"(B");
            }
            f.status = 0;
        }
        FromWchar::Utf7 => wchar_utf7_flush(f, out),
        FromWchar::Iso2022jp2004 => wchar_jis2004_flush(f, out),
        FromWchar::Iso2022jpKddi => wchar_kddi_flush(f, out),
        FromWchar::Base64 => base64enc_flush(f, out),
        FromWchar::Qprint => qprintenc_flush(f, out),
        FromWchar::HtmlEnt => f.status = 0,
        FromWchar::Iso2022kr => {
            // back to ascii (no escape sequence can be pending here)
            if f.status & 0x10 != 0 {
                out.push(0x0f);
            }
            f.status = 0;
            f.cache = 0;
        }
        FromWchar::Hz => {
            if f.status & 0xff00 != 0 {
                out.extend_from_slice(b"~}");
            }
            f.status = 0;
        }
        FromWchar::Cp50220 => {
            if f.cache != 0 {
                let mut consumed = false;
                let s = cp50220_translit(f.cache, 0, &mut consumed);
                wchar_cp50221(s, f, out);
                f.cache = 0;
            }
            any_jis_flush(f, out);
        }
        FromWchar::Utf7Imap => {
            let (status, cache) = (f.status, f.cache);
            f.status = 0;
            f.cache = 0;
            match status {
                1 => {
                    out.push(b64(cache >> 10));
                    out.push(b64(cache >> 4));
                    out.push(UTF7IMAP_BASE64[((cache << 2) & 0x3c) as usize]);
                    out.push(b'-');
                }
                2 => {
                    out.push(b64(cache >> 14));
                    out.push(b64(cache >> 8));
                    out.push(b64(cache >> 2));
                    out.push(UTF7IMAP_BASE64[((cache << 4) & 0x30) as usize]);
                    out.push(b'-');
                }
                3 => {
                    out.push(b64(cache >> 12));
                    out.push(b64(cache >> 6));
                    out.push(b64(cache));
                    out.push(b'-');
                }
                _ => {}
            }
        }
    }
}

/// mbfl_filt_conv_any_jis_flush: back to ASCII
fn any_jis_flush(f: &mut Filter, out: &mut Vec<u8>) {
    if f.status & 0xff00 != 0 {
        escape(out, b"(B");
    }
    f.status = 0;
}

/* ===== UTF-7, ISO-2022-JP-MS, ISO-2022-KR, HZ (encoding -> wchar) ===== */

/// decode_base64_char() of UTF-7
fn utf7_b64(c: i32) -> i32 {
    if (b'A' as i32..=b'Z' as i32).contains(&c) {
        c - 65
    } else if (b'a' as i32..=b'z' as i32).contains(&c) {
        c - 71
    } else if (b'0' as i32..=b'9' as i32).contains(&c) {
        c + 4
    } else if c == b'+' as i32 {
        62
    } else if c == b'/' as i32 {
        63
    } else {
        -1
    }
}

/// mbfl_filt_conv_utf7_wchar
fn utf7_wchar(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    let mut n = -1;
    if f.status != 0 {
        // Modified Base64
        n = utf7_b64(c);
        if n < 0 {
            if f.cache != 0 {
                // a surrogate pair's 2nd half never came, or unpadded data
                out.push(MBFL_BAD_INPUT);
            }
            if c == b'-' as i32 {
                if f.status == 1 {
                    // "+-" -> "+"
                    out.push(b'+' as i32);
                }
            } else if (0..0x80).contains(&c) {
                out.push(c);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
            f.cache = 0;
            f.status = 0;
            return;
        }
    }
    // A decoded UTF-16 unit `s` (`next` the bits of the following one)
    let unit = |f: &mut Filter, s: i32, next: i32, out: &mut Vec<i32>| {
        if (0xd800..0xdc00).contains(&s) {
            // 1st part of surrogate pair
            if f.cache & 0xfff0000 != 0 {
                out.push(MBFL_BAD_INPUT);
            }
            f.cache = (((s & 0x3ff) << 16) + 0x400000) | next;
        } else if (0xdc00..0xe000).contains(&s) {
            // 2nd part of surrogate pair
            if f.cache & 0xfff0000 != 0 {
                let s = (s & 0x3ff) | ((f.cache & 0xfff0000) >> 6);
                f.cache = next;
                out.push(s);
            } else {
                out.push(MBFL_BAD_INPUT);
                f.cache = next;
            }
        } else {
            if f.cache & 0xfff0000 != 0 {
                out.push(MBFL_BAD_INPUT);
            }
            f.cache = next;
            out.push(s);
        }
    };
    match f.status {
        0 => {
            if c == b'+' as i32 {
                f.status = 1;
            } else if (0..0x80).contains(&c) {
                out.push(c);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        1 | 2 => {
            f.cache |= n << 10;
            f.status = 3;
        }
        3 => {
            f.cache |= n << 4;
            f.status = 4;
        }
        4 => {
            let s = ((n >> 2) & 0xf) | (f.cache & 0xffff);
            f.status = 5;
            unit(f, s, (n & 0x3) << 14, out);
        }
        5 => {
            f.cache |= n << 8;
            f.status = 6;
        }
        6 => {
            f.cache |= n << 2;
            f.status = 7;
        }
        7 => {
            let s = ((n >> 4) & 0x3) | (f.cache & 0xffff);
            f.status = 8;
            unit(f, s, (n & 0xf) << 12, out);
        }
        8 => {
            f.cache |= n << 6;
            f.status = 9;
        }
        9 => {
            let s = n | (f.cache & 0xffff);
            f.status = 2;
            unit(f, s, 0, out);
        }
        _ => {}
    }
}

const JPMS_KANA: i32 = 0x20;
const JPMS_KANJI: i32 = 0x80;
const JPMS_UDC: i32 = 0xa0;

/// mbfl_filt_conv_2022jpms_wchar
fn jpms_wchar(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    match f.status & 0xf {
        0 => {
            if c == 0x1b {
                f.status += 2;
            } else if f.status == JPMS_KANA && c > 0x20 && c < 0x60 {
                out.push(0xff40 + c);
            } else if (f.status == JPMS_KANJI || f.status == JPMS_UDC) && c > 0x20 && c < 0x80 {
                f.cache = c;
                f.status += 1;
            } else if (0..0x80).contains(&c) {
                out.push(c);
            } else if c > 0xa0 && c < 0xe0 {
                out.push(0xfec0 + c);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        1 => {
            // Kanji, second byte
            let mut w = 0;
            f.status &= !0xf;
            let c1 = f.cache;
            if c > 0x20 && c < 0x7f {
                let s = (c1 - 0x21) * 94 + c - 0x21;
                if f.status == JPMS_KANJI {
                    if s <= 137 {
                        w = match s {
                            31 => 0xff3c,  // FULLWIDTH REVERSE SOLIDUS
                            32 => 0xff5e,  // FULLWIDTH TILDE
                            33 => 0x2225,  // PARALLEL TO
                            60 => 0xff0d,  // FULLWIDTH HYPHEN-MINUS
                            80 => 0xffe0,  // FULLWIDTH CENT SIGN
                            81 => 0xffe1,  // FULLWIDTH POUND SIGN
                            137 => 0xffe2, // FULLWIDTH NOT SIGN
                            _ => 0,
                        };
                    }
                    if w == 0 {
                        if s >= cp932ext1_ucs_table_min && s < cp932ext1_ucs_table_max {
                            w = cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min) as usize] as i32;
                        } else if s >= 0 && s < jisx0208_ucs_table_size {
                            w = jisx0208_ucs_table[s as usize] as i32;
                        } else if s >= cp932ext2_ucs_table_min && s < cp932ext2_ucs_table_max {
                            w = cp932ext2_ucs_table[(s - cp932ext2_ucs_table_min) as usize] as i32;
                        }
                    }
                    if w <= 0 {
                        w = MBFL_BAD_INPUT;
                    }
                } else if c1 > 0x20 && c1 < 0x35 {
                    w = 0xe000 + (c1 - 0x21) * 94 + c - 0x21;
                } else {
                    w = MBFL_BAD_INPUT;
                }
                out.push(w);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        2 => {
            // ESC
            if c == b'$' as i32 {
                f.status += 1;
            } else if c == b'(' as i32 {
                f.status += 3;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        3 => {
            // ESC $
            if c == b'@' as i32 || c == b'B' as i32 {
                f.status = JPMS_KANJI;
            } else if c == b'(' as i32 {
                f.status += 1;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        4 => {
            // ESC $ (
            if c == b'@' as i32 || c == b'B' as i32 {
                f.status = JPMS_KANJI;
            } else if c == b'?' as i32 {
                f.status = JPMS_UDC;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        5 => {
            // ESC (
            if c == b'B' as i32 || c == b'J' as i32 {
                f.status = 0;
            } else if c == b'I' as i32 {
                f.status = JPMS_KANA;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        _ => {}
    }
}

/// mbfl_filt_conv_2022kr_wchar
fn kr_wchar(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    match f.status & 0xf {
        0 => {
            if c == 0x1b {
                f.status += 2;
            } else if c == 0x0f {
                // shift in (ASCII)
                f.status = 0;
            } else if c == 0x0e {
                // shift out (KSC5601)
                f.status = 0x10;
            } else if f.status & 0x10 != 0 && c > 0x20 && c < 0x7f {
                // KSC5601 lead byte
                f.cache = c;
                f.status = 0x11;
            } else if f.status & 0x10 == 0 && (0..0x80).contains(&c) {
                out.push(c);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        1 => {
            // dbcs second byte
            f.status = 0x10;
            let c1 = f.cache;
            let flag = if c1 > 0x20 && c1 < 0x47 {
                1
            } else if (0x47..=0x7e).contains(&c1) && c1 != 0x49 {
                2
            } else {
                0
            };
            if flag > 0 && c > 0x20 && c < 0x7f {
                let mut w = 0;
                if flag == 1 {
                    if c1 != 0x22 || c <= 0x65 {
                        let i = (c1 - 1) * 190 + (c - 0x41) + 0x80;
                        w = uhc1_ucs_table[i as usize] as i32;
                    }
                } else {
                    let i = (c1 - 0x47) * 94 + c - 0x21;
                    w = if i < uhc3_ucs_table_size {
                        uhc3_ucs_table[i as usize] as i32
                    } else {
                        MBFL_BAD_INPUT
                    };
                }
                if w <= 0 {
                    w = MBFL_BAD_INPUT;
                }
                out.push(w);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        2 => {
            // ESC
            if c == b'$' as i32 {
                f.status += 1;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        3 => {
            // ESC $
            if c == b')' as i32 {
                f.status += 1;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        4 => {
            // ESC $ )
            f.status = 0;
            if c != b'C' as i32 {
                out.push(MBFL_BAD_INPUT);
            }
        }
        _ => {}
    }
}

/// mbfl_filt_conv_hz_wchar
fn hz_wchar(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    match f.status & 0xf {
        0 => {
            if c == b'~' as i32 {
                f.status += 2;
            } else if f.status == 0x10 && ((c > 0x20 && c <= 0x29) || (0x30..=0x77).contains(&c)) {
                // DBCS first char
                f.cache = c;
                f.status += 1;
            } else if f.status == 0 && (0..0x80).contains(&c) {
                out.push(c);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        1 => {
            // GB2312 second char
            f.status &= !0xf;
            let c1 = f.cache;
            if c1 > 0x20 && c1 < 0x7f && c > 0x20 && c < 0x7f {
                let s = (c1 - 1) * 192 + c + 0x40;
                let mut w = if s == 0x1864 {
                    0x30fb
                } else if s == 0x186a {
                    0x2015
                } else if s == 0x186c {
                    0x2225
                } else if (0x1920..=0x192a).contains(&s)
                    || s == 0x1963
                    || (0x1c60..=0x1c7f).contains(&s)
                    || (0x1dbb..=0x1dc4).contains(&s)
                {
                    0
                } else {
                    cp936_ucs_table[s as usize] as i32
                };
                if w <= 0 {
                    w = MBFL_BAD_INPUT;
                }
                out.push(w);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        2 => {
            // '~'
            if c == b'}' as i32 && f.status == 0x12 {
                f.status = 0;
            } else if c == b'{' as i32 && f.status == 2 {
                f.status = 0x10;
            } else if c == b'~' as i32 && f.status == 2 {
                out.push(b'~' as i32);
                f.status -= 2;
            } else if c == b'\n' as i32 {
                // a line continuation
                f.status -= 2;
            } else {
                f.status -= 2;
                out.push(MBFL_BAD_INPUT);
            }
        }
        _ => {}
    }
}

/* ===== UTF-7, CP50222, ISO-2022-JP-MS, ISO-2022-KR, HZ (wchar -> encoding) ===== */

const UTF7_BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64u(v: i32) -> u8 {
    UTF7_BASE64[(v & 0x3f) as usize]
}

/// mbfl_filt_conv_wchar_utf7
fn wchar_utf7(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut n = 0;
    if (0..0x80).contains(&c) {
        let ch = c as u8;
        if ch.is_ascii_alphanumeric() || ch == 0 || ch == b'/' || ch == b'-' {
            n = 1;
        } else if matches!(
            ch,
            b' ' | b'\t' | b'\r' | b'\n' | b'\'' | b'(' | b')' | b',' | b'.' | b':' | b'?'
        ) {
            n = 2;
        }
    } else if (0..MBFL_WCSPLANE_UCS2MAX).contains(&c) {
    } else if (MBFL_WCSPLANE_SUPMIN..0x0011_0000).contains(&c) {
        wchar_utf7(((c >> 10) - 0x40) | 0xd800, f, out);
        wchar_utf7((c & 0x3ff) | 0xdc00, f, out);
        return;
    } else {
        illegal_output(FromWchar::Utf7, c, f, out);
        return;
    }
    let end = |out: &mut Vec<u8>| {
        if n == 1 {
            out.push(b'-');
        }
        out.push(c as u8);
    };
    match f.status {
        0 => {
            if n != 0 {
                out.push(c as u8);
            } else {
                out.push(b'+');
                f.status = 1;
                f.cache = c;
            }
        }
        1 => {
            let s = f.cache;
            out.push(b64u(s >> 10));
            out.push(b64u(s >> 4));
            if n != 0 {
                out.push(UTF7_BASE64[((s << 2) & 0x3c) as usize]);
                end(out);
                f.status = 0;
            } else {
                f.status = 2;
                f.cache = ((s & 0xf) << 16) | c;
            }
        }
        2 => {
            let s = f.cache;
            out.push(b64u(s >> 14));
            out.push(b64u(s >> 8));
            out.push(b64u(s >> 2));
            if n != 0 {
                out.push(UTF7_BASE64[((s << 4) & 0x30) as usize]);
                end(out);
                f.status = 0;
            } else {
                f.status = 3;
                f.cache = ((s & 0x3) << 16) | c;
            }
        }
        3 => {
            let s = f.cache;
            out.push(b64u(s >> 12));
            out.push(b64u(s >> 6));
            out.push(b64u(s));
            if n != 0 {
                end(out);
                f.status = 0;
            } else {
                f.status = 1;
                f.cache = c;
            }
        }
        _ => {}
    }
}

/// mbfl_filt_conv_wchar_utf7_flush (fragments of a Base64 section)
fn wchar_utf7_flush(f: &mut Filter, out: &mut Vec<u8>) {
    let (status, cache) = (f.status, f.cache);
    f.status = 0;
    f.cache = 0;
    match status {
        1 => {
            out.push(b64u(cache >> 10));
            out.push(b64u(cache >> 4));
            out.push(UTF7_BASE64[((cache << 2) & 0x3c) as usize]);
            out.push(b'-');
        }
        2 => {
            out.push(b64u(cache >> 14));
            out.push(b64u(cache >> 8));
            out.push(b64u(cache >> 2));
            out.push(UTF7_BASE64[((cache << 4) & 0x30) as usize]);
            out.push(b'-');
        }
        3 => {
            out.push(b64u(cache >> 12));
            out.push(b64u(cache >> 6));
            out.push(b64u(cache));
            out.push(b'-');
        }
        _ => {}
    }
}

/// mbfl_filt_conv_wchar_cp50222: CP50221 with SI/SO for half-width kana
fn wchar_cp50222(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut s = ucs_jis(c, true);
    if s == 0 && (0xe000..=0xe757).contains(&c) {
        let p = c - 0xe000;
        s = (((p / 94) + 0x7f) << 8) | ((p % 94) + 0x21);
    }
    if s <= 0 {
        let fb = jis_fallback(c);
        if fb != 0 {
            s = fb;
        }
    }
    if s == 0 || ((s & 0x8000) != 0 && (s & 0x80) != 0) {
        s = -1;
        let oh = cp932ext1_ucs_table_min / 94;
        for (i, &u) in cp932ext1_ucs_table.iter().enumerate() {
            if c == u as i32 {
                let i = i as i32;
                s = ((i / 94 + oh + 0x21) << 8) + (i % 94 + 0x21);
                break;
            }
        }
        if s <= 0 {
            let oh = cp932ext2_ucs_table_min / 94;
            for (i, &u) in cp932ext2_ucs_table.iter().enumerate() {
                if c == u as i32 {
                    let i = i as i32;
                    s = ((i / 94 + oh + 0x21) << 8) + (i % 94 + 0x21);
                    break;
                }
            }
        }
        if c == 0 {
            s = 0;
        } else if s <= 0 {
            s = -1;
        }
    }
    let shift_in = |f: &mut Filter, out: &mut Vec<u8>| {
        if f.status & 0xff00 == 0x500 {
            out.push(0x0f);
            f.status = 0;
        }
    };
    if s >= 0 {
        if s < 0x80 {
            // ASCII
            if f.status & 0xff00 == 0x500 {
                out.push(0x0f);
                f.status = 0;
            } else if f.status & 0xff00 != 0 {
                escape(out, b"(B");
                f.status = 0;
            }
            out.push(s as u8);
        } else if (0xa0..0xe0).contains(&s) {
            // X 0201 kana
            if f.status & 0xff00 != 0x500 {
                out.push(0x0e);
                f.status = 0x500;
            }
            out.push((s - 0x80) as u8);
        } else if s <= 0x927e {
            // X 0208
            shift_in(f, out);
            if f.status & 0xff00 != 0x200 {
                escape(out, b"$B");
                f.status = 0x200;
            }
            out.push(((s >> 8) & 0xff) as u8);
            out.push((s & 0xff) as u8);
        } else if s < 0x10000 {
            // X 0212
            illegal_output(FromWchar::Cp50222, c, f, out);
        } else {
            // X 0201 latin
            shift_in(f, out);
            if f.status & 0xff00 != 0x400 {
                escape(out, b"(J");
            }
            f.status = 0x400;
            out.push((s & 0x7f) as u8);
        }
    } else {
        illegal_output(FromWchar::Cp50222, c, f, out);
    }
}

/// sjistoidx()
fn sjistoidx(c1: i32, c2: i32) -> i32 {
    let base = if c1 > 0x9f {
        (c1 - 0xc1) * 188
    } else {
        (c1 - 0x81) * 188
    };
    base + c2 - if c2 > 0x7e { 0x41 } else { 0x40 }
}

/// cp932ext3_cp932ext2_jis()
fn cp932ext3_cp932ext2_jis(c: i32) -> i32 {
    let mut idx = sjistoidx(0xfa, 0x40) + c;
    if idx >= sjistoidx(0xfa, 0x5c) {
        idx -= sjistoidx(0xfa, 0x5c) - sjistoidx(0xed, 0x40);
    } else if idx >= sjistoidx(0xfa, 0x55) {
        idx -= sjistoidx(0xfa, 0x55) - sjistoidx(0xee, 0xfa);
    } else if idx >= sjistoidx(0xfa, 0x40) {
        idx -= sjistoidx(0xfa, 0x40) - sjistoidx(0xee, 0xef);
    }
    (((idx / 94) + 0x21) << 8) | ((idx % 94) + 0x21)
}

/// mbfl_filt_conv_wchar_2022jpms
fn wchar_2022jpms(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut s1 = ucs_jis(c, false);
    if s1 == 0 && (0xe000..0xe000 + 20 * 94).contains(&c) {
        // Private User Area (95ku - 114ku)
        let p = c - 0xe000;
        s1 = (((p / 94) + 0x7f) << 8) | ((p % 94) + 0x21);
    }
    if s1 <= 0 {
        let fb = match c {
            0xa5 => 0x216f, // YEN SIGN: FULLWIDTH YEN SIGN
            _ => jis_fallback(c),
        };
        if fb != 0 {
            s1 = fb;
        }
    }
    if s1 <= 0 || s1 >= 0xa1a1 {
        // not found or X 0212
        s1 = -1;
        for (i, &u) in cp932ext1_ucs_table.iter().enumerate() {
            if c == u as i32 {
                let i = i as i32;
                s1 = (((i / 94) + 0x2d) << 8) + (i % 94) + 0x21;
                break;
            }
        }
        if s1 <= 0 {
            for (i, &u) in cp932ext3_ucs_table.iter().enumerate() {
                if c == u as i32 {
                    s1 = cp932ext3_cp932ext2_jis(i as i32);
                    break;
                }
            }
        }
        if c == 0 {
            s1 = 0;
        }
    }
    if s1 >= 0 {
        if s1 < 0x80 {
            // latin
            if f.status & 0xff00 != 0 {
                escape(out, b"(B");
            }
            out.push(s1 as u8);
            f.status = 0;
        } else if s1 > 0xa0 && s1 < 0xe0 {
            // kana
            if f.status & 0xff00 != 0x100 {
                escape(out, b"(I");
            }
            f.status = 0x100;
            out.push((s1 & 0x7f) as u8);
        } else if s1 < 0x7e7f {
            // X 0208
            if f.status & 0xff00 != 0x200 {
                escape(out, b"$B");
            }
            f.status = 0x200;
            out.push(((s1 >> 8) & 0xff) as u8);
            out.push((s1 & 0x7f) as u8);
        } else if s1 < 0x927f {
            // UDC
            if f.status & 0xff00 != 0x800 {
                escape(out, b"$(?");
            }
            f.status = 0x800;
            out.push((((s1 >> 8) - 0x5e) & 0x7f) as u8);
            out.push((s1 & 0x7f) as u8);
        }
    } else {
        illegal_output(FromWchar::Iso2022jpms, c, f, out);
    }
}

/// mbfl_filt_conv_wchar_2022kr
fn wchar_2022kr(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    if f.status & 0x100 == 0 {
        escape(out, b"$)C");
        f.status |= 0x100;
    }
    let mut s = if c >= ucs_a1_uhc_table_min && c < ucs_a1_uhc_table_max {
        ucs_a1_uhc_table[(c - ucs_a1_uhc_table_min) as usize] as i32
    } else if c >= ucs_a2_uhc_table_min && c < ucs_a2_uhc_table_max {
        ucs_a2_uhc_table[(c - ucs_a2_uhc_table_min) as usize] as i32
    } else if c >= ucs_a3_uhc_table_min && c < ucs_a3_uhc_table_max {
        ucs_a3_uhc_table[(c - ucs_a3_uhc_table_min) as usize] as i32
    } else if c >= ucs_i_uhc_table_min && c < ucs_i_uhc_table_max {
        ucs_i_uhc_table[(c - ucs_i_uhc_table_min) as usize] as i32
    } else if c >= ucs_s_uhc_table_min && c < ucs_s_uhc_table_max {
        ucs_s_uhc_table[(c - ucs_s_uhc_table_min) as usize] as i32
    } else if c >= ucs_r1_uhc_table_min && c < ucs_r1_uhc_table_max {
        ucs_r1_uhc_table[(c - ucs_r1_uhc_table_min) as usize] as i32
    } else if c >= ucs_r2_uhc_table_min && c < ucs_r2_uhc_table_max {
        ucs_r2_uhc_table[(c - ucs_r2_uhc_table_min) as usize] as i32
    } else {
        0
    };
    let c1 = (s >> 8) & 0xff;
    let c2 = s & 0xff;
    // exclude UHC extension area
    if c1 < 0xa1 || c2 < 0xa1 {
        s = c;
    } else if s & 0x8000 != 0 {
        s -= 0x8080;
    }
    if s <= 0 {
        s = if c == 0 { 0 } else { -1 };
    } else if (0x80..0x2121).contains(&s) || s > 0x8080 {
        s = -1;
    }
    if s >= 0 {
        if s < 0x80 {
            // ASCII
            if f.status & 0x10 != 0 {
                out.push(0x0f);
                f.status &= !0x10;
            }
            out.push(s as u8);
        } else {
            if f.status & 0x10 == 0 {
                out.push(0x0e);
                f.status |= 0x10;
            }
            out.push(((s >> 8) & 0xff) as u8);
            out.push((s & 0xff) as u8);
        }
    } else {
        illegal_output(FromWchar::Iso2022kr, c, f, out);
    }
}

/// mbfl_filt_conv_wchar_hz
fn wchar_hz(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut s = 0;
    if c >= ucs_a1_cp936_table_min && c < ucs_a1_cp936_table_max {
        if !matches!(
            c,
            0xb7 | 0x144 | 0x148 | 0x251 | 0x261 | 0x2ca | 0x2cb | 0x2d9
        ) {
            s = ucs_a1_cp936_table[(c - ucs_a1_cp936_table_min) as usize] as i32;
        }
    } else if c >= ucs_a2_cp936_table_min && c < ucs_a2_cp936_table_max {
        if c == 0x2015 {
            s = 0xa1aa;
        } else if !(matches!(
            c,
            0x2010
                | 0x2013
                | 0x2014
                | 0x2016
                | 0x2025
                | 0x2035
                | 0x2105
                | 0x2109
                | 0x2121
                | 0x2215
                | 0x221f
                | 0x2223
                | 0x2252
                | 0x2266
                | 0x2267
                | 0x2295
                | 0x22bf
                | 0x2609
                | 0x25bc
                | 0x25bd
        ) || (0x2170..=0x2179).contains(&c)
            || (0x2196..=0x2199).contains(&c)
            || (0x2550..=0x2573).contains(&c)
            || (0x2581..=0x258f).contains(&c)
            || (0x2593..=0x2595).contains(&c)
            || (0x25e2..=0x25e5).contains(&c))
        {
            s = ucs_a2_cp936_table[(c - ucs_a2_cp936_table_min) as usize] as i32;
        }
    } else if c >= ucs_a3_cp936_table_min && c < ucs_a3_cp936_table_max {
        if c == 0x30fb {
            s = 0xa1a4;
        } else if !(matches!(c, 0x3006 | 0x3007 | 0x3012 | 0x3231 | 0x32a3)
            || c >= 0x3300
            || (0x3018..=0x3040).contains(&c)
            || (0x309b..=0x309e).contains(&c)
            || (0x30fc..=0x30fe).contains(&c))
        {
            s = ucs_a3_cp936_table[(c - ucs_a3_cp936_table_min) as usize] as i32;
        }
    } else if c >= ucs_i_gb2312_table_min && c < ucs_i_gb2312_table_max {
        s = ucs_i_gb2312_table[(c - ucs_i_gb2312_table_min) as usize] as i32;
    } else if c >= ucs_hff_cp936_table_min && c < ucs_hff_cp936_table_max {
        if c == 0xff04 {
            s = 0xa1e7;
        } else if c == 0xff5e {
            s = 0xa1ab;
        } else if (0xff01..=0xff5d).contains(&c) {
            s = c - 0xff01 + 0xa3a1;
        } else if matches!(c, 0xffe0 | 0xffe1 | 0xffe3 | 0xffe5) {
            s = ucs_hff_s_cp936_table[(c - 0xffe0) as usize] as i32;
        }
    }
    if s & 0x8000 != 0 {
        s -= 0x8080;
    }
    if s <= 0 {
        s = if c == 0 { 0 } else { -1 };
    } else if (0x80..0x2121).contains(&s) || s > 0x8080 {
        s = -1;
    }
    if s >= 0 {
        if s < 0x80 {
            // ASCII
            if f.status & 0xff00 != 0 {
                out.extend_from_slice(b"~}");
            }
            f.status = 0;
            if s == 0x7e {
                out.push(b'~');
            }
            out.push(s as u8);
        } else {
            // GB 2312-80
            if f.status & 0xff00 != 0x200 {
                out.extend_from_slice(b"~{");
            }
            f.status = 0x200;
            out.push(((s >> 8) & 0x7f) as u8);
            out.push((s & 0x7f) as u8);
        }
    } else {
        illegal_output(FromWchar::Hz, c, f, out);
    }
}

/* ===== ISO-2022-JP-2004 ===== */

/// mbfl_filt_conv_jis2004_wchar (as ISO-2022-JP-2004)
fn jis2004_wchar(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    match f.status & 0xf {
        0 => {
            if (0..0x80).contains(&c) {
                if c == 0x1b {
                    f.status += 6;
                } else if (f.status == 0x80 || f.status == 0x90 || f.status == 0xa0)
                    && c > 0x20
                    && c < 0x7f
                {
                    // kanji first char
                    f.cache = c;
                    f.status += match f.status {
                        0x90 => 1, // JIS X 0213 plane 1
                        0xa0 => 4, // JIS X 0213 plane 2
                        _ => 5,    // JIS X 0208
                    };
                } else {
                    out.push(c);
                }
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        1 => {
            // kanji second char
            f.status &= !0xf;
            let c1 = f.cache;
            if !(0x21..=0x7e).contains(&c) {
                out.push(MBFL_BAD_INPUT);
                return;
            }
            let (s1, s2) = (c1, c);
            let mut w1 = (s1 << 8) | s2;
            let mut w = 0;
            // combining characters
            if (0x2477..=0x247b).contains(&w1)
                || (0x2577..=0x257e).contains(&w1)
                || w1 == 0x2678
                || w1 == 0x2b44
                || (0x2b48..=0x2b4f).contains(&w1)
                || (0x2b65..=0x2b66).contains(&w1)
            {
                let k = mbfl_bisec_srch2(w1, &jisx0213_u2_key, jisx0213_u2_tbl_len);
                if k >= 0 {
                    out.push(jisx0213_u2_tbl[2 * k as usize] as i32);
                    w = jisx0213_u2_tbl[2 * k as usize + 1] as i32;
                }
            }
            // BMP
            if w <= 0 {
                w1 = (s1 - 0x21) * 94 + s2 - 0x21;
                if w1 >= 0 && w1 < jisx0213_ucs_table_size {
                    w = jisx0213_ucs_table[w1 as usize] as i32;
                }
            }
            // CJK Unified Ideographs ext.B (U+2XXXX)
            if w <= 0 {
                let k = mbfl_bisec_srch2(w1, &jisx0213_jis_u5_key, jisx0213_u5_tbl_len);
                if k >= 0 {
                    w = jisx0213_jis_u5_tbl[k as usize] as i32 + 0x20000;
                }
            }
            if w <= 0 {
                w = MBFL_BAD_INPUT;
            }
            out.push(w);
        }
        4 => {
            // X 0213 plane 2 second char
            f.status &= !0xf;
            let c1 = f.cache;
            let c2 = c;
            if !(0x21..=0x7e).contains(&c2) {
                out.push(MBFL_BAD_INPUT);
                return;
            }
            let s1 = c1 - 0x21;
            let s2 = c2 - 0x21;
            if (((0..=4).contains(&s1) && s1 != 1)
                || s1 == 7
                || (11..=14).contains(&s1)
                || (77..94).contains(&s1))
                && (0..94).contains(&s2)
            {
                // offset from ku
                let mut k = 0;
                while k < jisx0213_p2_ofst_len {
                    if s1 == jisx0213_p2_ofst[k as usize] as i32 {
                        break;
                    }
                    k += 1;
                }
                k -= jisx0213_p2_ofst[k.min(jisx0213_p2_ofst_len - 1) as usize] as i32;
                let s = (s1 + 94 + k) * 94 + s2;
                let mut w = jisx0213_ucs_table[s as usize] as i32;
                if w <= 0 {
                    let k = mbfl_bisec_srch2(s, &jisx0213_jis_u5_key, jisx0213_u5_tbl_len);
                    if k >= 0 {
                        w = jisx0213_jis_u5_tbl[k as usize] as i32 + 0x20000;
                    }
                }
                if w <= 0 {
                    w = MBFL_BAD_INPUT;
                }
                out.push(w);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        5 => {
            // X 0208
            f.status &= !0xf;
            let c1 = f.cache;
            let mut w = 0;
            if c > 0x20 && c < 0x7f {
                let s = (c1 - 0x21) * 94 + c - 0x21;
                if s >= 0 && s < jisx0208_ucs_table_size {
                    w = jisx0208_ucs_table[s as usize] as i32;
                }
            }
            if w <= 0 {
                w = MBFL_BAD_INPUT;
            }
            out.push(w);
        }
        6 => {
            // ESC
            if c == b'$' as i32 {
                f.status += 1;
            } else if c == b'(' as i32 {
                f.status += 3;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        7 => {
            // ESC $
            if c == b'B' as i32 {
                f.status = 0x80;
            } else if c == b'(' as i32 {
                f.status += 1;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        8 => {
            // ESC $ (
            if c == b'Q' as i32 {
                f.status = 0x90;
            } else if c == b'P' as i32 {
                f.status = 0xa0;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        9 => {
            // ESC (
            if c == b'B' as i32 {
                f.status = 0;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        _ => {}
    }
}

/// mbfl_filt_conv_wchar_jis2004 (as ISO-2022-JP-2004)
fn wchar_jis2004(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    loop {
        let mut s1 = 0;
        // 1st char of combining characters
        if f.status & 0xf == 0
            && (c == 0x00e6
                || (0x0254..=0x02e9).contains(&c)
                || (0x304b..=0x3053).contains(&c)
                || (0x30ab..=0x30c8).contains(&c)
                || c == 0x31f7)
        {
            for k in 0..jisx0213_u2_tbl_len {
                if c == jisx0213_u2_tbl[2 * k as usize] as i32 {
                    f.status += 1;
                    f.cache = k;
                    return;
                }
            }
        }
        // 2nd char of combining characters
        if f.status & 0xf == 1 && f.cache >= 0 && f.cache < jisx0213_u2_tbl_len {
            let mut k = f.cache;
            f.status &= !0xf;
            f.cache = 0;
            let c1 = jisx0213_u2_tbl[2 * k as usize] as i32;
            if matches!(c1, 0x0254 | 0x028c | 0x0259 | 0x025a) && c == 0x0301 {
                k += 1;
            }
            if c == jisx0213_u2_tbl[2 * k as usize + 1] as i32 {
                s1 = jisx0213_u2_key[k as usize] as i32;
            } else {
                // fallback: flush out the cached character
                let s = jisx0213_u2_fb_tbl[k as usize] as i32;
                if f.status != 0x200 {
                    escape(out, b"$(Q");
                }
                f.status = 0x200;
                out.push(((s >> 8) & 0x7f) as u8);
                out.push((s & 0x7f) as u8);
                continue;
            }
        }
        // major japanese chars: U+4E00 - U+9FFF
        if s1 <= 0 {
            for (range, tbl) in uni2jis_tbl_range.iter().zip(uni2jis_tbl.iter()) {
                if c >= range[0] && c <= range[1] {
                    s1 = tbl[(c - range[0]) as usize] as i32;
                    break;
                }
            }
        }
        // compressed mapping area: U+1E00 - U+4DBF
        if s1 <= 0 && c >= ucs_c1_jisx0213_min && c <= ucs_c1_jisx0213_max {
            let k = mbfl_bisec_srch(c, &ucs_c1_jisx0213_tbl, ucs_c1_jisx0213_tbl_len);
            if k >= 0 {
                s1 = ucs_c1_jisx0213_ofst[k as usize] as i32 + c
                    - ucs_c1_jisx0213_tbl[2 * k as usize] as i32;
            }
        }
        // CJK Unified Ideographs ext.B (U+2XXXX)
        if s1 <= 0 && c >= jisx0213_u5_tbl_min && c <= jisx0213_u5_tbl_max {
            let k = mbfl_bisec_srch2(c - 0x20000, &jisx0213_u5_jis_key, jisx0213_u5_tbl_len);
            if k >= 0 {
                s1 = jisx0213_u5_jis_tbl[k as usize] as i32;
            }
        }
        if s1 <= 0 {
            // CJK Compatibility Forms / Ideographs
            if c == 0xfe45 {
                s1 = 0x233e;
            } else if c == 0xfe46 {
                s1 = 0x233d;
            } else if (0xf91d..=0xf9dc).contains(&c) {
                let k = mbfl_bisec_srch2(c, &ucs_r2b_jisx0213_cmap_key, ucs_r2b_jisx0213_cmap_len);
                if k >= 0 {
                    s1 = ucs_r2b_jisx0213_cmap_val[k as usize] as i32;
                }
            }
        }
        if s1 <= 0 {
            s1 = if c == 0 { 0 } else { -1 };
        }
        if s1 >= 0 {
            if s1 < 0x80 {
                // ASCII
                if f.status & 0xff00 != 0 {
                    escape(out, b"(B");
                }
                f.status = 0;
                out.push(s1 as u8);
            } else if s1 < 0x100 {
                // latin or kana
                illegal_output(FromWchar::Iso2022jp2004, c, f, out);
            } else if s1 < 0x7f00 {
                // X 0213 plane 1
                if f.status & 0xff00 != 0x200 {
                    escape(out, b"$(Q");
                }
                f.status = 0x200;
                out.push(((s1 >> 8) & 0xff) as u8);
                out.push((s1 & 0xff) as u8);
            } else {
                // X 0213 plane 2
                let s2 = s1 & 0xff;
                let k = ((s1 >> 8) & 0xff) - 0x7f;
                let s1 = if k >= 0 && k < jisx0213_p2_ofst_len {
                    jisx0213_p2_ofst[k as usize] as i32 + 0x21
                } else {
                    s1
                };
                if f.status & 0xff00 != 0x200 {
                    escape(out, b"$(P");
                }
                f.status = 0x200;
                out.push(s1 as u8);
                out.push(s2 as u8);
            }
        } else {
            illegal_output(FromWchar::Iso2022jp2004, c, f, out);
        }
        return;
    }
}

/// mbfl_filt_conv_wchar_jis2004_flush (as ISO-2022-JP-2004)
fn wchar_jis2004_flush(f: &mut Filter, out: &mut Vec<u8>) {
    let k = f.cache;
    f.cache = 0;
    if f.status == 1 && k >= 0 && k < jisx0213_u2_tbl_len {
        let s = jisx0213_u2_fb_tbl[k as usize] as i32;
        if f.status & 0xff00 != 0x200 {
            escape(out, b"$(Q");
        }
        f.status = 0x200;
        out.push(((s >> 8) & 0x7f) as u8);
        out.push((s & 0x7f) as u8);
    }
    // back to ASCII
    if f.status & 0xff00 != 0 {
        escape(out, b"(B");
    }
    f.status = 0;
}

/* ===== ISO-2022-JP-KDDI ===== */

const KDDI_KANA: i32 = 0x20;
const KDDI_KANJI: i32 = 0x80;

/// mbfl_filt_conv_2022jp_mobile_wchar
fn kddi_wchar(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    match f.status & 0xf {
        0 => {
            if c == 0x1b {
                f.status += 2;
            } else if f.status == KDDI_KANA && c > 0x20 && c < 0x60 {
                out.push(0xff40 + c);
            } else if f.status == KDDI_KANJI && c > 0x20 && c < 0x80 {
                f.cache = c;
                f.status += 1;
            } else if (0..0x80).contains(&c) {
                out.push(c);
            } else if c > 0xa0 && c < 0xe0 {
                out.push(0xfec0 + c);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        1 => {
            // JIS X 0208, second byte
            let mut w = 0;
            f.status &= !0xf;
            let c1 = f.cache;
            if c > 0x20 && c < 0x7f {
                let mut s = (c1 - 0x21) * 94 + c - 0x21;
                if s <= 137 {
                    w = match s {
                        31 => 0xff3c,
                        32 => 0xff5e,
                        33 => 0x2225,
                        60 => 0xff0d,
                        80 => 0xffe0,
                        81 => 0xffe1,
                        137 => 0xffe2,
                        _ => 0,
                    };
                }
                if (84 * 94..91 * 94).contains(&s) {
                    s += 22 * 94;
                    let mut snd = 0;
                    w = super::mobile::mbfilter_sjis_emoji_kddi2unicode(s, &mut snd);
                    if w > 0 && snd > 0 {
                        out.push(snd);
                    }
                }
                if w == 0 {
                    if s >= cp932ext1_ucs_table_min && s < cp932ext1_ucs_table_max {
                        w = cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min) as usize] as i32;
                    } else if s >= 0 && s < jisx0208_ucs_table_size {
                        w = jisx0208_ucs_table[s as usize] as i32;
                    }
                }
                if w <= 0 {
                    w = MBFL_BAD_INPUT;
                }
                out.push(w);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
        2 => {
            if c == b'$' as i32 {
                f.status += 1;
            } else if c == b'(' as i32 {
                f.status += 3;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        3 => {
            if c == b'@' as i32 || c == b'B' as i32 {
                f.status = KDDI_KANJI;
            } else if c == b'(' as i32 {
                f.status += 1;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        4 => {
            if c == b'@' as i32 || c == b'B' as i32 {
                f.status = KDDI_KANJI;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        5 => {
            if c == b'B' as i32 || c == b'J' as i32 {
                f.status = 0;
            } else if c == b'I' as i32 {
                f.status = KDDI_KANA;
            } else {
                f.status &= !0xf;
                out.push(MBFL_BAD_INPUT);
            }
        }
        _ => {}
    }
}

/// mbfilter_unicode2sjis_emoji_kddi: 1 with the kuten code in `s1` for a
/// KDDI emoji (a keypad digit waits for U+20E3).
fn unicode2sjis_emoji_kddi(c: i32, s1: &mut i32, f: &mut Filter, out: &mut Vec<u8>) -> bool {
    if f.status & 0xf == 1 {
        let c1 = f.cache;
        f.cache = 0;
        f.status &= !0xff;
        if c == 0x20e3 {
            *s1 = if c1 == b'#' as i32 {
                0x25bc
            } else if c1 == b'0' as i32 {
                0x2830
            } else {
                // '1'-'9'
                0x27a6 + (c1 - b'1' as i32)
            };
            return true;
        }
        if f.status & 0xff00 != 0 {
            escape(out, b"(B");
        }
        out.push(c1 as u8);
        f.status = 0;
    }
    if c == b'#' as i32 || (b'0' as i32..=b'9' as i32).contains(&c) {
        f.status |= 1;
        f.cache = c;
        return false;
    }
    if c == 0xa9 {
        // Copyright sign
        *s1 = 0x27dc;
        return true;
    } else if c == 0xae {
        // Registered sign
        *s1 = 0x27dd;
        return true;
    }
    if c >= 0 {
        let k = super::mobile::emoji_lookup3(c as u32, &super::mobile::kddi_tables());
        if k != 0 {
            *s1 = k as i32;
            return true;
        }
    }
    false
}

/// mbfl_filt_conv_wchar_2022jp_mobile (as ISO-2022-JP-KDDI)
fn wchar_kddi(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let mut s1 = ucs_jis(c, false);
    let mut s2 = 0;
    if s1 <= 0 {
        let fb = match c {
            0xa5 => 0x216f, // YEN SIGN: FULLWIDTH YEN SIGN
            _ => jis_fallback(c),
        };
        if fb != 0 {
            s1 = fb;
        }
    }
    if unicode2sjis_emoji_kddi(c, &mut s1, f, out) {
        // CODE2JIS
        let c1 = s1 / 94 + 0x21;
        let c2 = s1 - 94 * (c1 - 0x21) + 0x21;
        s1 = ((c1 << 8) | c2) - 0x1600;
        s2 = 1;
    } else if f.status & 0xff == 1 && f.cache != 0 {
        // a phone keypad emoji in progress
        return;
    }
    if s1 <= 0 || (s1 >= 0xa1a1 && s2 == 0) {
        // not found or X 0212
        s1 = -1;
        for (i, &u) in cp932ext1_ucs_table.iter().enumerate() {
            if c == u as i32 {
                let i = i as i32;
                s1 = (((i / 94) + 0x2d) << 8) + (i % 94) + 0x21;
                break;
            }
        }
        if c == 0 {
            s1 = 0;
        }
    }
    if s1 >= 0 {
        if s1 < 0x80 {
            // ASCII
            if f.status & 0xff00 != 0 {
                escape(out, b"(B");
            }
            out.push(s1 as u8);
            f.status = 0;
        } else if s1 > 0xa0 && s1 < 0xe0 {
            // Kana
            if f.status & 0xff00 != 0x100 {
                escape(out, b"(I");
            }
            f.status = 0x100;
            out.push((s1 & 0x7f) as u8);
        } else if s1 < 0x7e7f {
            // JIS X 0208
            if f.status & 0xff00 != 0x200 {
                escape(out, b"$B");
            }
            f.status = 0x200;
            out.push(((s1 >> 8) & 0xff) as u8);
            out.push((s1 & 0x7f) as u8);
        }
    } else {
        illegal_output(FromWchar::Iso2022jpKddi, c, f, out);
    }
}

/// mbfl_filt_conv_wchar_2022jp_mobile_flush
fn wchar_kddi_flush(f: &mut Filter, out: &mut Vec<u8>) {
    // back to ASCII
    if f.status & 0xff00 != 0 {
        escape(out, b"(B");
    }
    let c1 = f.cache;
    if f.status & 0xff == 1 && (c1 == b'#' as i32 || (b'0' as i32..=b'9' as i32).contains(&c1)) {
        out.push(c1 as u8);
    }
    f.status = 0;
    f.cache = 0;
}

/* ===== BASE64, Quoted-Printable, HTML-ENTITIES ===== */
// (the transfer encodings convert to and from 8bit: the values between the
// filters are bytes, except for HTML-ENTITIES)

/// mbfl_filt_conv_base64dec
fn base64dec(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    // CR, LF, SPACE, HTAB, '='
    if matches!(c, 0x0d | 0x0a | 0x20 | 0x09 | 0x3d) {
        return;
    }
    let n = if (0x41..=0x5a).contains(&c) {
        c - 65
    } else if (0x61..=0x7a).contains(&c) {
        c - 71
    } else if (0x30..=0x39).contains(&c) {
        c + 4
    } else if c == 0x2b {
        62
    } else if c == 0x2f {
        63
    } else {
        out.push(MBFL_BAD_INPUT);
        return;
    } & 0x3f;
    match f.status {
        0 => {
            f.status = 1;
            f.cache = n << 18;
        }
        1 => {
            f.status = 2;
            f.cache |= n << 12;
        }
        2 => {
            f.status = 3;
            f.cache |= n << 6;
        }
        _ => {
            f.status = 0;
            let n = n | f.cache;
            out.push((n >> 16) & 0xff);
            out.push((n >> 8) & 0xff);
            out.push(n & 0xff);
        }
    }
}

/// mbfl_filt_conv_base64dec_flush
fn base64dec_flush(f: &mut Filter, out: &mut Vec<i32>) {
    let (status, cache) = (f.status, f.cache);
    f.status = 0;
    f.cache = 0;
    if status >= 2 {
        out.push((cache >> 16) & 0xff);
        if status >= 3 {
            out.push((cache >> 8) & 0xff);
        }
    }
}

const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// mbfl_filt_conv_base64enc
fn base64enc(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let n = f.status & 0xff;
    if n == 0 {
        f.status += 1;
        f.cache = (c & 0xff) << 16;
    } else if n == 1 {
        f.status += 1;
        f.cache |= (c & 0xff) << 8;
    } else {
        f.status &= !0xff;
        if (f.status & 0xff00) >> 8 > 72 {
            out.extend_from_slice(b"\r\n");
            f.status &= !0xff00;
        }
        f.status += 0x400;
        let n = f.cache | (c & 0xff);
        out.push(BASE64_TABLE[((n >> 18) & 0x3f) as usize]);
        out.push(BASE64_TABLE[((n >> 12) & 0x3f) as usize]);
        out.push(BASE64_TABLE[((n >> 6) & 0x3f) as usize]);
        out.push(BASE64_TABLE[(n & 0x3f) as usize]);
    }
}

/// mbfl_filt_conv_base64enc_flush
fn base64enc_flush(f: &mut Filter, out: &mut Vec<u8>) {
    let status = f.status & 0xff;
    let cache = f.cache;
    let len = (f.status & 0xff00) >> 8;
    f.status &= !0xffff;
    f.cache = 0;
    if status >= 1 {
        if len > 72 {
            out.extend_from_slice(b"\r\n");
        }
        out.push(BASE64_TABLE[((cache >> 18) & 0x3f) as usize]);
        out.push(BASE64_TABLE[((cache >> 12) & 0x3f) as usize]);
        if status == 1 {
            out.extend_from_slice(b"==");
        } else {
            out.push(BASE64_TABLE[((cache >> 6) & 0x3f) as usize]);
            out.push(b'=');
        }
    }
}

/// hex2code_map
fn hex2code(c: i32) -> i32 {
    match (c & 0xff) as u8 {
        d @ b'0'..=b'9' => (d - b'0') as i32,
        d @ b'A'..=b'F' => (d - b'A' + 10) as i32,
        d @ b'a'..=b'f' => (d - b'a' + 10) as i32,
        _ => -1,
    }
}

/// mbfl_filt_conv_qprintdec
fn qprintdec(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    match f.status {
        1 => {
            if hex2code(c) >= 0 {
                f.cache = c;
                f.status = 2;
            } else if c == 0x0d {
                // soft line feed
                f.status = 3;
            } else if c == 0x0a {
                f.status = 0;
            } else {
                out.push(0x3d);
                out.push(c);
                f.status = 0;
            }
        }
        2 => {
            let m = hex2code(c);
            let n = if m < 0 {
                out.push(0x3d);
                out.push(f.cache);
                c
            } else {
                (hex2code(f.cache) << 4) | m
            };
            out.push(n);
            f.status = 0;
        }
        3 => {
            if c != 0x0a {
                out.push(c);
            }
            f.status = 0;
        }
        _ => {
            if c == 0x3d {
                f.status = 1;
            } else {
                out.push(c);
            }
        }
    }
}

/// mbfl_filt_conv_qprintdec_flush
fn qprintdec_flush(f: &mut Filter, out: &mut Vec<i32>) {
    let (status, cache) = (f.status, f.cache);
    f.status = 0;
    f.cache = 0;
    if status == 1 {
        out.push(0x3d);
    } else if status == 2 {
        out.push(0x3d);
        out.push(cache);
    }
}

fn hex_digit(n: i32) -> u8 {
    if n < 10 {
        (n + 48) as u8
    } else {
        (n + 55) as u8
    }
}

/// mbfl_filt_conv_qprintenc
fn qprintenc(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    if f.status & 0xff == 0 {
        f.cache = c;
        f.status += 1;
        return;
    }
    let s = f.cache;
    f.cache = c;
    let n = (f.status & 0xff00) >> 8;
    if s == 0 {
        // null
        out.push(0);
        f.status &= !0xff00;
        return;
    }
    if s == b'\n' as i32 || (s == b'\r' as i32 && c != b'\n' as i32) {
        // line feed
        out.extend_from_slice(b"\r\n");
        f.status &= !0xff00;
        return;
    } else if s == 0x0d {
        return;
    }
    if n >= 72 {
        // soft line feed
        out.extend_from_slice(b"=\r\n");
        f.status &= !0xff00;
    }
    if s <= 0 || s >= 0x80 || s == b'=' as i32 {
        // hex-octet
        out.push(b'=');
        out.push(hex_digit((s >> 4) & 0xf));
        out.push(hex_digit(s & 0xf));
        f.status += 0x300;
    } else {
        out.push(s as u8);
        f.status += 0x100;
    }
}

/// mbfl_filt_conv_qprintenc_flush
fn qprintenc_flush(f: &mut Filter, out: &mut Vec<u8>) {
    // flush the cache
    qprintenc(0, f, out);
    f.status &= !0xffff;
    f.cache = 0;
}

const HTML_ENC_BUFFER_SIZE: i32 = 16;

/// strchr(html_entity_chars, c) (which finds the terminating NUL too)
fn html_entity_char(c: i32) -> bool {
    c == 0
        || (0..0x100).contains(&c)
            && b"#0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
                .contains(&(c as u8))
}

/// mbfl_filt_conv_html_dec
fn html_dec(c: i32, f: &mut Filter, out: &mut Vec<i32>) {
    let flush_buffer = |f: &mut Filter, out: &mut Vec<i32>| {
        for pos in 0..f.status.max(0) as usize {
            out.push(f.buffer[pos] as i32);
        }
        f.status = 0;
    };
    if f.status == 0 {
        if c == b'&' as i32 {
            f.status = 1;
            f.buffer[0] = b'&';
        } else {
            out.push(c);
        }
        return;
    }
    if c == b';' as i32 {
        if f.buffer[1] == b'#' {
            let mut ent: u32 = 0;
            if f.status > 2 && (f.buffer[2] == b'x' || f.buffer[2] == b'X') {
                if f.status > 3 {
                    // numeric entity
                    for pos in 3..f.status as usize {
                        let v = f.buffer[pos];
                        let v = match v {
                            b'0'..=b'9' => (v - b'0') as u32,
                            b'A'..=b'F' => (v - b'A' + 10) as u32,
                            b'a'..=b'f' => (v - b'a' + 10) as u32,
                            _ => {
                                ent = u32::MAX;
                                break;
                            }
                        };
                        ent = ent.wrapping_mul(16).wrapping_add(v);
                    }
                } else {
                    ent = u32::MAX;
                }
            } else if f.status > 2 {
                // numeric entity
                for pos in 2..f.status as usize {
                    if ent > 0x19999999 {
                        ent = u32::MAX;
                        break;
                    }
                    let v = f.buffer[pos];
                    if v.is_ascii_digit() {
                        ent = ent * 10 + (v - b'0') as u32;
                    } else {
                        ent = u32::MAX;
                        break;
                    }
                }
            } else {
                ent = u32::MAX;
            }
            if ent < 0x110000 {
                out.push(ent as i32);
            } else {
                for pos in 0..f.status as usize {
                    out.push(f.buffer[pos] as i32);
                }
                out.push(c);
            }
            f.status = 0;
        } else {
            // named entity
            // (strcmp(): up to a NUL)
            let name = f.buffer[1..f.status as usize]
                .split(|&b| b == 0)
                .next()
                .unwrap_or(&[]);
            let ent = super::transfer::HTML_ENTITY_LIST
                .iter()
                .find(|(n, _)| n.as_bytes() == name)
                .map_or(0, |(_, code)| *code);
            if ent != 0 {
                out.push(ent as i32);
                f.status = 0;
            } else {
                // failure: flush the fragments
                f.buffer[f.status as usize] = b';';
                f.status += 1;
                flush_buffer(f, out);
            }
        }
    } else {
        // add character
        f.buffer[f.status as usize] = c as u8;
        f.status += 1;
        if !html_entity_char(c)
            || f.status + 1 == HTML_ENC_BUFFER_SIZE
            || (c == b'#' as i32 && f.status > 2)
        {
            // illegal character or end of buffer
            if c == b'&' as i32 {
                f.status -= 1;
            }
            flush_buffer(f, out);
            if c == b'&' as i32 {
                f.buffer[0] = b'&';
                f.status = 1;
            }
        }
    }
}

/// mbfl_filt_conv_html_dec_flush
fn html_dec_flush(f: &mut Filter, out: &mut Vec<i32>) {
    let status = f.status;
    f.status = 0;
    for pos in 0..status.max(0) as usize {
        out.push(f.buffer[pos] as i32);
    }
}

/// mbfl_filt_conv_html_enc
fn html_enc(c: i32, f: &mut Filter, out: &mut Vec<u8>) {
    let _ = f;
    // (`c < sizeof(htmlentitifieds)` compares unsigned: a negative c is
    // not in the table)
    if (0..256).contains(&c) && super::tables::filt_htmlent::htmlentitifieds[c as usize] != 1 {
        out.push(c as u8);
        return;
    }
    out.push(b'&');
    match super::transfer::HTML_ENTITY_LIST
        .iter()
        .find(|(_, code)| *code as i32 == c)
    {
        Some((name, _)) => out.extend_from_slice(name.as_bytes()),
        None => {
            out.push(b'#');
            out.extend_from_slice((c as u32).to_string().as_bytes());
        }
    }
    out.push(b';');
}

/* ===== mbfl_strcut ===== */

/// The two filters with the memory device the second one writes to (its
/// length; restoring a state truncates it).
#[derive(Clone)]
struct State {
    to_w: Filter,
    from_w: Filter,
    pos: usize,
}

struct Chain {
    tk: ToWchar,
    fk: FromWchar,
    st: State,
    dev: Vec<u8>,
    wbuf: Vec<i32>,
}

impl Chain {
    /// Feed a byte through both filters.
    fn feed(&mut self, b: u8) {
        self.wbuf.clear();
        to_wchar(self.tk, b as i32, &mut self.st.to_w, &mut self.wbuf);
        for i in 0..self.wbuf.len() {
            let w = self.wbuf[i];
            from_wchar(self.fk, w, &mut self.st.from_w, &mut self.dev);
        }
        self.st.pos = self.dev.len();
    }

    /// (*encoder->filter_flush)(encoder): the first filter's flush, which
    /// ends with the second's.
    fn flush(&mut self) {
        self.wbuf.clear();
        to_wchar_flush(self.tk, &mut self.st.to_w, &mut self.wbuf);
        for i in 0..self.wbuf.len() {
            let w = self.wbuf[i];
            from_wchar(self.fk, w, &mut self.st.from_w, &mut self.dev);
        }
        from_wchar_flush(self.fk, &mut self.st.from_w, &mut self.dev);
        self.st.pos = self.dev.len();
    }

    fn save(&self) -> State {
        self.st.clone()
    }

    fn restore(&mut self, s: &State) {
        self.st = s.clone();
        self.dev.truncate(s.pos);
    }
}

/// mbfl_strcut() through the legacy filters, for the encodings that have
/// them here (None otherwise).
pub fn strcut(s: &[u8], from: usize, length: usize, no: EncNo) -> Option<Vec<u8>> {
    let (tk, fk) = filters_for(no)?;
    let from = from.min(s.len());
    let mut ch = Chain {
        tk,
        fk,
        st: State {
            to_w: Filter::new(),
            from_w: Filter::new(),
            pos: 0,
        },
        dev: Vec::with_capacity(length + 8),
        wbuf: Vec::new(),
    };
    // Search the start position (the first filter's output is dropped).
    let mut sink = Vec::new();
    for &b in &s[..from] {
        sink.clear();
        to_wchar(tk, b as i32, &mut ch.st.to_w, &mut sink);
    }
    let q = s.len();
    let mut p = from;
    let mut bk_ = (ch.save(), p);
    let length = length.min(q - p);
    let mut bk;
    if length >= 20 {
        // output a little shorter than "length"
        let r = p + length - 20;
        while p < r {
            ch.feed(s[p]);
            p += 1;
        }
        if ch.st.pos > length {
            ch.restore(&bk_.0);
            p = bk_.1;
            bk = bk_.clone();
        } else {
            bk = (ch.save(), p);
            ch.flush();
            if ch.st.pos > length {
                ch.restore(&bk_.0);
                p = bk_.1;
                bk = bk_.clone();
            } else {
                ch.restore(&bk.0);
                p = bk.1;
            }
        }
    } else {
        bk = bk_.clone();
    }
    // detect the end position
    while p < q {
        ch.feed(s[p]);
        if ch.st.pos > length {
            ch.restore(&bk.0);
            p = bk.1;
            break;
        }
        p += 1;
        bk_ = (ch.save(), p);
        ch.flush();
        if ch.st.pos > length {
            ch.restore(&bk.0);
            p = bk.1;
            break;
        }
        ch.restore(&bk_.0);
        p = bk_.1;
        bk = bk_.clone();
    }
    let _ = p;
    ch.st.from_w.illegal_mode = MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE;
    ch.flush();
    Some(ch.dev)
}
