//! Shift-JIS variants (C: mbfilter_cjk.c, "SJIS variants" section):
//! SJIS, SJIS-mac, SJIS-Mobile#DOCOMO/KDDI/SOFTBANK, SJIS-2004, CP932, SJIS-win

use super::common::*;
use super::mobile::*;
use super::tables::emoji2uni::*;
use super::tables::filt_cjk::{mblen_table_sjis, mblen_table_sjis_mobile, mblen_table_sjismac, mblen_table_sjiswin, sjis_decode_tbl1, sjis_decode_tbl2, sjis_mobile_decode_tbl1};
use super::tables::sjis_mac2uni::*;
use super::tables::unicode_table_cp932_ext::*;
use super::tables::unicode_table_jis::*;
use super::tables::unicode_table_jis2004::*;
use super::*;

#[inline]
fn tbl_sum(t1: &[u16; 256], c: u8, c2: u8) -> u32 {
    t1[c as usize] as u32 + sjis_decode_tbl2[c2 as usize] as u32
}

#[inline]
fn emit_sjis_kanji(buf: &mut ConvBuf, s: u32) {
    let (s1, s2) = sjis_encode((s >> 8) & 0xFF, s & 0xFF);
    buf.add2(s1 as u8, s2 as u8);
}

/// Special mappings for the first 138 JIS X 0208 cells (row 1-2) shared by
/// the CP932-family decoders
#[inline]
fn cp932_row12(s: u32) -> u32 {
    if s <= 137 {
        match s {
            31 => 0xFF3C, /* FULLWIDTH REVERSE SOLIDUS */
            32 => 0xFF5E, /* FULLWIDTH TILDE */
            33 => 0x2225, /* PARALLEL TO */
            60 => 0xFF0D, /* FULLWIDTH HYPHEN-MINUS */
            80 => 0xFFE0, /* FULLWIDTH CENT SIGN */
            81 => 0xFFE1, /* FULLWIDTH POUND SIGN */
            137 => 0xFFE2, /* FULLWIDTH NOT SIGN */
            _ => 0,
        }
    } else {
        0
    }
}

/// Reverse of cp932_row12 for the Microsoft-specific mappings
#[inline]
fn cp932_special_reverse(w: u32) -> u32 {
    match w {
        0xFF3C => 0x2140,
        0x2225 => 0x2142,
        0xFF0D => 0x215D,
        0xFFE0 => 0x2171,
        0xFFE1 => 0x2172,
        0xFFE2 => 0x224C,
        _ => 0,
    }
}

/* ===== SJIS ===== */

fn mb_sjis_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    if input.is_empty() {
        return;
    }
    let mut p = 0usize;
    let e = input.len() - 1; /* Stop the main loop 1 byte short of the end of the input */
    while p < e {
        let c = input[p];
        p += 1;
        if c <= 0x7F {
            out.push(c as u32);
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else {
            let c2 = input[p];
            p += 1;
            let w = tbl_sum(&sjis_decode_tbl1, c, c2);
            if w < jisx0208_ucs_table_size as u32 {
                let w = jisx0208_ucs_table[w as usize] as u32;
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                if c == 0x80 || c == 0xA0 || c > 0xEF {
                    p -= 1;
                }
                out.push(MBFL_BAD_INPUT);
            }
        }
    }
    /* Finish up last byte of input string if there is one */
    if p == e {
        let c = input[p];
        if c <= 0x7F {
            out.push(c as u32);
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_sjis(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s = ucs_to_jis(w);
        if s == 0 {
            if w == 0xA5 {
                s = 0x216F;
            } else if w == 0xAF || w == 0x203E {
                s = 0x2131;
            } else if w == 0xFF3C {
                s = 0x2140;
            } else if w == 0x2225 {
                s = 0x2142;
            } else if w == 0xFF0D {
                s = 0x215D;
            } else if w == 0xFFE0 {
                s = 0x2171;
            } else if w == 0xFFE1 {
                s = 0x2172;
            } else if w == 0xFFE2 {
                s = 0x224C;
            } else if w != 0 {
                mb_illegal_output(w, mb_wchar_to_sjis, buf);
                continue;
            }
        } else if s >= 0x8080 {
            mb_illegal_output(w, mb_wchar_to_sjis, buf);
            continue;
        }
        if s <= 0xFF {
            buf.add(s as u8);
        } else {
            emit_sjis_kanji(buf, s);
        }
    }
}

pub static ENC_SJIS: Encoding = Encoding {
    no: EncNo::Sjis,
    name: "SJIS",
    mime_name: Some("Shift_JIS"),
    aliases: &["x-sjis", "SHIFT-JIS"],
    mblen_table: Some(&mblen_table_sjis),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_sjis_to_wchar),
    from_wchar: Some(mb_wchar_to_sjis),
    check: None,
    cut: None,
};

/* ===== SJIS-mac (MacJapanese) ===== */

fn mb_sjismac_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    'outer: while p < e {
        let c = input[p];
        p += 1;
        if c <= 0x80 || c == 0xA0 {
            if c == 0x5C {
                out.push(0xA5);
            } else if c == 0x80 {
                out.push(0x5C);
            } else {
                out.push(c as u32);
            }
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else if c <= 0xED {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            let w = tbl_sum(&sjis_decode_tbl1, c, c2);

            if w <= 0x89 {
                let r = match w {
                    0x1C => 0x2014,
                    0x1F => 0xFF3C,
                    0x20 => 0x301C,
                    0x21 => 0x2016,
                    0x3C => 0x2212,
                    0x50 => 0xA2,
                    0x51 => 0xA3,
                    0x89 => 0xAC,
                    _ => 0,
                };
                if r != 0 {
                    out.push(r);
                    continue;
                }
            } else {
                if (0x2F0..=0x3A3).contains(&w) {
                    for i in 0..7 {
                        if w >= code_tbl[i][0] as u32 && w <= code_tbl[i][1] as u32 {
                            out.push(w - code_tbl[i][0] as u32 + code_tbl[i][2] as u32);
                            continue 'outer;
                        }
                    }
                }
                if (0x340..=0x523).contains(&w) {
                    for i in 0..code_tbl_m_len as usize {
                        if w == code_tbl_m[i][0] as u32 {
                            let n = if code_tbl_m[i][1] == 0xF860 {
                                3
                            } else if code_tbl_m[i][1] == 0xF861 {
                                4
                            } else {
                                5
                            };
                            for j in 1..=n {
                                out.push(code_tbl_m[i][j] as u32);
                            }
                            continue 'outer;
                        }
                    }
                }
                if (0x3AC..=0x20A5).contains(&w) {
                    for i in 0..8 {
                        if w >= code_ofst_tbl[i][0] as u32 && w <= code_ofst_tbl[i][1] as u32 {
                            let w2 = code_map[i][(w - code_ofst_tbl[i][0] as u32) as usize] as u32;
                            if w2 == 0 {
                                out.push(MBFL_BAD_INPUT);
                                continue 'outer;
                            }
                            out.push(w2);
                            if (0x43E..=0x441).contains(&w) {
                                out.push(0xF87A);
                            } else if w == 0x3B1 || w == 0x3B7 {
                                out.push(0xF87F);
                            } else if w == 0x4B8 || w == 0x4B9 || w == 0x4C4 {
                                out.push(0x20DD);
                            } else if w == 0x1ED9
                                || w == 0x1EDA
                                || w == 0x1EE8
                                || w == 0x1EF3
                                || (0x1EF5..=0x1EFB).contains(&w)
                                || w == 0x1F05
                                || w == 0x1F06
                                || w == 0x1F18
                                || (0x1FF2..=0x20A5).contains(&w)
                            {
                                out.push(0xF87E);
                            }
                            continue 'outer;
                        }
                    }
                }
            }

            if w < jisx0208_ucs_table_size as u32 {
                let w = jisx0208_ucs_table[w as usize] as u32;
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c == 0xFD {
            out.push(0xA9);
        } else if c == 0xFE {
            out.push(0x2122);
        } else if c == 0xFF {
            out.push(0x2026);
            out.push(0xF87F);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn process_s_form(w: u32, w2: u32, s: &mut u32) -> bool {
    let (off, n) = match w2 {
        0xF87A => (34 + 3 + 3, 4),
        0x20DD => (34 + 3, 3),
        0xF87F => (34, 3),
        0xF87E => (0, 34),
        _ => return false,
    };
    for i in 0..n {
        if w == s_form_tbl[i + off] as u32 {
            *s = s_form_sjis_tbl[i + off] as u32;
            return true;
        }
    }
    false
}

/* For codepoints F860-F862, which are treated specially in MacJapanese */
const TRANSCODING_HINT_CP_WIDTH: [usize; 3] = [3, 4, 5];

enum HintResult {
    Return,
    Next,
}

/// Inner loop for Apple 'transcoding hint' sequences (resumable)
fn sjismac_hint(ti: usize, mut index: usize, w: u32, input: &[u32], i: &mut usize, buf: &mut ConvBuf, end: bool) -> HintResult {
    let expected = TRANSCODING_HINT_CP_WIDTH[(w - 0xF860) as usize];
    while index <= expected {
        if *i >= input.len() {
            if end {
                for j in 1..index {
                    mb_illegal_output(code_tbl_m[ti][j] as u32, mb_wchar_to_sjismac, buf);
                }
            } else {
                buf.state = ((ti as u32) << 24) | ((index as u32) << 16) | (w & 0xFFFF);
            }
            return HintResult::Return;
        }
        let w2 = input[*i];
        *i += 1;
        if w2 != code_tbl_m[ti][index] as u32 {
            for j in 1..index {
                mb_illegal_output(code_tbl_m[ti][j] as u32, mb_wchar_to_sjismac, buf);
            }
            mb_illegal_output(w2, mb_wchar_to_sjismac, buf);
            return HintResult::Next;
        }
        index += 1;
    }
    /* Successful match, emit SJIS-mac bytes */
    let s = code_tbl_m[ti][0] as u32;
    let (s1, s2) = sjis_encode((s / 94) + 0x21, (s % 94) + 0x21);
    buf.add2(s1 as u8, s2 as u8);
    HintResult::Next
}

fn mb_wchar_to_sjismac(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let len = input.len();
    let mut i = 0usize;
    let mut pending: Option<u32> = None;

    if buf.state != 0 {
        let w = buf.state & 0xFFFF;
        if buf.state & 0xFF00_0000 != 0 {
            let ti = (buf.state >> 24) as usize;
            let index = ((buf.state >> 16) & 0xFF) as usize;
            buf.state = 0;
            if let HintResult::Return = sjismac_hint(ti, index, w, input, &mut i, buf, end) {
                return;
            }
        } else {
            buf.state = 0;
            pending = Some(w);
        }
    }

    'outer: loop {
        let w = match pending.take() {
            Some(w) => w,
            None => {
                if i >= len {
                    break;
                }
                i += 1;
                input[i - 1]
            }
        };
        let mut s: u32 = 0;
        let wi = w as i64;

        if wi >= ucs_a1_jis_table_min as i64 && wi < ucs_a1_jis_table_max as i64 {
            if w == 0x5C {
                s = 0x80;
            } else if w == 0xA9 {
                s = 0xFD;
            } else {
                s = ucs_a1_jis_table[(w - ucs_a1_jis_table_min as u32) as usize] as u32;
            }
        } else if wi >= ucs_a2_jis_table_min as i64 && wi < ucs_a2_jis_table_max as i64 {
            if w == 0x2122 {
                s = 0xFE;
            } else if w == 0x2014 {
                s = 0x213D;
            } else if w == 0x2116 {
                s = 0x2C1D;
            } else {
                s = ucs_a2_jis_table[(w - ucs_a2_jis_table_min as u32) as usize] as u32;
            }
        } else if wi >= ucs_i_jis_table_min as i64 && wi < ucs_i_jis_table_max as i64 {
            s = ucs_i_jis_table[(w - ucs_i_jis_table_min as u32) as usize] as u32;
        } else if wi >= ucs_r_jis_table_min as i64 && wi < ucs_r_jis_table_max as i64 {
            s = ucs_r_jis_table[(w - ucs_r_jis_table_min as u32) as usize] as u32;
        }

        if w >= 0x2000 {
            for k in 0..s_form_tbl_len as usize {
                if w == s_form_tbl[k] as u32 {
                    if i >= len {
                        if end {
                            let s = s_form_sjis_fallback_tbl[k] as u32;
                            if s != 0 {
                                buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
                            } else {
                                mb_illegal_output(w, mb_wchar_to_sjismac, buf);
                            }
                        } else {
                            buf.state = w;
                        }
                        return;
                    }
                    let w2 = input[i];
                    i += 1;
                    if !process_s_form(w, w2, &mut s) {
                        i -= 1;
                        for kk in 0..s_form_tbl_len as usize {
                            if w == s_form_tbl[kk] as u32 {
                                s = s_form_sjis_fallback_tbl[kk] as u32;
                                break;
                            }
                        }
                    }
                    if s <= 0xFF {
                        buf.add(s as u8);
                    } else {
                        buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
                    }
                    continue 'outer;
                }
            }

            if w == 0xF860 || w == 0xF861 || w == 0xF862 {
                /* Apple 'transcoding hint' codepoints (from private use area) */
                if i >= len {
                    if end {
                        mb_illegal_output(w, mb_wchar_to_sjismac, buf);
                    } else {
                        buf.state = w;
                    }
                    return;
                }
                let w2 = input[i];
                i += 1;
                for ti in 0..code_tbl_m_len as usize {
                    if w == code_tbl_m[ti][1] as u32 && w2 == code_tbl_m[ti][2] as u32 {
                        match sjismac_hint(ti, 3, w, input, &mut i, buf, end) {
                            HintResult::Return => return,
                            HintResult::Next => continue 'outer,
                        }
                    }
                }
                /* No valid transcoding hint sequence found */
                i -= 1;
                mb_illegal_output(w, mb_wchar_to_sjismac, buf);
                continue;
            }
        }

        if s == 0 {
            if w == 0xA0 {
                s = 0xA0;
            } else if w == 0xA5 {
                s = 0x5C;
            } else if w == 0xFF3C {
                s = 0x2140;
            } else {
                'found: {
                    for k in 0..wchar2sjis_mac_r_tbl_len as usize {
                        let r = &wchar2sjis_mac_r_tbl[k];
                        if w >= r[0] as u32 && w <= r[1] as u32 {
                            let t = w - r[0] as u32 + r[2] as u32;
                            s = (((t / 94) + 0x21) << 8) | ((t % 94) + 0x21);
                            break 'found;
                        }
                    }
                    for k in 0..wchar2sjis_mac_r_map_len as usize {
                        let r = &wchar2sjis_mac_r_map[k];
                        if w >= r[0] as u32 && w <= r[1] as u32 {
                            let t = wchar2sjis_mac_code_map[k][(w - r[0] as u32) as usize] as u32;
                            if t != 0 {
                                s = (((t / 94) + 0x21) << 8) | ((t % 94) + 0x21);
                                break 'found;
                            }
                        }
                    }
                    for k in 0..wchar2sjis_mac_wchar_tbl_len as usize {
                        if w == wchar2sjis_mac_wchar_tbl[k][0] as u32 {
                            let t = wchar2sjis_mac_wchar_tbl[k][1] as u32;
                            s = (((t / 94) + 0x21) << 8) | ((t % 94) + 0x21);
                            break 'found;
                        }
                    }
                }
            }
        }

        if (s == 0 && w != 0) || s >= 0x8080 {
            mb_illegal_output(w, mb_wchar_to_sjismac, buf);
        } else if s <= 0xFF {
            buf.add(s as u8);
        } else {
            emit_sjis_kanji(buf, s);
        }
    }
}

pub static ENC_SJIS_MAC: Encoding = Encoding {
    no: EncNo::SjisMac,
    name: "SJIS-mac",
    mime_name: Some("Shift_JIS"),
    aliases: &["MacJapanese", "x-Mac-Japanese"],
    mblen_table: Some(&mblen_table_sjismac),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_sjismac_to_wchar),
    from_wchar: Some(mb_wchar_to_sjismac),
    check: None,
    cut: None,
};

/* ===== SJIS-Mobile ===== */

#[derive(Clone, Copy, PartialEq)]
enum Vendor {
    Docomo,
    Kddi,
    Sb,
}

/// Common tail of the mobile SJIS decoders for a kuten code `w` which is
/// not one of the vendor's emoji. Returns None for the "invalid" branch.
#[inline]
fn mobile_kuten(w: u32) -> Option<u32> {
    if w >= cp932ext1_ucs_table_min as u32 && w < cp932ext1_ucs_table_max as u32 {
        Some(cp932ext1_ucs_table[(w - cp932ext1_ucs_table_min as u32) as usize] as u32)
    } else if w < jisx0208_ucs_table_size as u32 {
        Some(jisx0208_ucs_table[w as usize] as u32)
    } else if w >= cp932ext2_ucs_table_min as u32 && w < cp932ext2_ucs_table_max as u32 {
        Some(cp932ext2_ucs_table[(w - cp932ext2_ucs_table_min as u32) as usize] as u32)
    } else if w >= cp932ext3_ucs_table_min as u32 && w < cp932ext3_ucs_table_max as u32 {
        Some(cp932ext3_ucs_table[(w - cp932ext3_ucs_table_min as u32) as usize] as u32)
    } else if w >= 94 * 94 && w < 114 * 94 {
        Some(w - (94 * 94) + 0xE000)
    } else {
        None
    }
}

fn sjis_mobile_kanji(vendor: Vendor, c: u8, c2: u8, p: &mut usize, out: &mut Vec<u32>) {
    let mut w = tbl_sum(&sjis_mobile_decode_tbl1, c, c2);
    let r = cp932_row12(w);
    if r != 0 {
        out.push(r);
        return;
    }
    let mut snd = 0i32;
    match vendor {
        Vendor::Docomo => {
            if w >= mb_tbl_code2uni_docomo1_min as u32 && w <= mb_tbl_code2uni_docomo1_max as u32 {
                w = mbfilter_sjis_emoji_docomo2unicode(w as i32, &mut snd) as u32;
                if snd != 0 {
                    out.push(snd as u32);
                }
                out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
                return;
            }
        }
        Vendor::Kddi => {
            if w >= mb_tbl_code2uni_kddi1_min as u32 && w <= mb_tbl_code2uni_kddi2_max as u32 {
                w = mbfilter_sjis_emoji_kddi2unicode(w as i32, &mut snd) as u32;
                if w == 0 {
                    w = tbl_sum(&sjis_mobile_decode_tbl1, c, c2);
                    if w >= 94 * 94 && w < 114 * 94 {
                        w = w - (94 * 94) + 0xE000;
                    }
                } else if snd != 0 {
                    out.push(snd as u32);
                }
                out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
                return;
            }
        }
        Vendor::Sb => {
            if w >= mb_tbl_code2uni_sb1_min as u32 && w <= mb_tbl_code2uni_sb3_max as u32 {
                w = mbfilter_sjis_emoji_sb2unicode(w as i32, &mut snd) as u32;
                if w == 0 {
                    w = tbl_sum(&sjis_mobile_decode_tbl1, c, c2);
                    if w >= cp932ext3_ucs_table_min as u32 && w < cp932ext3_ucs_table_max as u32 {
                        w = cp932ext3_ucs_table[(w - cp932ext3_ucs_table_min as u32) as usize] as u32;
                    } else if w >= 94 * 94 && w < 114 * 94 {
                        w = w - (94 * 94) + 0xE000;
                    }
                } else if snd != 0 {
                    out.push(snd as u32);
                }
                out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
                return;
            }
        }
    }
    match mobile_kuten(w) {
        Some(w) => out.push(if w != 0 { w } else { MBFL_BAD_INPUT }),
        None => {
            if c == 0x80 || c == 0xA0 || c >= 0xFD {
                *p -= 1;
            }
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn sjis_mobile_to_wchar(vendor: Vendor, input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;

    if *state != 0 {
        sb_escapes(input, &mut p, out, state);
    }

    while p < e {
        let c = input[p];
        p += 1;
        if vendor == Vendor::Sb && c == 0x1B {
            /* Escape sequence */
            if p == e || {
                let x = input[p];
                p += 1;
                x != b'$'
            } || p == e
            {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            let c2 = input[p];
            p += 1;
            if !(b'E'..=b'G').contains(&c2) && !(b'O'..=b'Q').contains(&c2) {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            *state = c2 as u32;
            sb_escapes(input, &mut p, out, state);
        } else if c <= 0x7F {
            out.push(c as u32);
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            sjis_mobile_kanji(vendor, c, c2, &mut p, out);
        }
    }
}

/// Series of specially encoded Softbank emoji after ESC $ [EFGOPQ]
fn sb_escapes(input: &[u8], p: &mut usize, out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    while *p < e {
        let c = input[*p];
        *p += 1;
        if c == 0xF {
            *state = 0;
            break;
        }
        let st = *state as u8;
        let s: u32 = if st == b'G' && (0x21..=0x7A).contains(&c) {
            (0x91 - 0x21) * 94
        } else if st == b'E' && (0x21..=0x7A).contains(&c) {
            (0x8D - 0x21) * 94
        } else if st == b'F' && (0x21..=0x7A).contains(&c) {
            (0x8E - 0x21) * 94
        } else if st == b'O' && (0x21..=0x6D).contains(&c) {
            (0x92 - 0x21) * 94
        } else if st == b'P' && (0x21..=0x6C).contains(&c) {
            (0x95 - 0x21) * 94
        } else if st == b'Q' && (0x21..=0x5E).contains(&c) {
            (0x96 - 0x21) * 94
        } else {
            out.push(MBFL_BAD_INPUT);
            *state = 0;
            break;
        };
        let mut snd = 0;
        let w = mbfilter_sjis_emoji_sb2unicode((s + c as u32 - 0x21) as i32, &mut snd);
        if w != 0 {
            if snd != 0 {
                out.push(snd as u32);
            }
            out.push(w as u32);
        } else {
            out.push(MBFL_BAD_INPUT);
            *state = 0;
            break;
        }
    }
}

fn mb_sjis_docomo_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    sjis_mobile_to_wchar(Vendor::Docomo, input, out, state)
}
fn mb_sjis_kddi_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    sjis_mobile_to_wchar(Vendor::Kddi, input, out, state)
}
fn mb_sjis_sb_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    sjis_mobile_to_wchar(Vendor::Sb, input, out, state)
}

/// Common first part of the mobile SJIS encoders (up to `process_emoji`)
#[inline]
fn mobile_prefix(w: u32) -> u32 {
    let mut s = ucs_to_jis(w);
    if s == 0 && w >= 0xE000 && w < 0xE000 + 20 * 94 {
        /* Private User Area (95ku - 114ku) */
        let t = w - 0xE000;
        return (((t / 94) + 0x7F) << 8) | ((t % 94) + 0x21);
    }
    if s == 0 {
        s = if w == 0xA5 { 0x216F } else { cp932_special_reverse(w) };
    }
    if w != 0 && (s == 0 || s >= 0x8080) {
        s = 0;
        for i in 0..(cp932ext1_ucs_table_max - cp932ext1_ucs_table_min) as usize {
            if w == cp932ext1_ucs_table[i] as u32 {
                return ((((i / 94) + 0x2D) << 8) + (i % 94) + 0x21) as u32;
            }
        }
        for i in 0..(cp932ext2_ucs_table_max - cp932ext2_ucs_table_min) as usize {
            if w == cp932ext2_ucs_table[i] as u32 {
                return ((((i / 94) + 0x79) << 8) + (i % 94) + 0x21) as u32;
            }
        }
    }
    s
}

#[inline]
fn kuten_to_jis(t: u32) -> u32 {
    (((t / 94) + 0x21) << 8) | ((t % 94) + 0x21)
}

fn wchar_to_sjis_mobile(vendor: Vendor, input: &[u32], buf: &mut ConvBuf, end: bool, self_fn: FromWcharFn) {
    let len = input.len();
    let mut i = 0usize;
    let mut pending: Option<u32> = None;
    if buf.state != 0 {
        pending = Some(buf.state);
        buf.state = 0;
    }
    let tables = match vendor {
        Vendor::Docomo => docomo_tables(),
        Vendor::Kddi => kddi_tables(),
        Vendor::Sb => sb_tables(),
    };
    let (keypad_hash, keypad0, keypad1, copyright, registered) = match vendor {
        Vendor::Docomo => (0x2964, 0x296F, 0x2966, 0x29B5, 0x29BA),
        Vendor::Kddi => (0x25BC, 0x2830, 0x27A6, 0x27DC, 0x27DD),
        Vendor::Sb => (0x2817, 0x282C, 0x2823, 0x2855, 0x2856),
    };

    'main: loop {
        let w = match pending.take() {
            Some(w) => w,
            None => {
                if i >= len {
                    break;
                }
                i += 1;
                input[i - 1]
            }
        };
        let mut s = mobile_prefix(w);

        /* process_emoji */
        'emit: {
            if w == '#' as u32 || (w >= '0' as u32 && w <= '9' as u32) {
                if i >= len {
                    if end {
                        break 'emit;
                    } else {
                        buf.state = w;
                        return;
                    }
                }
                let w2 = input[i];
                i += 1;
                if w2 == 0x20E3 {
                    let t = if w == '#' as u32 {
                        keypad_hash
                    } else if w == '0' as u32 {
                        keypad0
                    } else {
                        keypad1 + (w - '1' as u32)
                    };
                    s = kuten_to_jis(t);
                } else {
                    i -= 1;
                }
            } else if vendor != Vendor::Docomo && w >= nflags(b'C') && w <= nflags(b'U') {
                if i >= len {
                    if end {
                        mb_illegal_output(w, self_fn, buf);
                    } else {
                        buf.state = w;
                    }
                    return;
                }
                let w2 = input[i];
                i += 1;
                if w2 >= nflags(b'B') && w2 <= nflags(b'U') {
                    for k in 0..10 {
                        if w == nflags(nflags_s[k][0]) && w2 == nflags(nflags_s[k][1]) {
                            let code = if vendor == Vendor::Kddi { nflags_code_kddi[k] } else { nflags_code_sb[k] };
                            s = kuten_to_jis(code as u32);
                            break 'emit;
                        }
                    }
                }
                i -= 1;
                mb_illegal_output(w, self_fn, buf);
                continue 'main;
            } else if w == 0xA9 {
                s = kuten_to_jis(copyright);
            } else if w == 0xAE {
                s = kuten_to_jis(registered);
            } else {
                let t = emoji_lookup3(w, &tables);
                if t != 0 {
                    s = kuten_to_jis(t);
                }
            }
        }

        /* emit_output */
        if s == 0 && w != 0 {
            mb_illegal_output(w, self_fn, buf);
        } else if s <= 0xFF {
            buf.add(s as u8);
        } else {
            emit_sjis_kanji(buf, s);
        }
    }
}

fn mb_wchar_to_sjis_docomo(input: &[u32], buf: &mut ConvBuf, end: bool) {
    wchar_to_sjis_mobile(Vendor::Docomo, input, buf, end, mb_wchar_to_sjis_docomo)
}
fn mb_wchar_to_sjis_kddi(input: &[u32], buf: &mut ConvBuf, end: bool) {
    wchar_to_sjis_mobile(Vendor::Kddi, input, buf, end, mb_wchar_to_sjis_kddi)
}
fn mb_wchar_to_sjis_sb(input: &[u32], buf: &mut ConvBuf, end: bool) {
    wchar_to_sjis_mobile(Vendor::Sb, input, buf, end, mb_wchar_to_sjis_sb)
}

pub static ENC_SJIS_DOCOMO: Encoding = Encoding {
    no: EncNo::SjisDocomo,
    name: "SJIS-Mobile#DOCOMO",
    mime_name: Some("Shift_JIS"),
    aliases: &["SJIS-DOCOMO", "shift_jis-imode", "x-sjis-emoji-docomo"],
    mblen_table: Some(&mblen_table_sjis_mobile),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_sjis_docomo_to_wchar),
    from_wchar: Some(mb_wchar_to_sjis_docomo),
    check: None,
    cut: None,
};

pub static ENC_SJIS_KDDI: Encoding = Encoding {
    no: EncNo::SjisKddi,
    name: "SJIS-Mobile#KDDI",
    mime_name: Some("Shift_JIS"),
    aliases: &["SJIS-KDDI", "shift_jis-kddi", "x-sjis-emoji-kddi"],
    mblen_table: Some(&mblen_table_sjis_mobile),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_sjis_kddi_to_wchar),
    from_wchar: Some(mb_wchar_to_sjis_kddi),
    check: None,
    cut: None,
};

pub static ENC_SJIS_SB: Encoding = Encoding {
    no: EncNo::SjisSb,
    name: "SJIS-Mobile#SOFTBANK",
    mime_name: Some("Shift_JIS"),
    aliases: &["SJIS-SOFTBANK", "shift_jis-softbank", "x-sjis-emoji-softbank"],
    mblen_table: Some(&mblen_table_sjis_mobile),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_sjis_sb_to_wchar),
    from_wchar: Some(mb_wchar_to_sjis_sb),
    check: None,
    cut: None,
};

/* ===== SJIS-2004 ===== */

fn mb_sjis2004_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c <= 0x7F {
            if c == 0x5C {
                out.push(0xA5);
            } else if c == 0x7E {
                out.push(0x203E);
            } else {
                out.push(c as u32);
            }
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            let w1 = tbl_sum(&sjis_mobile_decode_tbl1, c, c2);

            /* Conversion for combining characters */
            if (0x0170..=0x03F1).contains(&w1) {
                let k = mbfl_bisec_srch2(w1 as i32, &jisx0213_u2_key_b, jisx0213_u2_tbl_len);
                if k >= 0 {
                    out.push(jisx0213_u2_tbl[2 * k as usize] as u32);
                    out.push(jisx0213_u2_tbl[2 * k as usize + 1] as u32);
                    continue;
                }
            }

            /* Conversion for BMP */
            if w1 < jisx0213_ucs_table_size as u32 {
                let w = jisx0213_ucs_table[w1 as usize] as u32;
                if w != 0 {
                    out.push(w);
                    continue;
                }
            }

            /* Conversion for CJK Unified Ideographs extension B (U+2XXXX) */
            let k = mbfl_bisec_srch2(w1 as i32, &jisx0213_jis_u5_key, jisx0213_u5_tbl_len);
            if k >= 0 {
                out.push(jisx0213_jis_u5_tbl[k as usize] as u32 + 0x20000);
            } else {
                if c == 0x80 || c == 0xA0 || c >= 0xFD {
                    p -= 1;
                }
                out.push(MBFL_BAD_INPUT);
            }
        }
    }
}

pub enum X0213 {
    Code(u32),
    Suspend,
}

/// Unicode -> JIS X 0213 lookup shared by SJIS-2004 and EUC-JP-2004
/// encoders; may consume a following combining codepoint.
pub fn ucs_to_jisx0213(w: u32, input: &[u32], i: &mut usize, end: bool) -> X0213 {
    let mut s: u32 = 0;

    /* Check for 1st char of combining characters */
    if w == 0xE6 || (0x254..=0x2E9).contains(&w) || (0x304B..=0x3053).contains(&w) || (0x30AB..=0x30C8).contains(&w) || w == 0x31F7 {
        let n = jisx0213_u2_tbl_len as usize;
        let mut k = 0usize;
        while k < n {
            if w == jisx0213_u2_tbl[2 * k] as u32 {
                if *i >= input.len() {
                    if !end {
                        return X0213::Suspend;
                    }
                } else {
                    let w2 = input[*i];
                    *i += 1;
                    if (w == 0x254 || w == 0x28C || w == 0x259 || w == 0x25A) && w2 == 0x301 {
                        k += 1;
                    }
                    if jisx0213_u2_tbl.get(2 * k + 1).map(|&x| x as u32) == Some(w2) {
                        s = jisx0213_u2_key[k] as u32;
                        break;
                    }
                    *i -= 1;
                }
                /* Fallback */
                s = jisx0213_u2_fb_tbl.get(k).copied().unwrap_or(0) as u32;
                break;
            }
            k += 1;
        }
    }

    /* Check for major Japanese chars: U+4E00-U+9FFF */
    if s == 0 {
        for k in 0..uni2jis_tbl_len as usize {
            let r = uni2jis_tbl_range[k];
            if w as i64 >= r[0] as i64 && w as i64 <= r[1] as i64 {
                s = uni2jis_tbl[k][(w - r[0] as u32) as usize] as u32;
                break;
            }
        }
    }

    /* Check for Japanese chars in compressed mapping area: U+1E00-U+4DBF */
    if s == 0 && w >= ucs_c1_jisx0213_min as u32 && w <= ucs_c1_jisx0213_max as u32 {
        let k = mbfl_bisec_srch(w as i32, &ucs_c1_jisx0213_tbl, ucs_c1_jisx0213_tbl_len);
        if k >= 0 {
            s = ucs_c1_jisx0213_ofst[k as usize] as u32 + w - ucs_c1_jisx0213_tbl[2 * k as usize] as u32;
        }
    }

    /* Check for Japanese chars in CJK Unified Ideographs ext.B (U+2XXXX) */
    if s == 0 && w >= jisx0213_u5_tbl_min as u32 && w <= jisx0213_u5_tbl_max as u32 {
        let k = mbfl_bisec_srch2(w as i32 - 0x20000, &jisx0213_u5_jis_key, jisx0213_u5_tbl_len);
        if k >= 0 {
            s = jisx0213_u5_jis_tbl[k as usize] as u32;
        }
    }

    if s == 0 {
        /* CJK Compatibility Forms: U+FE30-U+FE4F */
        if w == 0xFE45 {
            s = 0x233E;
        } else if w == 0xFE46 {
            s = 0x233D;
        } else if (0xF91D..=0xF9DC).contains(&w) {
            /* CJK Compatibility Ideographs: U+F900-U+F92A */
            let k = mbfl_bisec_srch2(w as i32, &ucs_r2b_jisx0213_cmap_key, ucs_r2b_jisx0213_cmap_len);
            if k >= 0 {
                s = ucs_r2b_jisx0213_cmap_val[k as usize] as u32;
            }
        }
    }

    X0213::Code(s)
}

fn mb_wchar_to_sjis2004(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let len = input.len();
    let mut i = 0usize;
    let mut pending: Option<u32> = None;
    if buf.state != 0 {
        pending = Some(buf.state);
        buf.state = 0;
    }
    loop {
        let w = match pending.take() {
            Some(w) => w,
            None => {
                if i >= len {
                    break;
                }
                i += 1;
                input[i - 1]
            }
        };
        let s = match ucs_to_jisx0213(w, input, &mut i, end) {
            X0213::Suspend => {
                buf.state = w;
                return;
            }
            X0213::Code(s) => s,
        };
        if s == 0 && w != 0 {
            mb_illegal_output(w, mb_wchar_to_sjis2004, buf);
        } else if s <= 0xFF {
            buf.add(s as u8);
        } else {
            emit_sjis_kanji(buf, s);
        }
    }
}

pub static ENC_SJIS2004: Encoding = Encoding {
    no: EncNo::Sjis2004,
    name: "SJIS-2004",
    mime_name: Some("Shift_JIS"),
    aliases: &["SJIS2004", "Shift_JIS-2004"],
    mblen_table: Some(&mblen_table_sjis_mobile),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_sjis2004_to_wchar),
    from_wchar: Some(mb_wchar_to_sjis2004),
    check: None,
    cut: None,
};

/* ===== CP932 / SJIS-win ===== */

fn mb_cp932_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if c > 0xA0 && c < 0xE0 {
            out.push(0xFEC0 + c as u32);
        } else {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            let s = tbl_sum(&sjis_mobile_decode_tbl1, c, c2);
            let mut w = cp932_row12(s);
            if w == 0 {
                w = mobile_kuten(s).unwrap_or(0);
            }
            if w == 0 {
                if c == 0x80 || c == 0xA0 || c >= 0xFD {
                    p -= 1;
                }
                w = MBFL_BAD_INPUT;
            }
            out.push(w);
        }
    }
}

fn wchar_to_cp932_common(input: &[u32], buf: &mut ConvBuf, sjiswin: bool) {
    for &w in input {
        let mut s1: u32;
        let mut s2: u32 = 0;
        let wi = w as i64;
        if wi >= ucs_a1_jis_table_min as i64 && wi < ucs_a1_jis_table_max as i64 {
            s1 = ucs_a1_jis_table[(w - ucs_a1_jis_table_min as u32) as usize] as u32;
        } else if !sjiswin && w == 0x203E {
            s1 = 0x7E;
        } else {
            s1 = ucs_to_jis(w);
            if s1 == 0 && w >= 0xE000 && w < 0xE000 + 20 * 94 {
                let t = w - 0xE000;
                s1 = ((t / 94 + 0x7F) << 8) | (t % 94 + 0x21);
                s2 = 1;
            }
        }

        if w == 0xA5 {
            s1 = if sjiswin { 0x216F } else { 0x5C };
        } else if w == 0 {
            buf.add(0);
            continue;
        } else {
            let r = cp932_special_reverse(w);
            if r != 0 {
                s1 = r;
            }
        }

        if s1 == 0 || (s1 >= 0x8080 && s2 == 0) {
            /* not found or X 0212 */
            if let Some(l) = mbfl_binary_search_paired_sorted_table(w, &cp932ext1_ucs_table_paired_sorted, cp932ext1_ucs_table_paired_sorted.len() as i32) {
                let l = l as u32;
                s1 = ((l / 94 + 0x2D) << 8) + (l % 94 + 0x21);
            } else if let Some(l) = mbfl_binary_search_paired_sorted_table(w, &cp932ext3_ucs_table_paired_sorted, cp932ext3_ucs_table_paired_sorted.len() as i32) {
                let l = l as u32;
                s1 = ((l / 94 + 0x93) << 8) + (l % 94 + 0x21);
            } else {
                mb_illegal_output(w, mb_wchar_to_cp932, buf);
                continue;
            }
        }

        if s1 < 0x100 {
            buf.add(s1 as u8);
        } else {
            emit_sjis_kanji(buf, s1);
        }
    }
}

fn mb_wchar_to_cp932(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    wchar_to_cp932_common(input, buf, false)
}

fn mb_wchar_to_sjiswin(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    wchar_to_cp932_common(input, buf, true)
}

pub static ENC_CP932: Encoding = Encoding {
    no: EncNo::Cp932,
    name: "CP932",
    mime_name: Some("Shift_JIS"),
    aliases: &["MS932", "Windows-31J", "MS_Kanji"],
    mblen_table: Some(&mblen_table_sjiswin),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_cp932_to_wchar),
    from_wchar: Some(mb_wchar_to_cp932),
    check: None,
    cut: None,
};

pub static ENC_SJISWIN: Encoding = Encoding {
    no: EncNo::SjisWin,
    name: "SJIS-win",
    mime_name: Some("Shift_JIS"),
    aliases: &["SJIS-ms", "SJIS-open"],
    mblen_table: Some(&mblen_table_sjiswin),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_cp932_to_wchar),
    from_wchar: Some(mb_wchar_to_sjiswin),
    check: None,
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[
    &ENC_SJIS,
    &ENC_SJIS_MAC,
    &ENC_SJIS_DOCOMO,
    &ENC_SJIS_KDDI,
    &ENC_SJIS_SB,
    &ENC_SJIS2004,
    &ENC_CP932,
    &ENC_SJISWIN,
];
