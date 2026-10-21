//! ISO-2022 variants (C: the ISO-2022 section of mbfilter_cjk.c):
//! JIS, ISO-2022-JP, ISO-2022-JP-2004, CP50220/1/2, ISO-2022-JP-MS,
//! ISO-2022-KR; plus `mb_convert_kana_codepoint`.

use std::sync::OnceLock;

use super::common::{mbfl_bisec_srch, mbfl_bisec_srch2};
use super::tables::filt_cjk::{
    hankana2zenhira_table, hankana2zenkana_table, zenkana2hankana_table,
};
use super::tables::translit_kana_jisx0201_jisx0208::*;
use super::tables::unicode_table_cp932_ext::*;
use super::tables::unicode_table_jis::*;
use super::tables::unicode_table_jis2004::*;
use super::tables::unicode_table_uhc::*;
use super::*;

const ASCII: u32 = 0;
const JISX_0201_LATIN: u32 = 1;
const JISX_0201_KANA: u32 = 2;
const JISX_0208: u32 = 3;
const JISX_0212: u32 = 4;
const JISX_0201_KANA_SO: u32 = 5;

/// Lookup in ucs_{a1,a2,i,r}_jis_table (with optional U+203E special case)
#[inline]
fn ucs_jis_lookup(w: u32, overline: bool) -> u32 {
    let wi = w as i64;
    if wi >= ucs_a1_jis_table_min as i64 && wi < ucs_a1_jis_table_max as i64 {
        ucs_a1_jis_table[(w as i32 - ucs_a1_jis_table_min) as usize] as u32
    } else if overline && w == 0x203E {
        0x1007E
    } else if wi >= ucs_a2_jis_table_min as i64 && wi < ucs_a2_jis_table_max as i64 {
        ucs_a2_jis_table[(w as i32 - ucs_a2_jis_table_min) as usize] as u32
    } else if wi >= ucs_i_jis_table_min as i64 && wi < ucs_i_jis_table_max as i64 {
        ucs_i_jis_table[(w as i32 - ucs_i_jis_table_min) as usize] as u32
    } else if wi >= ucs_r_jis_table_min as i64 && wi < ucs_r_jis_table_max as i64 {
        ucs_r_jis_table[(w as i32 - ucs_r_jis_table_min) as usize] as u32
    } else {
        0
    }
}

/// Fallbacks used when the main JIS lookup fails (YEN SIGN etc.)
#[inline]
fn jis_fallback(w: u32) -> u32 {
    match w {
        0xA5 => 0x1005C,
        0xFF3C => 0x2140,
        0x2225 => 0x2142,
        0xFF0D => 0x215D,
        0xFFE0 => 0x2171,
        0xFFE1 => 0x2172,
        0xFFE2 => 0x224C,
        _ => 0,
    }
}

/* ===== ISO-2022-JP / JIS decoder ===== */

fn mb_iso2022jp_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c == 0x1B {
            if e - p < 2 {
                out.push(MBFL_BAD_INPUT);
                if p != e && (input[p] == b'$' || input[p] == b'(') {
                    p += 1;
                }
                continue;
            }
            let c2 = input[p];
            p += 1;
            if c2 == b'$' {
                let c3 = input[p];
                p += 1;
                if c3 == b'@' || c3 == b'B' {
                    *state = JISX_0208;
                } else if c3 == b'(' {
                    if p == e {
                        out.push(MBFL_BAD_INPUT);
                        break;
                    }
                    let c4 = input[p];
                    p += 1;
                    if c4 == b'@' || c4 == b'B' {
                        *state = JISX_0208;
                    } else if c4 == b'D' {
                        *state = JISX_0212;
                    } else {
                        out.push(MBFL_BAD_INPUT);
                        out.push('$' as u32);
                        out.push('(' as u32);
                        p -= 1;
                    }
                } else {
                    out.push(MBFL_BAD_INPUT);
                    out.push('$' as u32);
                    p -= 1;
                }
            } else if c2 == b'(' {
                let c3 = input[p];
                p += 1;
                if c3 == b'B' || c3 == b'H' {
                    *state = ASCII;
                } else if c3 == b'J' {
                    *state = JISX_0201_LATIN;
                } else if c3 == b'I' {
                    *state = JISX_0201_KANA;
                } else {
                    out.push(MBFL_BAD_INPUT);
                    out.push('(' as u32);
                    p -= 1;
                }
            } else {
                out.push(MBFL_BAD_INPUT);
                p -= 1;
            }
        } else if c == 0xE {
            *state = JISX_0201_KANA;
        } else if c == 0xF {
            *state = ASCII;
        } else if *state == JISX_0201_LATIN && c == 0x5C {
            out.push(0xA5);
        } else if *state == JISX_0201_LATIN && c == 0x7E {
            out.push(0x203E);
        } else if *state == JISX_0201_KANA && c > 0x20 && c < 0x60 {
            out.push(0xFF40 + c as u32);
        } else if *state >= JISX_0208 && c > 0x20 && c < 0x7F {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            if c2 > 0x20 && c2 < 0x7F {
                let s = (c as usize - 0x21) * 94 + c2 as usize - 0x21;
                let mut w = 0u32;
                if *state == JISX_0208 {
                    if s < jisx0208_ucs_table.len() {
                        w = jisx0208_ucs_table[s] as u32;
                    }
                } else if s < jisx0212_ucs_table.len() {
                    w = jisx0212_ucs_table[s] as u32;
                }
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c < 0x80 {
            out.push(c as u32);
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_iso2022jp(input: &[u32], buf: &mut ConvBuf, end: bool) {
    for &w in input {
        let mut s = ucs_jis_lookup(w, false);
        if s == 0 {
            s = jis_fallback(w);
            if s == 0 && w != 0 {
                mb_illegal_output(w, mb_wchar_to_iso2022jp, buf);
                continue;
            }
        } else if (s >= 0x80 && s < 0x2121) || s > 0x8080 {
            mb_illegal_output(w, mb_wchar_to_iso2022jp, buf);
            continue;
        }
        emit_jis(buf, s, false);
    }
    if end && buf.state != ASCII {
        buf.add3(0x1B, b'(', b'B');
    }
}

/// Common emission for ISO-2022-JP / JIS encoders
#[inline]
fn emit_jis(buf: &mut ConvBuf, s: u32, allow_kana: bool) {
    if s < 0x80 {
        if buf.state != ASCII {
            buf.add3(0x1B, b'(', b'B');
            buf.state = ASCII;
        }
        buf.add(s as u8);
    } else if allow_kana && (0xA1..=0xDF).contains(&s) {
        if buf.state != JISX_0201_KANA {
            buf.add3(0x1B, b'(', b'I');
            buf.state = JISX_0201_KANA;
        }
        buf.add((s & 0x7F) as u8);
    } else if s < 0x8080 {
        if buf.state != JISX_0208 {
            buf.add3(0x1B, b'$', b'B');
            buf.state = JISX_0208;
        }
        buf.add2(((s >> 8) & 0x7F) as u8, (s & 0x7F) as u8);
    } else if s < 0x10000 {
        if buf.state != JISX_0212 {
            buf.add4(0x1B, b'$', b'(', b'D');
            buf.state = JISX_0212;
        }
        buf.add2(((s >> 8) & 0x7F) as u8, (s & 0x7F) as u8);
    } else {
        if buf.state != JISX_0201_LATIN {
            buf.add3(0x1B, b'(', b'J');
            buf.state = JISX_0201_LATIN;
        }
        buf.add((s & 0x7F) as u8);
    }
}

fn mb_wchar_to_jis(input: &[u32], buf: &mut ConvBuf, end: bool) {
    for &w in input {
        let mut s = ucs_jis_lookup(w, true);
        if s == 0 {
            s = jis_fallback(w);
            if s == 0 && w != 0 {
                // Note: C passes mb_wchar_to_iso2022jp here
                mb_illegal_output(w, mb_wchar_to_iso2022jp, buf);
                continue;
            }
        }
        emit_jis(buf, s, true);
    }
    if end && buf.state != ASCII {
        buf.add3(0x1B, b'(', b'B');
    }
}

fn mb_check_jis(input: &[u8]) -> bool {
    let e = input.len();
    let mut p = 0usize;
    let mut state = ASCII;
    while p < e {
        let c = input[p];
        p += 1;
        if c == 0x1B {
            if state == JISX_0201_KANA_SO {
                return false;
            }
            if e - p < 2 {
                return false;
            }
            let c2 = input[p];
            p += 1;
            if c2 == b'$' {
                let c3 = input[p];
                p += 1;
                if c3 == b'@' || c3 == b'B' {
                    state = JISX_0208;
                } else if c3 == b'(' {
                    if p == e {
                        return false;
                    }
                    let c4 = input[p];
                    p += 1;
                    if c4 == b'@' || c4 == b'B' {
                        state = JISX_0208;
                    } else if c4 == b'D' {
                        state = JISX_0212;
                    } else {
                        return false;
                    }
                } else {
                    return false;
                }
            } else if c2 == b'(' {
                let c3 = input[p];
                p += 1;
                if c3 == b'B' || c3 == b'H' {
                    state = ASCII;
                } else if c3 == b'J' {
                    state = JISX_0201_LATIN;
                } else if c3 == b'I' {
                    state = JISX_0201_KANA;
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else if c == 0xE {
            if state != ASCII {
                return false;
            }
            state = JISX_0201_KANA_SO;
        } else if c == 0xF {
            if state != JISX_0201_KANA_SO {
                return false;
            }
            state = ASCII;
        } else if (state == JISX_0208 || state == JISX_0212) && (c > 0x20 && c < 0x7F) {
            if p == e {
                return false;
            }
            let c2 = input[p];
            p += 1;
            if c2 > 0x20 && c2 < 0x7F {
                let s = (c as usize - 0x21) * 94 + c2 as usize - 0x21;
                if state == JISX_0208 {
                    if s < jisx0208_ucs_table.len() && jisx0208_ucs_table[s] != 0 {
                        continue;
                    }
                } else if s < jisx0212_ucs_table.len() && jisx0212_ucs_table[s] != 0 {
                    continue;
                }
                return false;
            } else {
                return false;
            }
        } else if c < 0x80 {
            continue;
        } else if (0xA1..=0xDF).contains(&c) {
            continue;
        } else {
            return false;
        }
    }
    state == ASCII
}

fn mb_check_iso2022jp(input: &[u8]) -> bool {
    let e = input.len();
    let mut p = 0usize;
    let mut state = ASCII;
    while p < e {
        let c = input[p];
        p += 1;
        if c == 0x1B {
            if e - p < 2 {
                return false;
            }
            let c2 = input[p];
            p += 1;
            if c2 == b'$' {
                let c3 = input[p];
                p += 1;
                if c3 == b'@' || c3 == b'B' {
                    state = JISX_0208;
                } else {
                    return false;
                }
            } else if c2 == b'(' {
                let c3 = input[p];
                p += 1;
                if c3 == b'B' {
                    state = ASCII;
                } else if c3 == b'J' {
                    state = JISX_0201_LATIN;
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else if c == 0xE || c == 0xF {
            return false;
        } else if state == JISX_0208 && (c > 0x20 && c < 0x7F) {
            if p == e {
                return false;
            }
            let c2 = input[p];
            p += 1;
            if c2 > 0x20 && c2 < 0x7F {
                let s = (c as usize - 0x21) * 94 + c2 as usize - 0x21;
                if s < jisx0208_ucs_table.len() && jisx0208_ucs_table[s] != 0 {
                    continue;
                }
                return false;
            } else {
                return false;
            }
        } else if c < 0x80 {
            continue;
        } else {
            return false;
        }
    }
    state == ASCII
}

/* ===== ISO-2022-JP-2004 ===== */

const JISX0208_2004: u32 = 1;
const JISX0213_PLANE1: u32 = 2;
const JISX0213_PLANE2: u32 = 3;

fn mb_iso2022jp2004_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c <= 0x7F {
            if c == 0x1B {
                if e - p < 2 {
                    out.push(MBFL_BAD_INPUT);
                    break;
                }
                let c2 = input[p];
                let c3 = input[p + 1];
                p += 2;
                if c2 == b'$' {
                    if c3 == b'B' {
                        *state = JISX0208_2004;
                    } else if c3 == b'(' {
                        if p == e {
                            out.push(MBFL_BAD_INPUT);
                            break;
                        }
                        let c4 = input[p];
                        p += 1;
                        if c4 == b'Q' {
                            *state = JISX0213_PLANE1;
                        } else if c4 == b'P' {
                            *state = JISX0213_PLANE2;
                        } else {
                            out.push(MBFL_BAD_INPUT);
                        }
                    } else {
                        out.push(MBFL_BAD_INPUT);
                    }
                } else if c2 == b'(' {
                    if c3 == b'B' {
                        *state = ASCII;
                    } else {
                        out.push(MBFL_BAD_INPUT);
                    }
                } else {
                    p -= 1;
                    out.push(MBFL_BAD_INPUT);
                }
            } else if *state >= JISX0208_2004 && c > 0x20 && c < 0x7F {
                if p == e {
                    out.push(MBFL_BAD_INPUT);
                    break;
                }
                let c2 = input[p];
                p += 1;
                if !(0x21..=0x7E).contains(&c2) {
                    out.push(MBFL_BAD_INPUT);
                    continue;
                }
                if *state == JISX0213_PLANE1 {
                    let w1 = ((c as u32) << 8) | c2 as u32;
                    if (0x2477..=0x247B).contains(&w1)
                        || (0x2577..=0x257E).contains(&w1)
                        || w1 == 0x2678
                        || w1 == 0x2B44
                        || (0x2B48..=0x2B4F).contains(&w1)
                        || (0x2B65..=0x2B66).contains(&w1)
                    {
                        let k = mbfl_bisec_srch2(w1 as i32, &jisx0213_u2_key, jisx0213_u2_tbl_len);
                        if k >= 0 {
                            out.push(jisx0213_u2_tbl[2 * k as usize] as u32);
                            out.push(jisx0213_u2_tbl[2 * k as usize + 1] as u32);
                            continue;
                        }
                    }
                    let mut w = 0u32;
                    let w1 = (c as u32 - 0x21) * 94 + c2 as u32 - 0x21;
                    if (w1 as usize) < jisx0213_ucs_table.len() {
                        w = jisx0213_ucs_table[w1 as usize] as u32;
                    }
                    if w == 0 {
                        let k =
                            mbfl_bisec_srch2(w1 as i32, &jisx0213_jis_u5_key, jisx0213_u5_tbl_len);
                        if k >= 0 {
                            w = jisx0213_jis_u5_tbl[k as usize] as u32 + 0x20000;
                        }
                    }
                    out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
                } else if *state == JISX0213_PLANE2 {
                    let s1 = c as u32 - 0x21;
                    let s2 = c2 as u32 - 0x21;
                    if ((s1 <= 4 && s1 != 1)
                        || s1 == 7
                        || (11..=14).contains(&s1)
                        || (77..94).contains(&s1))
                        && s2 < 94
                    {
                        let mut k: i32 = 0;
                        while k < jisx0213_p2_ofst_len {
                            if s1 == jisx0213_p2_ofst[k as usize] as u32 {
                                break;
                            }
                            k += 1;
                        }
                        k -= jisx0213_p2_ofst.get(k as usize).copied().unwrap_or(0) as i32;
                        let s = ((s1 as i32 + 94 + k) * 94 + s2 as i32) as u32;
                        let mut w = jisx0213_ucs_table.get(s as usize).copied().unwrap_or(0) as u32;
                        if w == 0 {
                            let k = mbfl_bisec_srch2(
                                s as i32,
                                &jisx0213_jis_u5_key,
                                jisx0213_u5_tbl_len,
                            );
                            if k >= 0 {
                                w = jisx0213_jis_u5_tbl[k as usize] as u32 + 0x20000;
                            }
                        }
                        out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
                    } else {
                        out.push(MBFL_BAD_INPUT);
                    }
                } else {
                    let s = (c as usize - 0x21) * 94 + c2 as usize - 0x21;
                    let mut w = 0u32;
                    if s < jisx0208_ucs_table.len() {
                        w = jisx0208_ucs_table[s] as u32;
                    }
                    out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
                }
            } else {
                out.push(c as u32);
            }
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_iso2022jp2004(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let len_total = input.len();
    let mut i = 0usize;
    // A cached codepoint from a previous call (which might combine with the next one)
    let mut pending: Option<u32> = None;
    if buf.state & 0xFF00 != 0 {
        let k = ((buf.state >> 8) - 1) as usize;
        pending = Some(jisx0213_u2_tbl[2 * k] as u32);
        buf.state &= 0xFF;
    }
    loop {
        let w = if let Some(pw) = pending.take() {
            pw
        } else {
            if i >= len_total {
                break;
            }
            let w = input[i];
            i += 1;
            w
        };
        // `len` in C: number of remaining input codepoints
        let mut s: u32 = 0;
        if w == 0xE6
            || (0x254..=0x2E9).contains(&w)
            || (0x304B..=0x3053).contains(&w)
            || (0x30AB..=0x30C8).contains(&w)
            || w == 0x31F7
        {
            let mut k = 0usize;
            while (k as i32) < jisx0213_u2_tbl_len {
                if w == jisx0213_u2_tbl[2 * k] as u32 {
                    if i >= len_total {
                        if !end {
                            buf.state |= ((k + 1) as u32) << 8;
                            return;
                        }
                    } else {
                        let w2 = input[i];
                        i += 1;
                        if (w == 0x254 || w == 0x28C || w == 0x259 || w == 0x25A) && w2 == 0x301 {
                            k += 1;
                        }
                        if w2 == jisx0213_u2_tbl[2 * k + 1] as u32 {
                            s = jisx0213_u2_key[k] as u32;
                            break;
                        }
                        i -= 1;
                    }
                    s = jisx0213_u2_fb_tbl[k] as u32;
                    break;
                }
                k += 1;
            }
        }

        if s == 0 {
            for k in 0..uni2jis_tbl_len as usize {
                let lo = uni2jis_tbl_range[k][0] as u32;
                let hi = uni2jis_tbl_range[k][1] as u32;
                if w >= lo && w <= hi {
                    s = uni2jis_tbl[k][(w - lo) as usize] as u32;
                    break;
                }
            }
        }

        if s == 0 && w >= ucs_c1_jisx0213_min as u32 && w <= ucs_c1_jisx0213_max as u32 {
            let k = mbfl_bisec_srch(w as i32, &ucs_c1_jisx0213_tbl, ucs_c1_jisx0213_tbl_len);
            if k >= 0 {
                s = ucs_c1_jisx0213_ofst[k as usize] as u32 + w
                    - ucs_c1_jisx0213_tbl[2 * k as usize] as u32;
            }
        }

        if s == 0 && w >= jisx0213_u5_tbl_min as u32 && w <= jisx0213_u5_tbl_max as u32 {
            let k = mbfl_bisec_srch2(
                (w - 0x20000) as i32,
                &jisx0213_u5_jis_key,
                jisx0213_u5_tbl_len,
            );
            if k >= 0 {
                s = jisx0213_u5_jis_tbl[k as usize] as u32;
            }
        }

        if s == 0 {
            if w == 0xFE45 {
                s = 0x233E;
            } else if w == 0xFE46 {
                s = 0x233D;
            } else if (0xF91D..=0xF9DC).contains(&w) {
                let k = mbfl_bisec_srch2(
                    w as i32,
                    &ucs_r2b_jisx0213_cmap_key,
                    ucs_r2b_jisx0213_cmap_len,
                );
                if k >= 0 {
                    s = ucs_r2b_jisx0213_cmap_val[k as usize] as u32;
                }
            }
        }

        if s == 0 && w != 0 {
            mb_illegal_output(w, mb_wchar_to_iso2022jp2004, buf);
        } else if s <= 0x7F {
            if buf.state != ASCII {
                buf.add3(0x1B, b'(', b'B');
                buf.state = ASCII;
            }
            buf.add(s as u8);
        } else if s <= 0xFF {
            mb_illegal_output(w, mb_wchar_to_iso2022jp2004, buf);
        } else if s <= 0x7EFF {
            if buf.state != JISX0213_PLANE1 {
                buf.add4(0x1B, b'$', b'(', b'Q');
                buf.state = JISX0213_PLANE1;
            }
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        } else {
            if buf.state != JISX0213_PLANE2 {
                buf.add4(0x1B, b'$', b'(', b'P');
                buf.state = JISX0213_PLANE2;
            }
            let s2 = s & 0xFF;
            let k = (((s >> 8) & 0xFF) as i32 - 0x7F) as usize;
            let s1 = jisx0213_p2_ofst.get(k).copied().unwrap_or(0) as u32 + 0x21;
            buf.add2(s1 as u8, s2 as u8);
        }
    }

    if end && buf.state != ASCII {
        buf.add3(0x1B, b'(', b'B');
    }
}

/* ===== Kana conversion ===== */

/// C: `mb_convert_kana_codepoint`. Returns (codepoint, consumed_next, second).
pub fn mb_convert_kana_codepoint(
    c: u32,
    next: u32,
    consumed: &mut bool,
    second: &mut u32,
    mode: u32,
) -> u32 {
    let mode = mode as i32;
    if (mode & MBFL_HAN2ZEN_ALL) != 0
        && (0x21..=0x7D).contains(&c)
        && c != '"' as u32
        && c != '\'' as u32
        && c != '\\' as u32
    {
        return c + 0xFEE0;
    }
    if (mode & MBFL_HAN2ZEN_ALPHA) != 0
        && ((c >= 'A' as u32 && c <= 'Z' as u32) || (c >= 'a' as u32 && c <= 'z' as u32))
    {
        return c + 0xFEE0;
    }
    if (mode & MBFL_HAN2ZEN_NUMERIC) != 0 && c >= '0' as u32 && c <= '9' as u32 {
        return c + 0xFEE0;
    }
    if (mode & MBFL_HAN2ZEN_SPACE) != 0 && c == ' ' as u32 {
        return 0x3000;
    }

    if mode & (MBFL_HAN2ZEN_KATAKANA | MBFL_HAN2ZEN_HIRAGANA) != 0 {
        if (mode & MBFL_HAN2ZEN_KATAKANA) != 0
            && (mode & MBFL_HAN2ZEN_GLUE) != 0
            && (0xFF61..=0xFF9F).contains(&c)
        {
            let n = (c - 0xFF60) as usize;
            if (0xFF61..=0xFF9F).contains(&next) {
                if next == 0xFF9E && ((22..=36).contains(&n) || (42..=46).contains(&n)) {
                    *consumed = true;
                    return 0x3001 + hankana2zenkana_table[n] as u32;
                }
                if next == 0xFF9E && n == 19 {
                    *consumed = true;
                    return 0x30F4;
                }
                if next == 0xFF9F && (42..=46).contains(&n) {
                    *consumed = true;
                    return 0x3002 + hankana2zenkana_table[n] as u32;
                }
            }
            return 0x3000 + hankana2zenkana_table[n] as u32;
        }
        if (mode & MBFL_HAN2ZEN_HIRAGANA) != 0
            && (mode & MBFL_HAN2ZEN_GLUE) != 0
            && (0xFF61..=0xFF9F).contains(&c)
        {
            let n = (c - 0xFF60) as usize;
            if (0xFF61..=0xFF9F).contains(&next) {
                if next == 0xFF9E && ((22..=36).contains(&n) || (42..=46).contains(&n)) {
                    *consumed = true;
                    return 0x3001 + hankana2zenhira_table[n] as u32;
                }
                if next == 0xFF9F && (42..=46).contains(&n) {
                    *consumed = true;
                    return 0x3002 + hankana2zenhira_table[n] as u32;
                }
            }
            return 0x3000 + hankana2zenhira_table[n] as u32;
        }
        if (mode & MBFL_HAN2ZEN_KATAKANA) != 0 && (0xFF61..=0xFF9F).contains(&c) {
            return 0x3000 + hankana2zenkana_table[(c - 0xFF60) as usize] as u32;
        }
        if (mode & MBFL_HAN2ZEN_HIRAGANA) != 0 && (0xFF61..=0xFF9F).contains(&c) {
            return 0x3000 + hankana2zenhira_table[(c - 0xFF60) as usize] as u32;
        }
    }

    if mode & MBFL_HAN2ZEN_SPECIAL != 0 {
        if c == '\\' as u32 || c == 0xA5 {
            return 0xFFE5;
        }
        if c == 0x7E || c == 0x203E {
            return 0xFFE3;
        }
        if c == '\'' as u32 {
            return 0x2019;
        }
        if c == '"' as u32 {
            return 0x201D;
        }
    }

    if mode & (MBFL_ZEN2HAN_ALL | MBFL_ZEN2HAN_ALPHA | MBFL_ZEN2HAN_NUMERIC | MBFL_ZEN2HAN_SPACE)
        != 0
    {
        if (mode & MBFL_ZEN2HAN_ALL) != 0
            && (0xFF01..=0xFF5D).contains(&c)
            && c != 0xFF02
            && c != 0xFF07
            && c != 0xFF3C
        {
            return c - 0xFEE0;
        }
        if (mode & MBFL_ZEN2HAN_ALPHA) != 0
            && ((0xFF21..=0xFF3A).contains(&c) || (0xFF41..=0xFF5A).contains(&c))
        {
            return c - 0xFEE0;
        }
        if (mode & MBFL_ZEN2HAN_NUMERIC) != 0 && (0xFF10..=0xFF19).contains(&c) {
            return c - 0xFEE0;
        }
        if (mode & MBFL_ZEN2HAN_SPACE) != 0 && c == 0x3000 {
            return ' ' as u32;
        }
        if (mode & MBFL_ZEN2HAN_ALL) != 0 && c == 0x2212 {
            return '-' as u32;
        }
    }

    if mode & (MBFL_ZEN2HAN_KATAKANA | MBFL_ZEN2HAN_HIRAGANA) != 0 {
        if (mode & MBFL_ZEN2HAN_KATAKANA) != 0 && (0x30A1..=0x30F4).contains(&c) {
            let n = (c - 0x30A1) as usize;
            if zenkana2hankana_table[n][1] != 0 {
                *second = 0xFF00 + zenkana2hankana_table[n][1] as u32;
            }
            return 0xFF00 + zenkana2hankana_table[n][0] as u32;
        }
        if (mode & MBFL_ZEN2HAN_HIRAGANA) != 0 && (0x3041..=0x3093).contains(&c) {
            let n = (c - 0x3041) as usize;
            if zenkana2hankana_table[n][1] != 0 {
                *second = 0xFF00 + zenkana2hankana_table[n][1] as u32;
            }
            return 0xFF00 + zenkana2hankana_table[n][0] as u32;
        }
        match c {
            0x3001 => return 0xFF64,
            0x3002 => return 0xFF61,
            0x300C => return 0xFF62,
            0x300D => return 0xFF63,
            0x309B => return 0xFF9E,
            0x309C => return 0xFF9F,
            0x30FC => return 0xFF70,
            0x30FB => return 0xFF65,
            _ => {}
        }
    }

    if mode & (MBFL_ZENKAKU_HIRA2KATA | MBFL_ZENKAKU_KATA2HIRA) != 0 {
        if (mode & MBFL_ZENKAKU_HIRA2KATA) != 0
            && ((0x3041..=0x3093).contains(&c) || c == 0x309D || c == 0x309E)
        {
            return c + 0x60;
        }
        if (mode & MBFL_ZENKAKU_KATA2HIRA) != 0
            && ((0x30A1..=0x30F3).contains(&c) || c == 0x30FD || c == 0x30FE)
        {
            return c - 0x60;
        }
    }

    if mode & MBFL_ZEN2HAN_SPECIAL != 0 {
        if c == 0xFFE5 || c == 0xFF3C {
            return '\\' as u32;
        }
        if c == 0xFFE3 || c == 0x203E {
            return '~' as u32;
        }
        if c == 0x2018 || c == 0x2019 {
            return '\'' as u32;
        }
        if c == 0x201C || c == 0x201D {
            return '"' as u32;
        }
    }

    c
}

/* ===== CP50220 / CP50221 / CP50222 ===== */

fn mb_cp5022x_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c == 0x1B {
            if e - p < 2 {
                out.push(MBFL_BAD_INPUT);
                if p < e && (input[p] == b'(' || input[p] == b'$') {
                    p += 1;
                }
                continue;
            }
            let c2 = input[p];
            p += 1;
            if c2 == b'$' {
                let c3 = input[p];
                p += 1;
                if c3 == b'@' || c3 == b'B' {
                    *state = JISX_0208;
                } else if c3 == b'(' {
                    if p == e {
                        out.push(MBFL_BAD_INPUT);
                        break;
                    }
                    let c4 = input[p];
                    p += 1;
                    if c4 == b'@' || c4 == b'B' {
                        *state = JISX_0208;
                    } else if c4 == b'D' {
                        *state = JISX_0212;
                    } else {
                        out.push(MBFL_BAD_INPUT);
                        out.push('$' as u32);
                        out.push('(' as u32);
                        p -= 1;
                    }
                } else {
                    out.push(MBFL_BAD_INPUT);
                    out.push('$' as u32);
                    p -= 1;
                }
            } else if c2 == b'(' {
                let c3 = input[p];
                p += 1;
                if c3 == b'B' || c3 == b'H' {
                    *state = ASCII;
                } else if c3 == b'J' {
                    *state = JISX_0201_LATIN;
                } else if c3 == b'I' {
                    *state = JISX_0201_KANA;
                } else {
                    out.push(MBFL_BAD_INPUT);
                    out.push('(' as u32);
                    p -= 1;
                }
            } else {
                out.push(MBFL_BAD_INPUT);
                p -= 1;
            }
        } else if c == 0xE {
            *state = JISX_0201_KANA;
        } else if c == 0xF {
            *state = ASCII;
        } else if *state == JISX_0201_LATIN && c == 0x5C {
            out.push(0xA5);
        } else if *state == JISX_0201_LATIN && c == 0x7E {
            out.push(0x203E);
        } else if *state == JISX_0201_KANA && c > 0x20 && c < 0x60 {
            out.push(0xFF40 + c as u32);
        } else if *state >= JISX_0208 && c > 0x20 && c <= 0x97 {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            if c2 > 0x20 && c2 < 0x7F {
                let s = (c as i32 - 0x21) * 94 + c2 as i32 - 0x21;
                let mut w = 0u32;
                if *state == JISX_0208 {
                    if s >= cp932ext1_ucs_table_min && s < cp932ext1_ucs_table_max {
                        w = cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min) as usize] as u32;
                    } else if s < jisx0208_ucs_table_size {
                        w = jisx0208_ucs_table[s as usize] as u32;
                    } else if s >= cp932ext2_ucs_table_min && s < cp932ext2_ucs_table_max {
                        w = cp932ext2_ucs_table[(s - cp932ext2_ucs_table_min) as usize] as u32;
                    } else if s >= cp932ext3_ucs_table_min && s < cp932ext3_ucs_table_max {
                        w = cp932ext3_ucs_table[(s - cp932ext3_ucs_table_min) as usize] as u32;
                    } else if s >= 94 * 94 && s < 114 * 94 {
                        w = (s - 94 * 94) as u32 + 0xE000;
                    }
                } else if s < jisx0212_ucs_table_size {
                    w = jisx0212_ucs_table[s as usize] as u32;
                }
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c < 0x80 {
            out.push(c as u32);
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

/// Reverse lookup for cp932ext1/2 tables (first matching index wins, as in
/// the linear scans of the C code)
fn cp932ext12_reverse(w: u32) -> Option<u32> {
    static MAP: OnceLock<std::collections::HashMap<u16, u32>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for (i, &v) in cp932ext1_ucs_table.iter().enumerate() {
            let i = i as i32;
            m.entry(v).or_insert(
                ((((i / 94) + (cp932ext1_ucs_table_min / 94) + 0x21) << 8) + (i % 94) + 0x21)
                    as u32,
            );
        }
        for (i, &v) in cp932ext2_ucs_table.iter().enumerate() {
            let i = i as i32;
            m.entry(v).or_insert(
                ((((i / 94) + (cp932ext2_ucs_table_min / 94) + 0x21) << 8) + (i % 94) + 0x21)
                    as u32,
            );
        }
        m
    });
    if w > 0xFFFF {
        return None;
    }
    map.get(&(w as u16)).copied()
}

fn lookup_wchar(w: u32) -> u32 {
    let mut s = ucs_jis_lookup(w, true);
    if (0xE000..=0xE757).contains(&w) {
        let t = w - 0xE000;
        s = (((t / 94) + 0x7F) << 8) | ((t % 94) + 0x21);
    }
    if s == 0 {
        s = jis_fallback(w);
        if s == 0 && w == 0 {
            return 0;
        }
    }
    if s == 0 || s >= 0x8080 {
        if let Some(r) = cp932ext12_reverse(w) {
            return r;
        }
    }
    s
}

fn emit_cp5022x(buf: &mut ConvBuf, w: u32, s: u32, err: FromWcharFn) {
    if s == 0 && w != 0 {
        mb_illegal_output(w, err, buf);
    } else if s < 0x80 {
        if buf.state != ASCII {
            buf.add3(0x1B, b'(', b'B');
            buf.state = ASCII;
        }
        buf.add(s as u8);
    } else if (0xA0..0xE0).contains(&s) {
        if buf.state != JISX_0201_KANA {
            buf.add3(0x1B, b'(', b'I');
            buf.state = JISX_0201_KANA;
        }
        buf.add((s - 0x80) as u8);
    } else if s <= 0x927E {
        if buf.state != JISX_0208 {
            buf.add3(0x1B, b'$', b'B');
            buf.state = JISX_0208;
        }
        buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
    } else if s >= 0x10000 {
        if buf.state != JISX_0201_LATIN {
            buf.add3(0x1B, b'(', b'J');
            buf.state = JISX_0201_LATIN;
        }
        buf.add((s & 0x7F) as u8);
    } else {
        mb_illegal_output(w, err, buf);
    }
}

fn mb_wchar_to_cp50220(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let n = input.len();
    let mut i = 0usize;
    let mut pending: Option<u32> = None;
    if buf.state & 0xFFFF00 != 0 {
        pending = Some(buf.state >> 8);
        buf.state &= 0xFF;
    }
    loop {
        let w = if let Some(pw) = pending.take() {
            pw
        } else {
            if i >= n {
                break;
            }
            let w = input[i];
            i += 1;
            w
        };
        let len = n - i; // remaining codepoints after this one
        if (0xFF61..=0xFF9F).contains(&w) && len == 0 && !end {
            buf.state |= w << 8;
            break;
        }
        let mut consumed = false;
        let mut second = 0u32;
        let next = if len > 0 { input[i] } else { 0 };
        let w = mb_convert_kana_codepoint(
            w,
            next,
            &mut consumed,
            &mut second,
            (MBFL_HAN2ZEN_KATAKANA | MBFL_HAN2ZEN_GLUE) as u32,
        );
        if consumed {
            i += 1;
        }
        let s = lookup_wchar(w);
        emit_cp5022x(buf, w, s, mb_wchar_to_cp50221);
    }
    if end && buf.state != ASCII {
        buf.add3(0x1B, b'(', b'B');
    }
}

fn mb_wchar_to_cp50221(input: &[u32], buf: &mut ConvBuf, end: bool) {
    for &w in input {
        let s = lookup_wchar(w);
        emit_cp5022x(buf, w, s, mb_wchar_to_cp50221);
    }
    if end && buf.state != ASCII {
        buf.add3(0x1B, b'(', b'B');
    }
}

fn mb_wchar_to_cp50222(input: &[u32], buf: &mut ConvBuf, end: bool) {
    for &w in input {
        let s = lookup_wchar(w);
        if s == 0 && w != 0 {
            mb_illegal_output(w, mb_wchar_to_cp50222, buf);
        } else if s < 0x80 {
            if buf.state == JISX_0201_KANA {
                buf.add(0xF);
                buf.state = ASCII;
            } else if buf.state != ASCII {
                buf.add3(0x1B, b'(', b'B');
                buf.state = ASCII;
            }
            buf.add(s as u8);
        } else if (0xA0..0xE0).contains(&s) {
            if buf.state != JISX_0201_KANA {
                buf.add(0xE);
                buf.state = JISX_0201_KANA;
            }
            buf.add((s - 0x80) as u8);
        } else if s <= 0x927E {
            if buf.state == JISX_0201_KANA {
                buf.add(0xF);
            }
            if buf.state != JISX_0208 {
                buf.add3(0x1B, b'$', b'B');
                buf.state = JISX_0208;
            }
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        } else if s >= 0x10000 {
            if buf.state == JISX_0201_KANA {
                buf.add(0xF);
            }
            if buf.state != JISX_0201_LATIN {
                buf.add3(0x1B, b'(', b'J');
                buf.state = JISX_0201_LATIN;
            }
            buf.add((s & 0x7F) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_cp50222, buf);
        }
    }
    if end {
        if buf.state == JISX_0201_KANA {
            buf.add(0xF);
        } else if buf.state != ASCII {
            buf.add3(0x1B, b'(', b'B');
        }
    }
}

/* ===== ISO-2022-JP-MS ===== */

const MS_ASCII: u32 = 0;
const MS_JISX0201_KANA: u32 = 0x20;
const MS_JISX0208_KANJI: u32 = 0x80;
const MS_UDC: u32 = 0xA0;

#[inline]
fn sjistoidx(c1: i32, c2: i32) -> i32 {
    let off = if c2 > 0x7e { 0x41 } else { 0x40 };
    if c1 > 0x9f {
        (c1 - 0xc1) * 188 + c2 - off
    } else {
        (c1 - 0x81) * 188 + c2 - off
    }
}

fn cp932ext3_cp932ext2_jis(c: i32) -> u32 {
    let mut idx = sjistoidx(0xfa, 0x40) + c;
    if idx >= sjistoidx(0xfa, 0x5c) {
        idx -= sjistoidx(0xfa, 0x5c) - sjistoidx(0xed, 0x40);
    } else if idx >= sjistoidx(0xfa, 0x55) {
        idx -= sjistoidx(0xfa, 0x55) - sjistoidx(0xee, 0xfa);
    } else if idx >= sjistoidx(0xfa, 0x40) {
        idx -= sjistoidx(0xfa, 0x40) - sjistoidx(0xee, 0xef);
    }
    ((((idx / 94) + 0x21) << 8) | ((idx % 94) + 0x21)) as u32
}

fn mb_iso2022jpms_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c == 0x1B {
            if e - p < 2 {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            let c3 = input[p + 1];
            p += 2;
            if c2 == b'$' {
                if c3 == b'@' || c3 == b'B' {
                    *state = MS_JISX0208_KANJI;
                } else if c3 == b'(' && p < e {
                    let c4 = input[p];
                    p += 1;
                    if c4 == b'@' || c4 == b'B' {
                        *state = MS_JISX0208_KANJI;
                    } else if c4 == b'?' {
                        *state = MS_UDC;
                    } else {
                        out.push(MBFL_BAD_INPUT);
                    }
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else if c2 == b'(' {
                if c3 == b'B' || c3 == b'J' {
                    *state = MS_ASCII;
                } else if c3 == b'I' {
                    *state = MS_JISX0201_KANA;
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else {
                p -= 1;
                out.push(MBFL_BAD_INPUT);
            }
        } else if *state == MS_JISX0201_KANA && (0x21..=0x5F).contains(&c) {
            out.push(0xFF40 + c as u32);
        } else if (*state == MS_JISX0208_KANJI || *state == MS_UDC) && (0x21..=0x7F).contains(&c) {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            let mut w = 0u32;
            if (0x21..=0x7E).contains(&c2) {
                let s = ((c as i32 - 0x21) * 94) + c2 as i32 - 0x21;
                if *state == MS_JISX0208_KANJI {
                    if s <= 137 {
                        w = match s {
                            31 => 0xFF3C,
                            32 => 0xFF5E,
                            33 => 0x2225,
                            60 => 0xFF0D,
                            80 => 0xFFE0,
                            81 => 0xFFE1,
                            137 => 0xFFE2,
                            _ => 0,
                        };
                    }
                    if w == 0 {
                        if s >= cp932ext1_ucs_table_min && s < cp932ext1_ucs_table_max {
                            w = cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min) as usize] as u32;
                        } else if s < jisx0208_ucs_table_size {
                            w = jisx0208_ucs_table[s as usize] as u32;
                        } else if s >= cp932ext2_ucs_table_min && s < cp932ext2_ucs_table_max {
                            w = cp932ext2_ucs_table[(s - cp932ext2_ucs_table_min) as usize] as u32;
                        }
                    }
                } else if (0x21..=0x34).contains(&c) {
                    w = 0xE000 + ((c as u32 - 0x21) * 94) + c2 as u32 - 0x21;
                }
                out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c <= 0x7F {
            out.push(c as u32);
        } else if (0xA1..=0xDF).contains(&c) {
            out.push(0xFEC0 + c as u32);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn cp932ext1_reverse_ms(w: u32) -> Option<u32> {
    static MAP: OnceLock<std::collections::HashMap<u16, u32>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for (i, &v) in cp932ext1_ucs_table.iter().enumerate() {
            let i = i as u32;
            m.entry(v)
                .or_insert((((i / 94) + 0x2D) << 8) + (i % 94) + 0x21);
        }
        m
    });
    if w > 0xFFFF {
        return None;
    }
    map.get(&(w as u16)).copied()
}

fn cp932ext3_reverse_ms(w: u32) -> Option<u32> {
    static MAP: OnceLock<std::collections::HashMap<u16, u32>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for (i, &v) in cp932ext3_ucs_table.iter().enumerate() {
            m.entry(v).or_insert(cp932ext3_cp932ext2_jis(i as i32));
        }
        m
    });
    if w > 0xFFFF {
        return None;
    }
    map.get(&(w as u16)).copied()
}

fn mb_wchar_to_iso2022jpms(input: &[u32], buf: &mut ConvBuf, end: bool) {
    for &w in input {
        let mut s = ucs_jis_lookup(w, false);
        if w >= 0xE000 && w < 0xE000 + 20 * 94 && !in_jis_tables(w) {
            s = ((((w - 0xE000) / 94) + 0x7F) << 8) | (((w - 0xE000) % 94) + 0x21);
        }
        if s == 0 {
            s = match w {
                0xA5 => 0x216F,
                0xFF3C => 0x2140,
                0x2225 => 0x2142,
                0xFF0D => 0x215D,
                0xFFE0 => 0x2171,
                0xFFE1 => 0x2172,
                0xFFE2 => 0x224C,
                _ => 0,
            };
        }
        if s >= 0xA1A1 {
            s = 0;
        }
        if s == 0 && w != 0 {
            if let Some(r) = cp932ext1_reverse_ms(w) {
                s = r;
            } else if let Some(r) = cp932ext3_reverse_ms(w) {
                s = r;
            }
        }

        if s == 0 && w != 0 {
            mb_illegal_output(w, mb_wchar_to_iso2022jpms, buf);
        } else if s <= 0x7F {
            if buf.state != MS_ASCII {
                buf.add3(0x1B, b'(', b'B');
                buf.state = MS_ASCII;
            }
            buf.add(s as u8);
        } else if (0xA1..=0xDF).contains(&s) {
            if buf.state != MS_JISX0201_KANA {
                buf.add3(0x1B, b'(', b'I');
                buf.state = MS_JISX0201_KANA;
            }
            buf.add((s & 0x7F) as u8);
        } else if s <= 0x7E7E {
            if buf.state != MS_JISX0208_KANJI {
                buf.add3(0x1B, b'$', b'B');
                buf.state = MS_JISX0208_KANJI;
            }
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0x7F) as u8);
        } else if s < 0x927F {
            if buf.state != MS_UDC {
                buf.add4(0x1B, b'$', b'(', b'?');
                buf.state = MS_UDC;
            }
            buf.add2((((s >> 8) - 0x5E) & 0x7F) as u8, (s & 0x7F) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_iso2022jpms, buf);
        }
    }
    if end && buf.state != MS_ASCII {
        buf.add3(0x1B, b'(', b'B');
    }
}

/// Whether `w` falls in one of the ranges covered by the ucs_*_jis tables
/// (in which case C's if/else chain does not reach the PUA branch)
#[inline]
fn in_jis_tables(w: u32) -> bool {
    let wi = w as i32;
    (wi >= ucs_a1_jis_table_min && wi < ucs_a1_jis_table_max)
        || (wi >= ucs_a2_jis_table_min && wi < ucs_a2_jis_table_max)
        || (wi >= ucs_i_jis_table_min && wi < ucs_i_jis_table_max)
        || (wi >= ucs_r_jis_table_min && wi < ucs_r_jis_table_max)
}

/* ===== ISO-2022-KR ===== */

const KR_ASCII: u32 = 0;
const KSC5601: u32 = 1;
const EMITTED_ESC_SEQUENCE: u32 = 0x10;

fn mb_iso2022kr_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c == 0x1B {
            if e - p < 3 {
                out.push(MBFL_BAD_INPUT);
                if p < e {
                    let b = input[p];
                    p += 1;
                    if b == b'$' && p < e {
                        p += 1;
                    }
                }
                continue;
            }
            let c2 = input[p];
            let c3 = input[p + 1];
            let c4 = input[p + 2];
            p += 3;
            if c2 == b'$' && c3 == b')' && c4 == b'C' {
                *state = KR_ASCII;
            } else {
                if c3 != b')' {
                    p -= 1;
                    if c2 != b'$' {
                        p -= 1;
                    }
                }
                out.push(MBFL_BAD_INPUT);
            }
        } else if c == 0xF {
            *state = KR_ASCII;
        } else if c == 0xE {
            *state = KSC5601;
        } else if (0x21..=0x7E).contains(&c) && *state == KSC5601 {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            let mut w = 0u32;
            if !(0x21..=0x7E).contains(&c2) {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            if c < 0x47 {
                if c != 0x22 || c2 <= 0x65 {
                    let idx = (c as usize - 1) * 190 + c2 as usize - 0x41 + 0x80;
                    w = uhc1_ucs_table.get(idx).copied().unwrap_or(0) as u32;
                }
            } else if c != 0x49 && c <= 0x7D {
                let idx = (c as usize - 0x47) * 94 + c2 as usize - 0x21;
                w = uhc3_ucs_table.get(idx).copied().unwrap_or(0) as u32;
            }
            out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
        } else if c < 0x80 && *state == KR_ASCII {
            out.push(c as u32);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_iso2022kr(input: &[u32], buf: &mut ConvBuf, end: bool) {
    if !input.is_empty() && (buf.state & EMITTED_ESC_SEQUENCE) == 0 {
        buf.add4(0x1B, b'$', b')', b'C');
        buf.state |= EMITTED_ESC_SEQUENCE;
    }
    for &w in input {
        let wi = w as i64;
        let mut s: u32 = if wi >= ucs_a1_uhc_table_min as i64 && wi < ucs_a1_uhc_table_max as i64 {
            ucs_a1_uhc_table[(wi - ucs_a1_uhc_table_min as i64) as usize] as u32
        } else if wi >= ucs_a2_uhc_table_min as i64 && wi < ucs_a2_uhc_table_max as i64 {
            ucs_a2_uhc_table[(wi - ucs_a2_uhc_table_min as i64) as usize] as u32
        } else if wi >= ucs_a3_uhc_table_min as i64 && wi < ucs_a3_uhc_table_max as i64 {
            ucs_a3_uhc_table[(wi - ucs_a3_uhc_table_min as i64) as usize] as u32
        } else if wi >= ucs_i_uhc_table_min as i64 && wi < ucs_i_uhc_table_max as i64 {
            ucs_i_uhc_table[(wi - ucs_i_uhc_table_min as i64) as usize] as u32
        } else if wi >= ucs_s_uhc_table_min as i64 && wi < ucs_s_uhc_table_max as i64 {
            ucs_s_uhc_table[(wi - ucs_s_uhc_table_min as i64) as usize] as u32
        } else if wi >= ucs_r1_uhc_table_min as i64 && wi < ucs_r1_uhc_table_max as i64 {
            ucs_r1_uhc_table[(wi - ucs_r1_uhc_table_min as i64) as usize] as u32
        } else if wi >= ucs_r2_uhc_table_min as i64 && wi < ucs_r2_uhc_table_max as i64 {
            ucs_r2_uhc_table[(wi - ucs_r2_uhc_table_min as i64) as usize] as u32
        } else {
            0
        };

        if ((s >> 8) & 0xFF) < 0xA1 || (s & 0xFF) < 0xA1 {
            s = w;
        } else {
            s -= 0x8080;
        }

        if (s >= 0x80 && s < 0x2121) || s > 0x8080 {
            mb_illegal_output(w, mb_wchar_to_iso2022kr, buf);
        } else if s < 0x80 {
            if (buf.state & 1) != KR_ASCII {
                buf.add(0xF);
                buf.state &= !KSC5601;
            }
            buf.add(s as u8);
        } else {
            if (buf.state & 1) != KSC5601 {
                buf.add(0xE);
                buf.state |= KSC5601;
            }
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        }
    }
    if end && (buf.state & 1) != KR_ASCII {
        buf.add(0xF);
    }
}

/* ===== Encoding definitions ===== */

pub static ENC_JIS: Encoding = Encoding {
    no: EncNo::Jis,
    name: "JIS",
    mime_name: Some("ISO-2022-JP"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_iso2022jp_to_wchar),
    from_wchar: Some(mb_wchar_to_jis),
    check: Some(mb_check_jis),
    cut: None,
};

pub static ENC_2022JP: Encoding = Encoding {
    no: EncNo::Iso2022jp,
    name: "ISO-2022-JP",
    mime_name: Some("ISO-2022-JP"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_iso2022jp_to_wchar),
    from_wchar: Some(mb_wchar_to_iso2022jp),
    check: Some(mb_check_iso2022jp),
    cut: None,
};

pub static ENC_2022JP_2004: Encoding = Encoding {
    no: EncNo::Iso2022jp2004,
    name: "ISO-2022-JP-2004",
    mime_name: Some("ISO-2022-JP-2004"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_iso2022jp2004_to_wchar),
    from_wchar: Some(mb_wchar_to_iso2022jp2004),
    check: None,
    cut: None,
};

pub static ENC_CP50220: Encoding = Encoding {
    no: EncNo::Cp50220,
    name: "CP50220",
    mime_name: Some("ISO-2022-JP"),
    aliases: &["cp50220raw", "cp50220-raw", "JIS-ms"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_cp5022x_to_wchar),
    from_wchar: Some(mb_wchar_to_cp50220),
    check: None,
    cut: None,
};

pub static ENC_CP50221: Encoding = Encoding {
    no: EncNo::Cp50221,
    name: "CP50221",
    mime_name: Some("ISO-2022-JP"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_cp5022x_to_wchar),
    from_wchar: Some(mb_wchar_to_cp50221),
    check: None,
    cut: None,
};

pub static ENC_CP50222: Encoding = Encoding {
    no: EncNo::Cp50222,
    name: "CP50222",
    mime_name: Some("ISO-2022-JP"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_cp5022x_to_wchar),
    from_wchar: Some(mb_wchar_to_cp50222),
    check: None,
    cut: None,
};

pub static ENC_2022JPMS: Encoding = Encoding {
    no: EncNo::Iso2022jpms,
    name: "ISO-2022-JP-MS",
    mime_name: Some("ISO-2022-JP"),
    aliases: &["ISO2022JPMS"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_iso2022jpms_to_wchar),
    from_wchar: Some(mb_wchar_to_iso2022jpms),
    check: None,
    cut: None,
};

pub static ENC_2022KR: Encoding = Encoding {
    no: EncNo::Iso2022kr,
    name: "ISO-2022-KR",
    mime_name: Some("ISO-2022-KR"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_iso2022kr_to_wchar),
    from_wchar: Some(mb_wchar_to_iso2022kr),
    check: None,
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[
    &ENC_JIS,
    &ENC_2022JP,
    &ENC_2022JP_2004,
    &ENC_CP50220,
    &ENC_CP50221,
    &ENC_CP50222,
    &ENC_2022JPMS,
    &ENC_2022KR,
];
