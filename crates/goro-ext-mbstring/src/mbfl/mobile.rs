//! Mobile (DOCOMO/KDDI/SoftBank) emoji helpers and ISO-2022-JP-MOBILE#KDDI
//! (C: mbfilter_cjk.c)

use super::common::*;
use super::tables::emoji2uni::*;
use super::tables::unicode_table_cp932_ext::*;
use super::tables::unicode_table_jis::*;
use super::*;

/// Unicode codepoints for emoji are above 0x1F000, but the tables only
/// store 16 bits; also some use the Private Use Area above 0xFE000.
#[inline]
pub fn convert_emoji_cp(cp: i32) -> i32 {
    if cp > 0xF000 {
        cp + 0x10000
    } else if cp > 0xE000 {
        cp + 0xF0000
    } else {
        cp
    }
}

#[inline]
fn flag(country: &[u8; 2], snd: &mut i32) -> i32 {
    *snd = nflags(country[0]) as i32;
    nflags(country[1]) as i32
}

#[inline]
fn keypad(c: i32, snd: &mut i32) -> i32 {
    *snd = c;
    0x20E3
}

pub fn mbfilter_sjis_emoji_kddi2unicode(s: i32, snd: &mut i32) -> i32 {
    if s >= mb_tbl_code2uni_kddi1_min && s <= mb_tbl_code2uni_kddi1_max {
        if s == 0x24C0 {
            return flag(b"ES", snd);
        } else if s == 0x24C1 {
            return flag(b"RU", snd);
        } else if (0x2545..=0x254A).contains(&s) {
            return flag(&nflags_kddi[(s - 0x2545) as usize], snd);
        } else if s == 0x25BC {
            return keypad('#' as i32, snd);
        } else {
            *snd = 0;
            return convert_emoji_cp(mb_tbl_code2uni_kddi1[(s - mb_tbl_code2uni_kddi1_min) as usize] as i32);
        }
    } else if s >= mb_tbl_code2uni_kddi2_min && s <= mb_tbl_code2uni_kddi2_max {
        if s == 0x2750 {
            return flag(b"JP", snd);
        } else if (0x27A6..=0x27AE).contains(&s) {
            return keypad(s - 0x27A6 + '1' as i32, snd);
        } else if s == 0x27F7 {
            return flag(b"US", snd);
        } else if s == 0x2830 {
            return keypad('0' as i32, snd);
        } else {
            *snd = 0;
            return convert_emoji_cp(mb_tbl_code2uni_kddi2[(s - mb_tbl_code2uni_kddi2_min) as usize] as i32);
        }
    }
    0
}

pub fn mbfilter_sjis_emoji_docomo2unicode(s: i32, snd: &mut i32) -> i32 {
    if s >= mb_tbl_code2uni_docomo1_min && s <= mb_tbl_code2uni_docomo1_max {
        let v = convert_emoji_cp(mb_tbl_code2uni_docomo1[(s - mb_tbl_code2uni_docomo1_min) as usize] as i32);
        if (s >= docomo_keypad(1) && s <= docomo_keypad(9)) || s == docomo_keypad(0) || s == DOCOMO_KEYPAD_HASH {
            return keypad(v, snd);
        } else {
            *snd = 0;
            return v;
        }
    }
    0
}

pub fn mbfilter_sjis_emoji_sb2unicode(s: i32, snd: &mut i32) -> i32 {
    if s >= mb_tbl_code2uni_sb1_min && s <= mb_tbl_code2uni_sb1_max {
        if s == 0x2817 || (0x2823..=0x282C).contains(&s) {
            return keypad(mb_tbl_code2uni_sb1[(s - mb_tbl_code2uni_sb1_min) as usize] as i32, snd);
        } else {
            *snd = 0;
            return convert_emoji_cp(mb_tbl_code2uni_sb1[(s - mb_tbl_code2uni_sb1_min) as usize] as i32);
        }
    } else if s >= mb_tbl_code2uni_sb2_min && s <= mb_tbl_code2uni_sb2_max {
        *snd = 0;
        return convert_emoji_cp(mb_tbl_code2uni_sb2[(s - mb_tbl_code2uni_sb2_min) as usize] as i32);
    } else if s >= mb_tbl_code2uni_sb3_min && s <= mb_tbl_code2uni_sb3_max {
        if (0x2B02..=0x2B0B).contains(&s) {
            return flag(&nflags_sb[(s - 0x2B02) as usize], snd);
        } else {
            *snd = 0;
            return convert_emoji_cp(mb_tbl_code2uni_sb3[(s - mb_tbl_code2uni_sb3_min) as usize] as i32);
        }
    }
    0
}

/// Look up a Unicode codepoint in a vendor's uni->code emoji tables
/// (the code2/code3/code5 ranges). Returns the kuten code or 0.
#[inline]
pub fn emoji_lookup(
    w: u32,
    t2: (i32, i32, &[u16], &[u16], i32),
    t3: (i32, i32, &[u16], &[u16], i32),
    t5: (i32, i32, &[u16], &[u16], i32),
) -> u32 {
    let wi = w as i64;
    if wi >= t2.0 as i64 && wi <= t2.1 as i64 {
        let i = mbfl_bisec_srch2(w as i32, t2.2, t2.4);
        if i >= 0 {
            return t2.3[i as usize] as u32;
        }
    } else if wi >= t3.0 as i64 && wi <= t3.1 as i64 {
        let i = mbfl_bisec_srch2(w as i32 - 0x10000, t3.2, t3.4);
        if i >= 0 {
            return t3.3[i as usize] as u32;
        }
    } else if wi >= t5.0 as i64 && wi <= t5.1 as i64 {
        let i = mbfl_bisec_srch2(w as i32 - 0xF0000, t5.2, t5.4);
        if i >= 0 {
            return t5.3[i as usize] as u32;
        }
    }
    0
}

pub fn docomo_tables() -> [(i32, i32, &'static [u16], &'static [u16], i32); 3] {
    [
        (mb_tbl_uni_docomo2code2_min, mb_tbl_uni_docomo2code2_max, &mb_tbl_uni_docomo2code2_key, &mb_tbl_uni_docomo2code2_value, mb_tbl_uni_docomo2code2_len),
        (mb_tbl_uni_docomo2code3_min, mb_tbl_uni_docomo2code3_max, &mb_tbl_uni_docomo2code3_key, &mb_tbl_uni_docomo2code3_value, mb_tbl_uni_docomo2code3_len),
        (mb_tbl_uni_docomo2code5_min, mb_tbl_uni_docomo2code5_max, &mb_tbl_uni_docomo2code5_key, &mb_tbl_uni_docomo2code5_val, mb_tbl_uni_docomo2code5_len),
    ]
}

pub fn kddi_tables() -> [(i32, i32, &'static [u16], &'static [u16], i32); 3] {
    [
        (mb_tbl_uni_kddi2code2_min, mb_tbl_uni_kddi2code2_max, &mb_tbl_uni_kddi2code2_key, &mb_tbl_uni_kddi2code2_value, mb_tbl_uni_kddi2code2_len),
        (mb_tbl_uni_kddi2code3_min, mb_tbl_uni_kddi2code3_max, &mb_tbl_uni_kddi2code3_key, &mb_tbl_uni_kddi2code3_value, mb_tbl_uni_kddi2code3_len),
        (mb_tbl_uni_kddi2code5_min, mb_tbl_uni_kddi2code5_max, &mb_tbl_uni_kddi2code5_key, &mb_tbl_uni_kddi2code5_val, mb_tbl_uni_kddi2code5_len),
    ]
}

pub fn sb_tables() -> [(i32, i32, &'static [u16], &'static [u16], i32); 3] {
    [
        (mb_tbl_uni_sb2code2_min, mb_tbl_uni_sb2code2_max, &mb_tbl_uni_sb2code2_key, &mb_tbl_uni_sb2code2_value, mb_tbl_uni_sb2code2_len),
        (mb_tbl_uni_sb2code3_min, mb_tbl_uni_sb2code3_max, &mb_tbl_uni_sb2code3_key, &mb_tbl_uni_sb2code3_value, mb_tbl_uni_sb2code3_len),
        (mb_tbl_uni_sb2code5_min, mb_tbl_uni_sb2code5_max, &mb_tbl_uni_sb2code5_key, &mb_tbl_uni_sb2code5_val, mb_tbl_uni_sb2code5_len),
    ]
}

#[inline]
pub fn emoji_lookup3(w: u32, t: &[(i32, i32, &'static [u16], &'static [u16], i32); 3]) -> u32 {
    emoji_lookup(w, t[0], t[1], t[2])
}

/// Unicode -> JIS X 0208 via the common ucs_*_jis tables (0 if unmapped)
#[inline]
pub fn ucs_to_jis(w: u32) -> u32 {
    let wi = w as i64;
    if wi >= ucs_a1_jis_table_min as i64 && wi < ucs_a1_jis_table_max as i64 {
        ucs_a1_jis_table[(w - ucs_a1_jis_table_min as u32) as usize] as u32
    } else if wi >= ucs_a2_jis_table_min as i64 && wi < ucs_a2_jis_table_max as i64 {
        ucs_a2_jis_table[(w - ucs_a2_jis_table_min as u32) as usize] as u32
    } else if wi >= ucs_i_jis_table_min as i64 && wi < ucs_i_jis_table_max as i64 {
        ucs_i_jis_table[(w - ucs_i_jis_table_min as u32) as usize] as u32
    } else if wi >= ucs_r_jis_table_min as i64 && wi < ucs_r_jis_table_max as i64 {
        ucs_r_jis_table[(w - ucs_r_jis_table_min as u32) as usize] as u32
    } else {
        0
    }
}

/* ISO-2022-JP-MOBILE#KDDI */

const ASCII: u32 = 0;
const JISX0201_KANA: u32 = 0x20;
const JISX0208_KANJI: u32 = 0x80;

fn mb_iso2022jp_kddi_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c == 0x1B {
            if e - p < 2 {
                p = e;
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            let c3 = input[p + 1];
            p += 2;
            if c2 == b'$' {
                if c3 == b'@' || c3 == b'B' {
                    *state = JISX0208_KANJI;
                } else if c3 == b'(' {
                    if p == e {
                        out.push(MBFL_BAD_INPUT);
                        break;
                    }
                    let c4 = input[p];
                    p += 1;
                    if c4 == b'@' || c4 == b'B' {
                        *state = JISX0208_KANJI;
                    } else {
                        out.push(MBFL_BAD_INPUT);
                    }
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else if c2 == b'(' {
                if c3 == b'B' || c3 == b'J' {
                    *state = ASCII;
                } else if c3 == b'I' {
                    *state = JISX0201_KANA;
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else {
                p -= 1;
                out.push(MBFL_BAD_INPUT);
            }
        } else if *state == JISX0201_KANA && (0x21..=0x5F).contains(&c) {
            out.push(0xFF40 + c as u32);
        } else if *state == JISX0208_KANJI && (0x21..=0x7F).contains(&c) {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;
            if (0x21..=0x7E).contains(&c2) {
                let mut s = ((c as u32 - 0x21) * 94) + c2 as u32 - 0x21;
                let mut w: u32 = 0;
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
                if s >= 84 * 94 && s < 91 * 94 {
                    let mut snd = 0;
                    s += 22 * 94;
                    w = mbfilter_sjis_emoji_kddi2unicode(s as i32, &mut snd) as u32;
                    if w != 0 && snd != 0 {
                        out.push(snd as u32);
                    }
                }
                if w == 0 {
                    if s >= cp932ext1_ucs_table_min as u32 && s < cp932ext1_ucs_table_max as u32 {
                        w = cp932ext1_ucs_table[(s - cp932ext1_ucs_table_min as u32) as usize] as u32;
                    } else if s < jisx0208_ucs_table_size as u32 {
                        w = jisx0208_ucs_table[s as usize] as u32;
                    }
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

#[inline]
fn kuten_to_jis_kddi(s1: u32) -> u32 {
    (((s1 / 94) + 0x21) << 8) + ((s1 % 94) + 0x21) - 0x1600
}

fn mb_wchar_to_iso2022jp_kddi(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let len = input.len();
    let mut i = 0usize;
    let kt = kddi_tables();
    while i < len {
        let w = input[i];
        i += 1;
        let mut s = ucs_to_jis(w);

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

        if (w == '#' as u32 || (w >= '0' as u32 && w <= '9' as u32)) && i < len {
            let w2 = input[i];
            i += 1;
            if w2 == 0x20E3 {
                let s1 = if w == '#' as u32 {
                    0x25BC
                } else if w == '0' as u32 {
                    0x2830
                } else {
                    0x27A6 + (w - '1' as u32)
                };
                s = kuten_to_jis_kddi(s1);
            } else {
                i -= 1;
            }
        } else if w >= nflags(b'C') && w <= nflags(b'U') && i < len {
            let w2 = input[i];
            i += 1;
            let mut found = false;
            if w2 >= nflags(b'B') && w2 <= nflags(b'U') {
                for k in 0..10 {
                    if w == nflags(nflags_s[k][0]) && w2 == nflags(nflags_s[k][1]) {
                        s = kuten_to_jis_kddi(nflags_code_kddi[k] as u32);
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                i -= 1;
            }
        }

        if w == 0xA9 {
            s = kuten_to_jis_kddi(0x27DC);
        } else if w == 0xAE {
            s = kuten_to_jis_kddi(0x27DD);
        } else {
            let s1 = emoji_lookup3(w, &kt);
            if s1 != 0 {
                s = kuten_to_jis_kddi(s1);
            }
        }

        if s == 0 || s >= 0xA1A1 {
            s = 0;
            for k in 0..(cp932ext1_ucs_table_max - cp932ext1_ucs_table_min) as usize {
                if w == cp932ext1_ucs_table[k] as u32 {
                    s = ((((k / 94) + 0x2D) << 8) + (k % 94) + 0x21) as u32;
                    break;
                }
            }
            if w == 0 {
                s = 0;
            }
        }

        if s == 0 && w != 0 {
            mb_illegal_output(w, mb_wchar_to_iso2022jp_kddi, buf);
        } else if s <= 0x7F {
            if buf.state != ASCII {
                buf.add3(0x1B, b'(', b'B');
                buf.state = ASCII;
            }
            buf.add(s as u8);
        } else if (0xA1..=0xDF).contains(&s) {
            if buf.state != JISX0201_KANA {
                buf.add3(0x1B, b'(', b'I');
                buf.state = JISX0201_KANA;
            }
            buf.add((s & 0x7F) as u8);
        } else if s <= 0x7E7E {
            if buf.state != JISX0208_KANJI {
                buf.add3(0x1B, b'$', b'B');
                buf.state = JISX0208_KANJI;
            }
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_iso2022jp_kddi, buf);
        }
    }

    if end && buf.state != ASCII {
        buf.add3(0x1B, b'(', b'B');
    }
}

pub static ENC_2022JP_KDDI: Encoding = Encoding {
    no: EncNo::Iso2022jpKddi,
    name: "ISO-2022-JP-MOBILE#KDDI",
    mime_name: Some("ISO-2022-JP"),
    aliases: &["ISO-2022-JP-KDDI"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_iso2022jp_kddi_to_wchar),
    from_wchar: Some(mb_wchar_to_iso2022jp_kddi),
    check: None,
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[&ENC_2022JP_KDDI];
