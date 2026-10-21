//! Japanese EUC variants (C: mbfilter_cjk.c, "EUC variants" section):
//! EUC-JP, eucJP-win, CP51932, EUC-JP-2004

use super::common::*;
use super::mobile::ucs_to_jis;
use super::sjis::{X0213, ucs_to_jisx0213};
use super::tables::cp932_table::*;
use super::tables::filt_cjk::mblen_table_eucjp;
use super::tables::unicode_table_cp932_ext::*;
use super::tables::unicode_table_jis::*;
use super::tables::unicode_table_jis2004::*;
use super::*;

#[inline]
fn row12(s: u32) -> u32 {
    if s <= 137 {
        match s {
            31 => 0xFF3C,
            32 => 0xFF5E,
            33 => 0x2225,
            60 => 0xFF0D,
            80 => 0xFFE0,
            81 => 0xFFE1,
            137 => 0xFFE2,
            _ => 0,
        }
    } else {
        0
    }
}

#[inline]
fn special_reverse(w: u32) -> u32 {
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

/* ===== EUC-JP ===== */

fn mb_eucjp_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if (0xA1..=0xFE).contains(&c) && p < e {
            /* JISX 0208 */
            let c2 = input[p];
            p += 1;
            if (0xA1..=0xFE).contains(&c2) {
                let s = (c as u32 - 0xA1) * 94 + c2 as u32 - 0xA1;
                if s < jisx0208_ucs_table_size as u32 {
                    let w = jisx0208_ucs_table[s as usize] as u32;
                    out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c == 0x8E && p < e {
            /* Kana */
            let c2 = input[p];
            p += 1;
            out.push(if (0xA1..=0xDF).contains(&c2) {
                0xFEC0 + c2 as u32
            } else {
                MBFL_BAD_INPUT
            });
        } else if c == 0x8F {
            /* JISX 0212 */
            if e - p >= 2 {
                let c2 = input[p];
                let c3 = input[p + 1];
                p += 2;
                if (0xA1..=0xFE).contains(&c3) && (0xA1..=0xFE).contains(&c2) {
                    let s = (c2 as u32 - 0xA1) * 94 + c3 as u32 - 0xA1;
                    if s < jisx0212_ucs_table_size as u32 {
                        let w = jisx0212_ucs_table[s as usize] as u32;
                        out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
                    } else {
                        out.push(MBFL_BAD_INPUT);
                    }
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else {
                out.push(MBFL_BAD_INPUT);
                p = e; /* Jump to end of string */
            }
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_eucjp(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s = if w == 0xAF {
            0xA2B4 /* Use JIS X 0212 overline */
        } else {
            ucs_to_jis(w)
        };
        if s == 0 {
            let r = special_reverse(w);
            if r != 0 {
                s = r;
            } else if w == 0 {
                buf.add(0);
                continue;
            } else {
                mb_illegal_output(w, mb_wchar_to_eucjp, buf);
                continue;
            }
        }
        if s < 0x80 {
            buf.add(s as u8);
        } else if s < 0x100 {
            buf.add2(0x8E, s as u8);
        } else if s < 0x8080 {
            buf.add2((((s >> 8) & 0xFF) | 0x80) as u8, ((s & 0xFF) | 0x80) as u8);
        } else {
            buf.add3(
                0x8F,
                (((s >> 8) & 0xFF) | 0x80) as u8,
                ((s & 0xFF) | 0x80) as u8,
            );
        }
    }
}

pub static ENC_EUCJP: Encoding = Encoding {
    no: EncNo::EucJp,
    name: "EUC-JP",
    mime_name: Some("EUC-JP"),
    aliases: &["EUC", "EUC_JP", "eucJP", "x-euc-jp"],
    mblen_table: Some(&mblen_table_eucjp),
    flag: 0,
    to_wchar: Some(mb_eucjp_to_wchar),
    from_wchar: Some(mb_wchar_to_eucjp),
    check: None,
    cut: None,
};

/* ===== eucJP-win ===== */

fn mb_eucjpwin_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if (0xA1..=0xFE).contains(&c) && p < e {
            let c2 = input[p];
            p += 1;
            if (0xA1..=0xFE).contains(&c2) {
                let s = (c as u32 - 0xA1) * 94 + c2 as u32 - 0xA1;
                let mut w = row12(s);
                if w == 0 {
                    if s >= cp932ext1_ucs_table_min as u32 && s < cp932ext1_ucs_table_max as u32 {
                        w = cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min as u32) as usize]
                            as u32;
                    } else if s < jisx0208_ucs_table_size as u32 {
                        w = jisx0208_ucs_table[s as usize] as u32;
                    } else if s >= 84 * 94 {
                        w = s - (84 * 94) + 0xE000;
                    }
                }
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c == 0x8E && p < e {
            let c2 = input[p];
            p += 1;
            out.push(if (0xA1..=0xDF).contains(&c2) {
                0xFEC0 + c2 as u32
            } else {
                MBFL_BAD_INPUT
            });
        } else if c == 0x8F && p < e {
            let c2 = input[p];
            p += 1;
            if p == e {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            let c3 = input[p];
            p += 1;
            if (0xA1..=0xFE).contains(&c2) && (0xA1..=0xFE).contains(&c3) {
                let s = (c2 as u32 - 0xA1) * 94 + c3 as u32 - 0xA1;
                let mut w: u32 = 0;
                if s < jisx0212_ucs_table_size as u32 {
                    w = jisx0212_ucs_table[s as usize] as u32;
                    if w == 0x7E {
                        w = 0xFF5E; /* FULLWIDTH TILDE */
                    }
                } else if s >= 82 * 94 && s < 84 * 94 {
                    let s = ((c2 as u32) << 8) | c3 as u32;
                    for i in 0..cp932ext3_eucjp_table_size as usize {
                        if cp932ext3_eucjp_table[i] as u32 == s {
                            w = cp932ext3_ucs_table[i] as u32;
                            break;
                        }
                    }
                } else if s >= 84 * 94 {
                    w = s - (84 * 94) + 0xE000 + (94 * 10);
                }
                if w == 0xA6 {
                    w = 0xFFE4; /* FULLWIDTH BROKEN BAR */
                }
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_eucjpwin(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s: u32;
        if w == 0 {
            buf.add(0);
            continue;
        } else if w == 0xAF {
            s = 0xA2B4;
        } else if w == 0x203E {
            s = 0x7E;
        } else {
            s = ucs_to_jis(w);
            if s == 0 {
                if w >= 0xE000 && w < 0xE000 + 10 * 94 {
                    let t = w - 0xE000;
                    s = ((t / 94 + 0x75) << 8) + (t % 94) + 0x21;
                } else if w >= 0xE000 + 10 * 94 && w < 0xE000 + 20 * 94 {
                    let t = w - (0xE000 + 10 * 94);
                    s = ((t / 94 + 0xF5) << 8) + (t % 94) + 0xA1;
                }
            }
        }

        if s == 0xA2F1 {
            s = 0x2D62; /* NUMERO SIGN */
        }

        if s == 0 {
            if w == 0xA5 {
                s = 0x5C;
            } else if w == 0x2014 {
                s = 0x213D;
            } else {
                s = special_reverse(w);
                if s == 0 {
                    for i in 0..(cp932ext1_ucs_table_max - cp932ext1_ucs_table_min) as usize {
                        if cp932ext1_ucs_table[i] as u32 == w {
                            s = ((((i / 94) + (cp932ext1_ucs_table_min as usize / 94) + 0x21) << 8)
                                + (i % 94)
                                + 0x21) as u32;
                            break;
                        }
                    }
                    if s == 0 {
                        for i in 0..(cp932ext3_ucs_table_max - cp932ext3_ucs_table_min) as usize {
                            if cp932ext3_ucs_table[i] as u32 == w {
                                s = cp932ext3_eucjp_table[i] as u32;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if s == 0 {
            mb_illegal_output(w, mb_wchar_to_eucjpwin, buf);
        } else if s < 0x80 {
            buf.add(s as u8);
        } else if s < 0x100 {
            buf.add2(0x8E, s as u8);
        } else if s < 0x8080 {
            buf.add2((((s >> 8) & 0xFF) | 0x80) as u8, ((s & 0xFF) | 0x80) as u8);
        } else {
            buf.add3(
                0x8F,
                (((s >> 8) & 0xFF) | 0x80) as u8,
                ((s & 0xFF) | 0x80) as u8,
            );
        }
    }
}

pub static ENC_EUCJP_WIN: Encoding = Encoding {
    no: EncNo::EucJpWin,
    name: "eucJP-win",
    mime_name: Some("EUC-JP"),
    aliases: &["eucJP-open", "eucJP-ms"],
    mblen_table: Some(&mblen_table_eucjp),
    flag: 0,
    to_wchar: Some(mb_eucjpwin_to_wchar),
    from_wchar: Some(mb_wchar_to_eucjpwin),
    check: None,
    cut: None,
};

/* ===== CP51932 ===== */

fn mb_cp51932_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if (0xA1..=0xFE).contains(&c) && p < e {
            let c2 = input[p];
            p += 1;
            if (0xA1..=0xFE).contains(&c2) {
                let s = (c as u32 - 0xA1) * 94 + c2 as u32 - 0xA1;
                let mut w = row12(s);
                if w == 0 {
                    if s >= cp932ext1_ucs_table_min as u32 && s < cp932ext1_ucs_table_max as u32 {
                        w = cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min as u32) as usize]
                            as u32;
                    } else if s < jisx0208_ucs_table_size as u32 {
                        w = jisx0208_ucs_table[s as usize] as u32;
                    } else if s >= cp932ext2_ucs_table_min as u32
                        && s < cp932ext2_ucs_table_max as u32
                    {
                        w = cp932ext2_ucs_table[(s - cp932ext2_ucs_table_min as u32) as usize]
                            as u32;
                    }
                }
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c == 0x8E && p < e {
            let c2 = input[p];
            p += 1;
            out.push(if (0xA1..=0xDF).contains(&c2) {
                0xFEC0 + c2 as u32
            } else {
                MBFL_BAD_INPUT
            });
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_cp51932(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w == 0 {
            buf.add(0);
            continue;
        }
        let mut s = ucs_to_jis(w);
        if s >= 0x8080 {
            s = 0; /* We don't support JIS X0213 */
        }
        if s == 0 {
            if w == 0xA5 {
                s = 0x216F;
            } else {
                s = special_reverse(w);
                if s == 0 {
                    'found: {
                        for i in 0..(cp932ext1_ucs_table_max - cp932ext1_ucs_table_min) as usize {
                            if cp932ext1_ucs_table[i] as u32 == w {
                                s = (((i / 94 + 0x2D) << 8) + (i % 94) + 0x21) as u32;
                                break 'found;
                            }
                        }
                        for i in 0..(cp932ext2_ucs_table_max - cp932ext2_ucs_table_min) as usize {
                            if cp932ext2_ucs_table[i] as u32 == w {
                                s = (((i / 94 + 0x79) << 8) + (i % 94) + 0x21) as u32;
                                break 'found;
                            }
                        }
                    }
                }
            }
        }

        if s == 0 || s >= 0x8080 {
            mb_illegal_output(w, mb_wchar_to_cp51932, buf);
        } else if s < 0x80 {
            buf.add(s as u8);
        } else if s < 0x100 {
            buf.add2(0x8E, s as u8);
        } else {
            buf.add2((((s >> 8) & 0xFF) | 0x80) as u8, ((s & 0xFF) | 0x80) as u8);
        }
    }
}

pub static ENC_CP51932: Encoding = Encoding {
    no: EncNo::Cp51932,
    name: "CP51932",
    mime_name: Some("CP51932"),
    aliases: &["cp51932"],
    mblen_table: Some(&mblen_table_eucjp),
    flag: 0,
    to_wchar: Some(mb_cp51932_to_wchar),
    from_wchar: Some(mb_wchar_to_cp51932),
    check: None,
    cut: None,
};

/* ===== EUC-JP-2004 ===== */

fn mb_eucjp2004_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c <= 0x7F {
            out.push(c as u32);
        } else if (0xA1..=0xFE).contains(&c) {
            /* Kanji */
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            if c2 <= 0xA0 || c2 == 0xFF {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            let s1 = c as u32 - 0x80;
            let s2 = c2 as u32 - 0x80;
            let mut w1 = (s1 << 8) | s2;
            let mut w: u32 = 0;

            /* Conversion for combining characters */
            if (0x2477..=0x2479).contains(&w1)
                || (0x2479..=0x247B).contains(&w1)
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

            /* Conversion for BMP */
            w1 = (s1 - 0x21) * 94 + s2 - 0x21;
            if w1 < jisx0213_ucs_table_size as u32 {
                w = jisx0213_ucs_table[w1 as usize] as u32;
            }

            /* Conversion for CJK Unified Ideographs ext.B (U+2XXXX) */
            if w == 0 {
                let k = mbfl_bisec_srch2(w1 as i32, &jisx0213_jis_u5_key, jisx0213_u5_tbl_len);
                if k >= 0 {
                    w = jisx0213_jis_u5_tbl[k as usize] as u32 + 0x20000;
                }
            }

            out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
        } else if c == 0x8E && p < e {
            /* Kana */
            let c2 = input[p];
            p += 1;
            out.push(if (0xA1..=0xDF).contains(&c2) {
                0xFEC0 + c2 as u32
            } else {
                MBFL_BAD_INPUT
            });
        } else if c == 0x8F && p < e {
            let c2 = input[p];
            p += 1;
            if (c2 == 0xA1
                || (0xA3..=0xA5).contains(&c2)
                || c2 == 0xA8
                || (0xAC..=0xAF).contains(&c2)
                || (0xEE..=0xFE).contains(&c2))
                && p < e
            {
                let c3 = input[p];
                p += 1;
                if c3 < 0xA1 || c3 == 0xFF {
                    out.push(MBFL_BAD_INPUT);
                    continue;
                }
                let s1 = c2 as u32 - 0xA1;
                let s2 = c3 as u32 - 0xA1;
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

                    /* Check for Japanese chars in BMP */
                    let s = ((s1 as i32 + 94 + k) * 94 + s2 as i32) as u32;
                    let mut w = jisx0213_ucs_table.get(s as usize).copied().unwrap_or(0) as u32;

                    /* Check for Japanese chars in CJK Unified Ideographs ext B (U+2XXXX) */
                    if w == 0 {
                        let k =
                            mbfl_bisec_srch2(s as i32, &jisx0213_jis_u5_key, jisx0213_u5_tbl_len);
                        if k >= 0 {
                            w = jisx0213_jis_u5_tbl[k as usize] as u32 + 0x20000;
                        }
                    }
                    out.push(if w != 0 { w } else { MBFL_BAD_INPUT });
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_eucjp2004(input: &[u32], buf: &mut ConvBuf, end: bool) {
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
            mb_illegal_output(w, mb_wchar_to_eucjp2004, buf);
        } else if s <= 0x7F {
            buf.add(s as u8);
        } else if s <= 0xFF {
            buf.add2(0x8E, s as u8);
        } else if s <= 0x7EFF {
            buf.add2((((s >> 8) & 0xFF) + 0x80) as u8, ((s & 0xFF) + 0x80) as u8);
        } else {
            let s2 = s & 0xFF;
            let k = ((s >> 8) & 0xFF) as i32 - 0x7F;
            let s = jisx0213_p2_ofst.get(k as usize).copied().unwrap_or(0) as u32 + 0x21;
            buf.add3(0x8F, (s | 0x80) as u8, (s2 | 0x80) as u8);
        }
    }
}

pub static ENC_EUCJP2004: Encoding = Encoding {
    no: EncNo::EucJp2004,
    name: "EUC-JP-2004",
    mime_name: Some("EUC-JP"),
    aliases: &["EUC_JP-2004"],
    mblen_table: Some(&mblen_table_eucjp),
    flag: 0,
    to_wchar: Some(mb_eucjp2004_to_wchar),
    from_wchar: Some(mb_wchar_to_eucjp2004),
    check: None,
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[&ENC_EUCJP, &ENC_EUCJP_WIN, &ENC_CP51932, &ENC_EUCJP2004];
