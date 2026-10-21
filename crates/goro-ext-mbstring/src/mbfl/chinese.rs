//! Chinese and Korean multi-byte encodings: EUC-CN, EUC-TW, EUC-KR, UHC,
//! GB18030, CP936, GB18030-2022, BIG-5, CP950, HZ
//! (C: the second half of mbfilter_cjk.c)

use super::common::{mbfl_bisec_srch, mbfl_bisec_srch2};
use super::tables::filt_cjk::{gb18030_2022_pua_tbl3, gb18030_pua_tbl3, mblen_table_81_to_fe, mblen_table_euccn};
use super::tables::unicode_table_big5::*;
use super::tables::unicode_table_cns11643::*;
use super::tables::unicode_table_cp936::*;
use super::tables::unicode_table_gb18030::*;
use super::tables::unicode_table_gb2312::*;
use super::tables::unicode_table_uhc::*;
use super::*;

#[inline]
fn inr(w: u32, min: i32, max: i32) -> bool {
    w >= min as u32 && w < max as u32
}

/* ===== EUC-CN ===== */

fn mb_euccn_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if ((0xA1..=0xA9).contains(&c) || (0xB0..=0xF7).contains(&c)) && p < e {
            let c2 = input[p];
            p += 1;
            if (0xA1..=0xFE).contains(&c2) {
                let mut w = (c as u32 - 0x81) * 192 + c2 as u32 - 0x40;
                if w == 0x1864 {
                    w = 0x30FB;
                } else if w == 0x186A {
                    w = 0x2015;
                } else if (0x1921..=0x192A).contains(&w) || w == 0x1963 || (0x1C59..=0x1C7E).contains(&w) || (0x1DBB..=0x1DC4).contains(&w) {
                    w = 0;
                } else {
                    w = cp936_ucs_table[w as usize] as u32;
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

fn mb_wchar_to_euccn(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s: u32 = 0;
        if inr(w, ucs_a1_cp936_table_min, ucs_a1_cp936_table_max) {
            if w != 0xB7 && w != 0x144 && w != 0x148 && w != 0x251 && w != 0x261 {
                s = ucs_a1_cp936_table[(w as i32 - ucs_a1_cp936_table_min) as usize] as u32;
            }
        } else if inr(w, ucs_a2_cp936_table_min, ucs_a2_cp936_table_max) {
            if w == 0x2015 {
                s = 0xA1AA;
            } else if w != 0x2014 && !(0x2170..=0x2179).contains(&w) {
                s = ucs_a2_cp936_table[(w as i32 - ucs_a2_cp936_table_min) as usize] as u32;
            }
        } else if inr(w, ucs_a3_cp936_table_min, ucs_a3_cp936_table_max) {
            if w == 0x30FB {
                s = 0xA1A4;
            } else {
                s = ucs_a3_cp936_table[(w as i32 - ucs_a3_cp936_table_min) as usize] as u32;
            }
        } else if inr(w, ucs_i_cp936_table_min, ucs_i_cp936_table_max) {
            s = ucs_i_cp936_table[(w as i32 - ucs_i_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_hff_cp936_table_min, ucs_hff_cp936_table_max) {
            if w == 0xFF04 {
                s = 0xA1E7;
            } else if w == 0xFF5E {
                s = 0xA1AB;
            } else if (0xFF01..=0xFF5D).contains(&w) {
                s = w - 0xFF01 + 0xA3A1;
            } else if (0xFFE0..=0xFFE5).contains(&w) {
                s = ucs_hff_s_cp936_table[(w - 0xFFE0) as usize] as u32;
            }
        }

        // Exclude CP936 extensions
        if ((s >> 8) & 0xFF) < 0xA1 || (s & 0xFF) < 0xA1 {
            s = 0;
        }

        if s == 0 {
            if w < 0x80 {
                buf.add(w as u8);
            } else {
                mb_illegal_output(w, mb_wchar_to_euccn, buf);
            }
        } else if s < 0x80 {
            buf.add(s as u8);
        } else {
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        }
    }
}

pub static ENC_EUC_CN: Encoding = Encoding {
    no: EncNo::EucCn,
    name: "EUC-CN",
    mime_name: Some("CN-GB"),
    aliases: &["CN-GB", "EUC_CN", "eucCN", "x-euc-cn", "gb2312"],
    mblen_table: Some(&mblen_table_euccn),
    flag: 0,
    to_wchar: Some(mb_euccn_to_wchar),
    from_wchar: Some(mb_wchar_to_euccn),
    check: None,
    cut: None,
};

/* ===== EUC-TW ===== */

fn mb_euctw_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if ((0xA1..=0xA6).contains(&c) || (0xC2..=0xFD).contains(&c)) && c != 0xC3 && p < e {
            let c2 = input[p];
            p += 1;
            if (0xA1..=0xFE).contains(&c2) {
                let w = (c as u32 - 0xA1) * 94 + (c2 as u32 - 0xA1);
                let w = if (w as i32) < cns11643_1_ucs_table_size { cns11643_1_ucs_table[w as usize] as u32 } else { 0 };
                out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c == 0x8E && p < e {
            let c2 = input[p];
            p += 1;
            if (c2 == 0xA1 || c2 == 0xA2 || c2 == 0xAE) && p < e {
                let plane = (c2 - 0xA1) as u32;
                let c3 = input[p];
                p += 1;
                if c3 >= 0xA1
                    && ((plane == 0 && ((0xA1..=0xA6).contains(&c3) || (0xC2..=0xFD).contains(&c3)) && c3 != 0xC3)
                        || (plane == 1 && c3 <= 0xF2)
                        || (plane == 13 && c3 <= 0xE7))
                    && p < e
                {
                    let c4 = input[p];
                    p += 1;
                    if c2 <= 0xAE && c4 > 0xA0 && c4 < 0xFF {
                        let s = (c3 as u32 - 0xA1) * 94 + c4 as u32 - 0xA1;
                        let mut w = 0u32;
                        if plane == 0 && (s as i32) < cns11643_1_ucs_table_size {
                            w = cns11643_1_ucs_table[s as usize] as u32;
                        } else if plane == 1 && (s as i32) < cns11643_2_ucs_table_size {
                            w = cns11643_2_ucs_table[s as usize] as u32;
                        } else if plane == 13 && (s as i32) < cns11643_14_ucs_table_size {
                            w = cns11643_14_ucs_table[s as usize] as u32;
                        }
                        out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
                        continue;
                    }
                }
            }
            out.push(MBFL_BAD_INPUT);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_euctw(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s: u32 = 0;
        if inr(w, ucs_a1_cns11643_table_min, ucs_a1_cns11643_table_max) {
            s = ucs_a1_cns11643_table[(w as i32 - ucs_a1_cns11643_table_min) as usize];
        } else if inr(w, ucs_a2_cns11643_table_min, ucs_a2_cns11643_table_max) {
            s = ucs_a2_cns11643_table[(w as i32 - ucs_a2_cns11643_table_min) as usize];
        } else if inr(w, ucs_a3_cns11643_table_min, ucs_a3_cns11643_table_max) {
            s = ucs_a3_cns11643_table[(w as i32 - ucs_a3_cns11643_table_min) as usize];
        } else if inr(w, ucs_i_cns11643_table_min, ucs_i_cns11643_table_max) {
            s = ucs_i_cns11643_table[(w as i32 - ucs_i_cns11643_table_min) as usize];
        } else if inr(w, ucs_r_cns11643_table_min, ucs_r_cns11643_table_max) {
            s = ucs_r_cns11643_table[(w as i32 - ucs_r_cns11643_table_min) as usize];
        }

        if s == 0 {
            if w == 0 {
                buf.add(0);
            } else {
                mb_illegal_output(w, mb_wchar_to_euctw, buf);
            }
        } else {
            let plane = s >> 16;
            if plane <= 1 {
                if s < 0x80 {
                    buf.add(s as u8);
                } else {
                    buf.add2((((s >> 8) & 0xFF) | 0x80) as u8, ((s & 0xFF) | 0x80) as u8);
                }
            } else {
                buf.add4(0x8E, (0xA0 + plane) as u8, (((s >> 8) & 0xFF) | 0x80) as u8, ((s & 0xFF) | 0x80) as u8);
            }
        }
    }
}

pub static ENC_EUC_TW: Encoding = Encoding {
    no: EncNo::EucTw,
    name: "EUC-TW",
    mime_name: Some("EUC-TW"),
    aliases: &["EUC_TW", "eucTW", "x-euc-tw"],
    mblen_table: Some(&mblen_table_euccn),
    flag: 0,
    to_wchar: Some(mb_euctw_to_wchar),
    from_wchar: Some(mb_wchar_to_euctw),
    check: None,
    cut: None,
};

/* ===== EUC-KR ===== */

fn mb_euckr_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if ((0xA1..=0xAC).contains(&c) || (0xB0..=0xFD).contains(&c)) && c != 0xC9 && p < e {
            let c2 = input[p];
            p += 1;
            if c2 < 0xA1 || c2 == 0xFF {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            let w = if c <= 0xC6 {
                uhc1_ucs_table[((c as u32 - 0x81) * 190 + c2 as u32 - 0x41) as usize] as u32
            } else {
                uhc3_ucs_table[((c as u32 - 0xC7) * 94 + c2 as u32 - 0xA1) as usize] as u32
            };
            out.push(if w == 0 { MBFL_BAD_INPUT } else { w });
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

#[inline]
fn uhc_lookup(w: u32) -> u32 {
    if inr(w, ucs_a1_uhc_table_min, ucs_a1_uhc_table_max) {
        ucs_a1_uhc_table[(w as i32 - ucs_a1_uhc_table_min) as usize] as u32
    } else if inr(w, ucs_a2_uhc_table_min, ucs_a2_uhc_table_max) {
        ucs_a2_uhc_table[(w as i32 - ucs_a2_uhc_table_min) as usize] as u32
    } else if inr(w, ucs_a3_uhc_table_min, ucs_a3_uhc_table_max) {
        ucs_a3_uhc_table[(w as i32 - ucs_a3_uhc_table_min) as usize] as u32
    } else if inr(w, ucs_i_uhc_table_min, ucs_i_uhc_table_max) {
        ucs_i_uhc_table[(w as i32 - ucs_i_uhc_table_min) as usize] as u32
    } else if inr(w, ucs_s_uhc_table_min, ucs_s_uhc_table_max) {
        ucs_s_uhc_table[(w as i32 - ucs_s_uhc_table_min) as usize] as u32
    } else if inr(w, ucs_r1_uhc_table_min, ucs_r1_uhc_table_max) {
        ucs_r1_uhc_table[(w as i32 - ucs_r1_uhc_table_min) as usize] as u32
    } else if inr(w, ucs_r2_uhc_table_min, ucs_r2_uhc_table_max) {
        ucs_r2_uhc_table[(w as i32 - ucs_r2_uhc_table_min) as usize] as u32
    } else {
        0
    }
}

fn mb_wchar_to_euckr(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s = uhc_lookup(w);
        // Exclude UHC extension area
        if ((s >> 8) & 0xFF) < 0xA1 || (s & 0xFF) < 0xA1 {
            s = 0;
        }
        if s == 0 {
            if w < 0x80 {
                buf.add(w as u8);
            } else {
                mb_illegal_output(w, mb_wchar_to_euckr, buf);
            }
        } else if s < 0x80 {
            buf.add(s as u8);
        } else {
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        }
    }
}

pub static ENC_EUC_KR: Encoding = Encoding {
    no: EncNo::EucKr,
    name: "EUC-KR",
    mime_name: Some("EUC-KR"),
    aliases: &["EUC_KR", "eucKR", "x-euc-kr"],
    mblen_table: Some(&mblen_table_euccn),
    flag: 0,
    to_wchar: Some(mb_euckr_to_wchar),
    from_wchar: Some(mb_wchar_to_euckr),
    check: None,
    cut: None,
};

/* ===== UHC ===== */

fn mb_uhc_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    if input.is_empty() {
        return;
    }
    let e = input.len() - 1; // Stop the main loop 1 byte short of the end
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if c > 0x80 && c < 0xFE {
            let c2 = input[p];
            p += 1;
            if c2 < 0x41 || c2 == 0xFF {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            let mut w = 0u32;
            if c <= 0xC6 {
                w = uhc1_ucs_table[((c as u32 - 0x81) * 190 + c2 as u32 - 0x41) as usize] as u32;
            } else if c2 >= 0xA1 {
                w = uhc3_ucs_table[((c as u32 - 0xC7) * 94 + c2 as u32 - 0xA1) as usize] as u32;
            }
            if w == 0 {
                if c == 0xC9 {
                    p -= 1;
                }
                w = MBFL_BAD_INPUT;
            }
            out.push(w);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
    // Finish up last byte of input string if there is one
    if p == e {
        let c = input[p];
        out.push(if c < 0x80 { c as u32 } else { MBFL_BAD_INPUT });
    }
}

fn mb_wchar_to_uhc(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let s = uhc_lookup(w);
        if s == 0 {
            if w == 0 {
                buf.add(0);
            } else {
                mb_illegal_output(w, mb_wchar_to_uhc, buf);
            }
        } else if s < 0x80 {
            buf.add(s as u8);
        } else {
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        }
    }
}

pub static ENC_UHC: Encoding = Encoding {
    no: EncNo::Uhc,
    name: "UHC",
    mime_name: Some("UHC"),
    aliases: &["CP949"],
    mblen_table: Some(&mblen_table_81_to_fe),
    flag: 0,
    to_wchar: Some(mb_uhc_to_wchar),
    from_wchar: Some(mb_wchar_to_uhc),
    check: None,
    cut: None,
};

/* ===== GB18030 / CP936 ===== */

#[inline]
fn gb_bmp_4byte(w: u32) -> u32 {
    let i = mbfl_bisec_srch(w as i32, &mbfl_gb2uni_tbl, mbfl_gb_uni_max);
    if i < 0 {
        return MBFL_BAD_INPUT;
    }
    w + mbfl_gb_uni_ofst[i as usize] as u32
}

/// Shared by GB18030 and GB18030-2022 decoders; `y2022` selects the variant
fn gb18030_to_wchar_impl(input: &[u8], out: &mut Vec<u32>, y2022: bool) {
    let e = input.len();
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if c == 0x80 || c == 0xFF {
            out.push(MBFL_BAD_INPUT);
        } else {
            if p == e {
                out.push(MBFL_BAD_INPUT);
                break;
            }
            let c2 = input[p];
            p += 1;

            if ((0x81..=0x84).contains(&c) || (0x90..=0xE3).contains(&c)) && (0x30..=0x39).contains(&c2) {
                if p >= e {
                    out.push(MBFL_BAD_INPUT);
                    break;
                }
                let c3 = input[p];
                p += 1;
                if (0x81..=0xFE).contains(&c3) && p < e {
                    let c4 = input[p];
                    p += 1;
                    if (0x30..=0x39).contains(&c4) {
                        if (0x90..=0xE3).contains(&c) {
                            let w = (((c as u32 - 0x90) * 10 + (c2 as u32 - 0x30)) * 126 + (c3 as u32 - 0x81)) * 10 + (c4 as u32 - 0x30) + 0x10000;
                            out.push(if w > 0x10FFFF { MBFL_BAD_INPUT } else { w });
                        } else {
                            // Unicode BMP
                            let w = (((c as u32 - 0x81) * 10 + (c2 as u32 - 0x30)) * 126 + (c3 as u32 - 0x81)) * 10 + (c4 as u32 - 0x30);
                            if !y2022 {
                                if w <= 39419 {
                                    out.push(gb_bmp_4byte(w));
                                } else {
                                    out.push(MBFL_BAD_INPUT);
                                }
                            } else if w == 0x98A4 {
                                out.push(0xE78D);
                            } else if w == 0x98A6 {
                                out.push(0xE78E);
                            } else if w == 0x98A5 {
                                out.push(0xE78F);
                            } else if (0x98A7..=0x98AD).contains(&w) {
                                out.push(w + (0xE790 - 0x98A7));
                            } else if w == 0x1D21 {
                                out.push(0xE7C7);
                            } else if w == 0x4A71 {
                                out.push(0xE81E);
                            } else if w == 0x4A72 {
                                out.push(0xE826);
                            } else if (0x4A73..=0x4A74).contains(&w) {
                                out.push(w + (0xE82B - 0x4A73));
                            } else if w == 0x4A75 {
                                out.push(0xE832);
                            } else if w == 0x4A76 {
                                out.push(0xE843);
                            } else if w == 0x4A77 {
                                out.push(0xE854);
                            } else if w == 0x4A78 {
                                out.push(0xE864);
                            } else if w <= 0x99FB {
                                out.push(gb_bmp_4byte(w));
                            } else {
                                out.push(MBFL_BAD_INPUT);
                            }
                        }
                    } else {
                        out.push(MBFL_BAD_INPUT);
                    }
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else if ((0xAA..=0xAF).contains(&c) || (0xF8..=0xFE).contains(&c)) && (0xA1..=0xFE).contains(&c2) {
                // UDA part 1, 2: U+E000-U+E4C5
                out.push(94 * (if c >= 0xF8 { c as u32 - 0xF2 } else { c as u32 - 0xAA }) + (c2 as u32 - 0xA1) + 0xE000);
            } else if (0xA1..=0xA7).contains(&c) && c2 >= 0x40 && c2 < 0xA1 && c2 != 0x7F {
                // UDA part 3: U+E4C6-U+E765
                out.push(96 * (c as u32 - 0xA1) + c2 as u32 - (if c2 >= 0x80 { 0x41 } else { 0x40 }) + 0xE4C6);
            } else if c2 >= 0x40 && c2 != 0x7F && c2 != 0xFF {
                let w = (c as u32 - 0x81) * 192 + c2 as u32 - 0x40;
                if w >= 0x192B {
                    if w <= 0x1EBE {
                        if w != 0x1963 && w != 0x1DBF && !(0x1E49..=0x1E55).contains(&w) && w != 0x1E7F {
                            let tbl: &[u16] = if y2022 { &gb18030_2022_pua_tbl1 } else { &cp936_pua_tbl1 };
                            out.push(tbl[(w - 0x192B) as usize] as u32);
                            continue;
                        }
                    } else if w >= 0x413A {
                        if w <= 0x413E {
                            out.push(cp936_pua_tbl2[(w - 0x413A) as usize] as u32);
                            continue;
                        } else if (0x5DD0..=0x5E20).contains(&w) {
                            let tbl: &[u16] = if y2022 { &gb18030_2022_pua_tbl3 } else { &gb18030_pua_tbl3 };
                            let c = tbl[(w - 0x5DD0) as usize] as u32;
                            if c != 0 {
                                out.push(c);
                                continue;
                            }
                        }
                    }
                }
                if (0x81..=0xA9).contains(&c) || ((0xB0..=0xF7).contains(&c) && c2 >= 0xA1) || ((0xAA..=0xFE).contains(&c) && c2 <= 0xA0) {
                    out.push(cp936_ucs_table[w as usize] as u32);
                } else {
                    out.push(MBFL_BAD_INPUT);
                }
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
    }
}

fn mb_gb18030_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    gb18030_to_wchar_impl(input, out, false);
}

fn mb_gb18030_2022_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    gb18030_to_wchar_impl(input, out, true);
}

#[inline]
fn gb_pua_encode(w: u32, tbl: &[[u16; 3]]) -> u32 {
    if w < 0xE766 {
        if w < 0xE4C6 {
            let mut c1 = w - 0xE000;
            let mut s = (c1 % 94) + 0xA1;
            c1 /= 94;
            s |= (c1 + (if c1 < 0x06 { 0xAA } else { 0xF2 })) << 8;
            s
        } else {
            let mut c1 = w - 0xE4C6;
            let mut s = ((c1 / 96) + 0xA1) << 8;
            c1 %= 96;
            s |= c1 + (if c1 >= 0x3F { 0x41 } else { 0x40 });
            s
        }
    } else {
        // U+E766-U+E864
        let (mut k1, mut k2) = (0usize, tbl.len());
        while k1 < k2 {
            let k = (k1 + k2) >> 1;
            if w < tbl[k][0] as u32 {
                k2 = k;
            } else if w > tbl[k][1] as u32 {
                k1 = k + 1;
            } else {
                return w - tbl[k][0] as u32 + tbl[k][2] as u32;
            }
        }
        0
    }
}

#[inline]
fn gb_4byte_encode(mut c1: u32, top: u32) -> u32 {
    let mut s = (c1 % 10) + 0x30;
    c1 /= 10;
    s |= ((c1 % 126) + 0x81) << 8;
    c1 /= 126;
    s |= ((c1 % 10) + 0x30) << 16;
    c1 /= 10;
    s |= (c1 + top) << 24;
    s
}

fn gb18030_emit(s: u32, w: u32, buf: &mut ConvBuf) {
    if s == 0 {
        // Note: GB18030-2022 also uses the GB18030 encoder for error markers, as in C
        mb_illegal_output(w, mb_wchar_to_gb18030, buf);
    } else if s < 0x80 {
        buf.add(s as u8);
    } else if s > 0xFFFFFF {
        buf.add4(((s >> 24) & 0xFF) as u8, ((s >> 16) & 0xFF) as u8, ((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
    } else {
        buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
    }
}

#[inline]
fn gb_ci_lookup(w: u32) -> u32 {
    if w == 0xF92C {
        0xFD9C
    } else if w == 0xF979 {
        0xFD9D
    } else if w == 0xF995 {
        0xFD9E
    } else if w == 0xF9E7 {
        0xFD9F
    } else if w == 0xF9F1 {
        0xFDA0
    } else if (0xFA0C..=0xFA29).contains(&w) {
        ucs_ci_s_cp936_table[(w - 0xFA0C) as usize] as u32
    } else {
        0
    }
}

#[inline]
fn hff_lookup(w: u32) -> u32 {
    if w == 0xFF04 {
        0xA1E7
    } else if w == 0xFF5E {
        0xA1AB
    } else if (0xFF01..=0xFF5D).contains(&w) {
        w - 0xFF01 + 0xA3A1
    } else if (0xFFE0..=0xFFE5).contains(&w) {
        ucs_hff_s_cp936_table[(w - 0xFFE0) as usize] as u32
    } else {
        0
    }
}

#[inline]
fn gb18030_c_tbl(w: u32) -> u32 {
    let n = mbfl_gb18030_c_tbl_max;
    if w >= mbfl_gb18030_c_tbl_key[0] as u32 && w <= mbfl_gb18030_c_tbl_key[(n - 1) as usize] as u32 {
        let i = mbfl_bisec_srch2(w as i32, &mbfl_gb18030_c_tbl_key, n);
        if i >= 0 {
            return mbfl_gb18030_c_tbl_val[i as usize] as u32;
        }
    }
    0
}

fn mb_wchar_to_gb18030(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s: u32 = 0;
        if w == 0 {
            buf.add(0);
            continue;
        } else if inr(w, ucs_a1_cp936_table_min, ucs_a1_cp936_table_max) {
            s = if w == 0x1F9 { 0xA8BF } else { ucs_a1_cp936_table[(w as i32 - ucs_a1_cp936_table_min) as usize] as u32 };
        } else if inr(w, ucs_a2_cp936_table_min, ucs_a2_cp936_table_max) {
            s = if w == 0x20AC { 0xA2E3 } else { ucs_a2_cp936_table[(w as i32 - ucs_a2_cp936_table_min) as usize] as u32 };
        } else if inr(w, ucs_a3_cp936_table_min, ucs_a3_cp936_table_max) {
            s = ucs_a3_cp936_table[(w as i32 - ucs_a3_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_i_cp936_table_min, ucs_i_cp936_table_max) {
            s = ucs_i_cp936_table[(w as i32 - ucs_i_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_ci_cp936_table_min, ucs_ci_cp936_table_max) {
            s = gb_ci_lookup(w);
        } else if inr(w, ucs_cf_cp936_table_min, ucs_cf_cp936_table_max) {
            s = ucs_cf_cp936_table[(w as i32 - ucs_cf_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_sfv_cp936_table_min, ucs_sfv_cp936_table_max) {
            s = ucs_sfv_cp936_table[(w as i32 - ucs_sfv_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_hff_cp936_table_min, ucs_hff_cp936_table_max) {
            s = hff_lookup(w);
        } else if (0xE000..=0xE864).contains(&w) {
            s = gb_pua_encode(w, &mbfl_gb18030_pua_tbl);
        }

        if s == 0 {
            s = gb18030_c_tbl(w);
        }

        if s == 0 && (0x80..=0xFFFF).contains(&w) {
            let i = mbfl_bisec_srch(w as i32, &mbfl_uni2gb_tbl, mbfl_gb_uni_max);
            if i >= 0 {
                s = gb_4byte_encode(w - mbfl_gb_uni_ofst[i as usize] as u32, 0x81);
            }
        } else if (0x10000..=0x10FFFF).contains(&w) {
            s = gb_4byte_encode(w - 0x10000, 0x90);
        }

        gb18030_emit(s, w, buf);
    }
}

fn mb_wchar_to_gb18030_2022(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s: u32 = 0;
        if w == 0 {
            buf.add(0);
            continue;
        } else if inr(w, ucs_a1_cp936_table_min, ucs_a1_cp936_table_max) {
            s = if w == 0x1F9 { 0xA8BF } else { ucs_a1_cp936_table[(w as i32 - ucs_a1_cp936_table_min) as usize] as u32 };
        } else if inr(w, ucs_a2_cp936_table_min, ucs_a2_cp936_table_max) {
            s = if w == 0x20AC { 0xA2E3 } else { ucs_a2_cp936_table[(w as i32 - ucs_a2_cp936_table_min) as usize] as u32 };
        } else if inr(w, ucs_a3_cp936_table_min, ucs_a3_cp936_table_max) {
            s = ucs_a3_cp936_table[(w as i32 - ucs_a3_cp936_table_min) as usize] as u32;
        } else if (0x9FB4..=0x9FBB).contains(&w) {
            s = match w {
                0x9FB4 => 0xFE59,
                0x9FB5 => 0xFE61,
                0x9FB6 => 0xFE66,
                0x9FB7 => 0xFE67,
                0x9FB8 => 0xFE6D,
                0x9FB9 => 0xFE7E,
                0x9FBA => 0xFE90,
                _ => 0xFEA0,
            };
        } else if inr(w, ucs_i_cp936_table_min, ucs_i_cp936_table_max) {
            s = ucs_i_cp936_table[(w as i32 - ucs_i_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_ci_cp936_table_min, ucs_ci_cp936_table_max) {
            s = gb_ci_lookup(w);
        } else if inr(w, ucs_cf_cp936_table_min, ucs_cf_cp936_table_max) {
            s = ucs_cf_cp936_table[(w as i32 - ucs_cf_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_sfv_cp936_table_min, ucs_sfv_cp936_table_max) {
            s = ucs_sfv_cp936_table[(w as i32 - ucs_sfv_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_hff_cp936_table_min, ucs_hff_cp936_table_max) {
            s = hff_lookup(w);
        } else if (0xE000..=0xE864).contains(&w) {
            s = gb_pua_encode(w, &mbfl_gb18030_2022_pua_tbl);
        } else if (0xFE10..=0xFE19).contains(&w) {
            s = if w == 0xFE11 {
                0xA6DB
            } else if w == 0xFE12 {
                0xA6DA
            } else if w <= 0xFE16 {
                w - (0xFE10 - 0xA6D9)
            } else if w <= 0xFE18 {
                w - (0xFE17 - 0xA6EC)
            } else {
                0xA6F3
            };
        } else if w == 0x1E3F {
            s = 0xA8BC;
        }

        if s == 0 {
            s = gb18030_c_tbl(w);
        }

        if s == 0 && (0x80..=0xFFFF).contains(&w) {
            let i = mbfl_bisec_srch(w as i32, &mbfl_uni2gb2022_tbl, mbfl_gb2022_uni_max);
            if i >= 0 {
                s = gb_4byte_encode(w - mbfl_gb2022_uni_ofst[i as usize] as u32, 0x81);
            }
        } else if (0x10000..=0x10FFFF).contains(&w) {
            s = gb_4byte_encode(w - 0x10000, 0x90);
        }

        gb18030_emit(s, w, buf);
    }
}

fn mb_cp936_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if c == 0x80 {
            out.push(0x20AC); // Euro sign
        } else if c < 0xFF {
            if p >= e {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            let c2 = input[p];
            p += 1;
            if c2 < 0x40 || c2 == 0x7F || c2 == 0xFF {
                out.push(MBFL_BAD_INPUT);
                continue;
            }
            if ((0xAA..=0xAF).contains(&c) || (0xF8..=0xFE).contains(&c)) && c2 >= 0xA1 {
                out.push(94 * (if c >= 0xF8 { c as u32 - 0xF2 } else { c as u32 - 0xAA }) + (c2 as u32 - 0xA1) + 0xE000);
            } else if (0xA1..=0xA7).contains(&c) && c2 < 0xA1 {
                out.push(96 * (c as u32 - 0xA1) + c2 as u32 - (if c2 >= 0x80 { 0x41 } else { 0x40 }) + 0xE4C6);
            } else {
                let w = (c as u32 - 0x81) * 192 + c2 as u32 - 0x40;
                if w >= 0x192B {
                    if w <= 0x1EBE {
                        out.push(cp936_pua_tbl1[(w - 0x192B) as usize] as u32);
                        continue;
                    } else if w >= 0x413A {
                        if w <= 0x413E {
                            out.push(cp936_pua_tbl2[(w - 0x413A) as usize] as u32);
                            continue;
                        } else if (0x5DD0..=0x5E20).contains(&w) {
                            out.push(cp936_pua_tbl3[(w - 0x5DD0) as usize] as u32);
                            continue;
                        }
                    }
                }
                out.push(cp936_ucs_table[w as usize] as u32);
            }
        } else {
            out.push(0xF8F5);
        }
    }
}

fn mb_wchar_to_cp936(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s: u32 = 0;
        if inr(w, ucs_a1_cp936_table_min, ucs_a1_cp936_table_max) {
            s = ucs_a1_cp936_table[(w as i32 - ucs_a1_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_a2_cp936_table_min, ucs_a2_cp936_table_max) {
            s = if w == 0x203E {
                0xA3FE
            } else if w == 0x2218 {
                0xA1E3
            } else if w == 0x223C {
                0xA1AB
            } else {
                ucs_a2_cp936_table[(w as i32 - ucs_a2_cp936_table_min) as usize] as u32
            };
        } else if inr(w, ucs_a3_cp936_table_min, ucs_a3_cp936_table_max) {
            s = ucs_a3_cp936_table[(w as i32 - ucs_a3_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_i_cp936_table_min, ucs_i_cp936_table_max) {
            s = ucs_i_cp936_table[(w as i32 - ucs_i_cp936_table_min) as usize] as u32;
        } else if (0xE000..=0xE864).contains(&w) {
            s = gb_pua_encode(w, &mbfl_cp936_pua_tbl);
        } else if w == 0xF8F5 {
            s = 0xFF;
        } else if inr(w, ucs_ci_cp936_table_min, ucs_ci_cp936_table_max) {
            s = ucs_ci_cp936_table[(w as i32 - ucs_ci_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_cf_cp936_table_min, ucs_cf_cp936_table_max) {
            s = ucs_cf_cp936_table[(w as i32 - ucs_cf_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_sfv_cp936_table_min, ucs_sfv_cp936_table_max) {
            s = ucs_sfv_cp936_table[(w as i32 - ucs_sfv_cp936_table_min) as usize] as u32;
        } else if inr(w, ucs_hff_cp936_table_min, ucs_hff_cp936_table_max) {
            s = hff_lookup(w);
        }

        if s == 0 {
            if w == 0 {
                buf.add(0);
            } else {
                mb_illegal_output(w, mb_wchar_to_cp936, buf);
            }
        } else if s <= 0x80 || s == 0xFF {
            buf.add(s as u8);
        } else {
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        }
    }
}

/// Step through a GB18030 string one character at a time
fn step_through_gb18030_str(s: &[u8], mut p: usize, limit: usize) -> usize {
    while p < limit {
        let c = s[p];
        if c < 0x81 || c == 0xFF {
            p += 1;
        } else {
            if limit - p == 1 {
                break;
            }
            let c2 = s[p + 1];
            // For a 4-byte char, the 2nd byte will be 0x30-0x39
            let w = if (0x30..=0x39).contains(&c2) { 4 } else { 2 };
            if limit - p < w {
                break;
            }
            p += w;
        }
    }
    p
}

fn mb_cut_gb18030(s: &[u8], from: usize, mut len: usize) -> Vec<u8> {
    let end = s.len();
    let start = step_through_gb18030_str(s, 0, from);
    if from + len > end {
        len = end - from;
    }
    if start + len >= end {
        s[start..end].to_vec()
    } else {
        let e = step_through_gb18030_str(s, start, start + len);
        s[start..e].to_vec()
    }
}

pub static ENC_GB18030: Encoding = Encoding {
    no: EncNo::Gb18030,
    name: "GB18030",
    mime_name: Some("GB18030"),
    aliases: &["gb-18030", "gb-18030-2000"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_gb18030_to_wchar),
    from_wchar: Some(mb_wchar_to_gb18030),
    check: None,
    cut: Some(mb_cut_gb18030),
};

pub static ENC_CP936: Encoding = Encoding {
    no: EncNo::Cp936,
    name: "CP936",
    mime_name: Some("CP936"),
    aliases: &["CP-936", "GBK"],
    mblen_table: Some(&mblen_table_81_to_fe),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_cp936_to_wchar),
    from_wchar: Some(mb_wchar_to_cp936),
    check: None,
    cut: None,
};

pub static ENC_GB18030_2022: Encoding = Encoding {
    no: EncNo::Gb18030_2022,
    name: "GB18030-2022",
    mime_name: Some("GB18030-2022"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_gb18030_2022_to_wchar),
    from_wchar: Some(mb_wchar_to_gb18030_2022),
    check: None,
    cut: Some(mb_cut_gb18030),
};

/* ===== BIG5 / CP950 ===== */

// 63 + 94 = 157 or 94
static cp950_pua_tbl: [[u16; 4]; 5] = [
    [0xe000, 0xe310, 0xfa40, 0xfefe],
    [0xe311, 0xeeb7, 0x8e40, 0xa0fe],
    [0xeeb8, 0xf6b0, 0x8140, 0x8dfe],
    [0xf6b1, 0xf70e, 0xc6a1, 0xc6fe],
    [0xf70f, 0xf848, 0xc740, 0xc8fe],
];

#[inline]
fn is_in_cp950_pua(c1: u32, c: u32) -> bool {
    if (0xfa..=0xfe).contains(&c1) || (0x8e..=0xa0).contains(&c1) || (0x81..=0x8d).contains(&c1) || (0xc7..=0xc8).contains(&c1) {
        (0x40..=0x7e).contains(&c) || (0xa1..=0xfe).contains(&c)
    } else if c1 == 0xc6 {
        (0xa1..=0xfe).contains(&c)
    } else {
        false
    }
}

fn mb_big5_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    if input.is_empty() {
        return;
    }
    let e = input.len() - 1; // Stop the main loop 1 byte short of the end
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c <= 0x7F {
            out.push(c as u32);
        } else if c > 0xA0 && c <= 0xF9 {
            let c2 = input[p];
            p += 1;
            if (0x40..=0x7E).contains(&c2) || (0xA1..=0xFE).contains(&c2) {
                let idx = (c as u32 - 0xA1) * 157 + c2 as u32 - (if c2 <= 0x7E { 0x40 } else { 0xA1 - 0x3F });
                let mut w = big5_ucs_table[idx as usize] as u32;
                if w == 0 {
                    if c == 0xC8 {
                        p -= 1;
                    }
                    w = MBFL_BAD_INPUT;
                }
                out.push(w);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
    // Finish up last byte of input string if there is one
    if p == e {
        let c = input[p];
        out.push(if c <= 0x7F { c as u32 } else { MBFL_BAD_INPUT });
    }
}

#[inline]
fn big5_lookup(w: u32) -> u32 {
    if inr(w, ucs_a1_big5_table_min, ucs_a1_big5_table_max) {
        ucs_a1_big5_table[(w as i32 - ucs_a1_big5_table_min) as usize] as u32
    } else if inr(w, ucs_a2_big5_table_min, ucs_a2_big5_table_max) {
        ucs_a2_big5_table[(w as i32 - ucs_a2_big5_table_min) as usize] as u32
    } else if inr(w, ucs_a3_big5_table_min, ucs_a3_big5_table_max) {
        ucs_a3_big5_table[(w as i32 - ucs_a3_big5_table_min) as usize] as u32
    } else if inr(w, ucs_i_big5_table_min, ucs_i_big5_table_max) {
        ucs_i_big5_table[(w as i32 - ucs_i_big5_table_min) as usize] as u32
    } else if inr(w, ucs_r1_big5_table_min, ucs_r1_big5_table_max) {
        ucs_r1_big5_table[(w as i32 - ucs_r1_big5_table_min) as usize] as u32
    } else if inr(w, ucs_r2_big5_table_min, ucs_r2_big5_table_max) {
        ucs_r2_big5_table[(w as i32 - ucs_r2_big5_table_min) as usize] as u32
    } else {
        0
    }
}

fn mb_wchar_to_big5(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let s = big5_lookup(w);
        if s == 0 {
            if w == 0 {
                buf.add(0);
            } else {
                mb_illegal_output(w, mb_wchar_to_big5, buf);
            }
        } else if s <= 0x80 {
            buf.add(s as u8);
        } else {
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        }
    }
}

fn mb_cp950_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c <= 0x7F {
            out.push(c as u32);
        } else if c > 0x80 && c <= 0xFE && p < e {
            let c2 = input[p];
            p += 1;
            if (0x40..=0x7E).contains(&c2) || (0xA1..=0xFE).contains(&c2) {
                let (c, c2) = (c as u32, c2 as u32);
                // Note: (c - 0xA1) may be negative in C (signed int arithmetic)
                let idx = (c as i32 - 0xA1) * 157 + c2 as i32 - (if c2 <= 0x7E { 0x40 } else { 0xA1 - 0x3F });
                let mut w = if idx >= 0 && idx < big5_ucs_table_size { big5_ucs_table[idx as usize] as u32 } else { 0 };

                if is_in_cp950_pua(c, c2) {
                    let s = (c << 8) | c2;
                    let mut k = 0;
                    while k < cp950_pua_tbl.len() {
                        if s >= cp950_pua_tbl[k][2] as u32 && s <= cp950_pua_tbl[k][3] as u32 {
                            break;
                        }
                        k += 1;
                    }
                    if (cp950_pua_tbl[k][2] & 0xFF) == 0x40 {
                        w = 157 * (c - (cp950_pua_tbl[k][2] as u32 >> 8)) + c2 - (if c2 >= 0xA1 { 0x62 } else { 0x40 }) + cp950_pua_tbl[k][0] as u32;
                    } else {
                        w = s - cp950_pua_tbl[k][2] as u32 + cp950_pua_tbl[k][0] as u32;
                    }
                } else if c == 0xA1 {
                    match c2 {
                        0x45 => w = 0x2027,
                        0x4E => w = 0xFE51,
                        0x5A => w = 0x2574,
                        0xC2 => w = 0x00AF,
                        0xC3 => w = 0xFFE3,
                        0xC5 => w = 0x02CD,
                        0xE3 => w = 0xFF5E,
                        0xF2 => w = 0x2295,
                        0xF3 => w = 0x2299,
                        0xFE => w = 0xFF0F,
                        _ => {}
                    }
                } else if c == 0xA2 {
                    match c2 {
                        0x40 => w = 0xFF3C,
                        0x41 => w = 0x2215,
                        0x42 => w = 0xFE68,
                        0x46 => w = 0xFFE0,
                        0x47 => w = 0xFFE1,
                        0xCC => w = 0x5341,
                        0xCE => w = 0x5345,
                        _ => {}
                    }
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

fn mb_wchar_to_cp950(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        let mut s = big5_lookup(w);
        if (0xE000..=0xF848).contains(&w) {
            let mut k = 0;
            while k < cp950_pua_tbl.len() {
                if w <= cp950_pua_tbl[k][1] as u32 {
                    break;
                }
                k += 1;
            }
            let mut c1 = w as i32 - cp950_pua_tbl[k][0] as i32;
            if (cp950_pua_tbl[k][2] & 0xFF) == 0x40 {
                let c2 = (cp950_pua_tbl[k][2] >> 8) as i32;
                let mut ss = ((c1 / 157) + c2) << 8;
                c1 %= 157;
                ss |= c1 + (if c1 >= 0x3F { 0x62 } else { 0x40 });
                s = ss as u32;
            } else {
                s = (c1 + cp950_pua_tbl[k][2] as i32) as u32;
            }
        } else if w == 0xA2
            || w == 0xA3
            || w == 0x401
            || (0x414..=0x41C).contains(&w)
            || (0x423..=0x44F).contains(&w)
            || w == 0x451
            || w == 0x2022
            || w == 0x203E
            || w == 0x223C
            || (0x2460..=0x247D).contains(&w)
            || w == 0x2609
            || w == 0x2641
            || w == 0x3005
            || (0x302A..=0x30FF).contains(&w)
            || w == 0xFF64
        {
            s = 0;
        } else {
            match w {
                0xAF => s = 0xA1C2,
                0x2CD => s = 0xA1C5,
                0x2027 => s = 0xA145,
                0x2215 => s = 0xA241,
                0x2295 => s = 0xA1F2,
                0x2299 => s = 0xA1F3,
                0x2574 => s = 0xA15A,
                0xFE51 => s = 0xA14E,
                0xFE68 => s = 0xA242,
                0xFF3C => s = 0xA240,
                0xFF5E => s = 0xA1E3,
                0xFFE0 => s = 0xA246,
                0xFFE1 => s = 0xA247,
                0xFFE3 => s = 0xA1C3,
                0xFF0F => s = 0xA1FE,
                _ => {}
            }
        }

        if s == 0 {
            if w == 0 {
                buf.add(0);
            } else {
                // C uses the Big5 encoder for error markers here
                mb_illegal_output(w, mb_wchar_to_big5, buf);
            }
        } else if s <= 0x80 {
            buf.add(s as u8);
        } else {
            buf.add2(((s >> 8) & 0xFF) as u8, (s & 0xFF) as u8);
        }
    }
}

pub static ENC_BIG5: Encoding = Encoding {
    no: EncNo::Big5,
    name: "BIG-5",
    mime_name: Some("BIG5"),
    aliases: &["CN-BIG5", "BIG-FIVE", "BIGFIVE"],
    mblen_table: Some(&mblen_table_81_to_fe),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_big5_to_wchar),
    from_wchar: Some(mb_wchar_to_big5),
    check: None,
    cut: None,
};

pub static ENC_CP950: Encoding = Encoding {
    no: EncNo::Cp950,
    name: "CP950",
    mime_name: Some("BIG5"),
    aliases: &[],
    mblen_table: Some(&mblen_table_81_to_fe),
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_cp950_to_wchar),
    from_wchar: Some(mb_wchar_to_cp950),
    check: None,
    cut: None,
};

/* ===== HZ ===== */

const HZ_ASCII: u32 = 0;
const HZ_GB2312: u32 = 1;

fn mb_hz_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0;
    while p < e {
        let c = input[p];
        p += 1;
        if c == b'~' {
            if p == e {
                break;
            }
            let c2 = input[p];
            p += 1;
            if c2 == b'}' && *state == HZ_GB2312 {
                *state = HZ_ASCII;
            } else if c2 == b'{' && *state == HZ_ASCII {
                *state = HZ_GB2312;
            } else if c2 == b'~' && *state == HZ_ASCII {
                out.push('~' as u32);
            } else if c2 == b'\n' {
                // "~\n" is a line continuation
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if ((c > 0x20 && c <= 0x29) || (0x30..=0x77).contains(&c)) && p < e && *state == HZ_GB2312 {
            let c2 = input[p];
            p += 1;
            if c > 0x20 && c < 0x7F && c2 > 0x20 && c2 < 0x7F {
                let mut s = (c as u32 - 1) * 192 + c2 as u32 + 0x40;
                if s == 0x1864 {
                    s = 0x30FB;
                } else if s == 0x186A {
                    s = 0x2015;
                } else if s == 0x186C {
                    s = 0x2225;
                } else if (0x1920..=0x192A).contains(&s) || s == 0x1963 || (0x1C60..=0x1C7F).contains(&s) || (0x1DBB..=0x1DC4).contains(&s) {
                    s = 0;
                } else {
                    s = cp936_ucs_table[s as usize] as u32;
                }
                out.push(if s == 0 { MBFL_BAD_INPUT } else { s });
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c < 0x80 && *state == HZ_ASCII {
            out.push(c as u32);
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

fn mb_wchar_to_hz(input: &[u32], buf: &mut ConvBuf, end: bool) {
    for &w in input {
        let mut s: u32 = 0;
        if inr(w, ucs_a1_cp936_table_min, ucs_a1_cp936_table_max) {
            if !(w == 0xB7 || w == 0x144 || w == 0x148 || w == 0x251 || w == 0x261 || w == 0x2CA || w == 0x2CB || w == 0x2D9) {
                s = ucs_a1_cp936_table[(w as i32 - ucs_a1_cp936_table_min) as usize] as u32;
            }
        } else if inr(w, ucs_a2_cp936_table_min, ucs_a2_cp936_table_max) {
            if w == 0x2015 {
                s = 0xA1AA;
            } else if w == 0x2010
                || w == 0x2013
                || w == 0x2014
                || w == 0x2016
                || w == 0x2025
                || w == 0x2035
                || w == 0x2105
                || w == 0x2109
                || w == 0x2121
                || (0x2170..=0x2179).contains(&w)
                || (0x2196..=0x2199).contains(&w)
                || w == 0x2215
                || w == 0x221F
                || w == 0x2223
                || w == 0x2252
                || w == 0x2266
                || w == 0x2267
                || w == 0x2295
                || (0x2550..=0x2573).contains(&w)
                || w == 0x22BF
                || w == 0x2609
                || (0x2581..=0x258F).contains(&w)
                || (0x2593..=0x2595).contains(&w)
                || w == 0x25BC
                || w == 0x25BD
                || (0x25E2..=0x25E5).contains(&w)
            {
                s = 0;
            } else {
                s = ucs_a2_cp936_table[(w as i32 - ucs_a2_cp936_table_min) as usize] as u32;
            }
        } else if inr(w, ucs_a3_cp936_table_min, ucs_a3_cp936_table_max) {
            if w == 0x30FB {
                s = 0xA1A4;
            } else if w == 0x3006
                || w == 0x3007
                || w == 0x3012
                || w == 0x3231
                || w == 0x32A3
                || w >= 0x3300
                || (0x3018..=0x3040).contains(&w)
                || (0x309B..=0x309E).contains(&w)
                || (0x30FC..=0x30FE).contains(&w)
            {
                s = 0;
            } else {
                s = ucs_a3_cp936_table[(w as i32 - ucs_a3_cp936_table_min) as usize] as u32;
            }
        } else if inr(w, ucs_i_gb2312_table_min, ucs_i_gb2312_table_max) {
            s = ucs_i_gb2312_table[(w as i32 - ucs_i_gb2312_table_min) as usize] as u32;
        } else if inr(w, ucs_hff_cp936_table_min, ucs_hff_cp936_table_max) {
            if w == 0xFF04 {
                s = 0xA1E7;
            } else if w == 0xFF5E {
                s = 0xA1AB;
            } else if (0xFF01..=0xFF5D).contains(&w) {
                s = w - 0xFF01 + 0xA3A1;
            } else if w == 0xFFE0 || w == 0xFFE1 || w == 0xFFE3 || w == 0xFFE5 {
                s = ucs_hff_s_cp936_table[(w - 0xFFE0) as usize] as u32;
            }
        }

        s &= !0x8080;

        if (s == 0 && w != 0) || (s >= 0x80 && s < 0x2121) {
            mb_illegal_output(w, mb_wchar_to_hz, buf);
        } else if s < 0x80 {
            // ASCII
            if buf.state != HZ_ASCII {
                buf.add2(b'~', b'}');
                buf.state = HZ_ASCII;
            }
            if s == '~' as u32 {
                buf.add2(b'~', b'~');
            } else {
                buf.add(s as u8);
            }
        } else {
            // GB 2312-80
            if buf.state != HZ_GB2312 {
                buf.add2(b'~', b'{');
                buf.state = HZ_GB2312;
            }
            buf.add2(((s >> 8) & 0x7F) as u8, (s & 0x7F) as u8);
        }
    }

    if end && buf.state != HZ_ASCII {
        // If not in ASCII state, need to emit closing control chars
        buf.add2(b'~', b'}');
    }
}

pub static ENC_HZ: Encoding = Encoding {
    no: EncNo::Hz,
    name: "HZ",
    mime_name: Some("HZ-GB-2312"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_hz_to_wchar),
    from_wchar: Some(mb_wchar_to_hz),
    check: None,
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[
    &ENC_EUC_CN,
    &ENC_EUC_TW,
    &ENC_EUC_KR,
    &ENC_UHC,
    &ENC_GB18030,
    &ENC_CP936,
    &ENC_GB18030_2022,
    &ENC_BIG5,
    &ENC_CP950,
    &ENC_HZ,
];
