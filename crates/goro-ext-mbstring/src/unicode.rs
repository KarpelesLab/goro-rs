//! Port of php-src ext/mbstring/php_unicode.c (case mapping and Unicode
//! property lookup), using the generated tables from unicode_data.h.

use crate::mbfl::tables::unicode_data::*;
use crate::mbfl::{ConvBuf, EncNo, Encoding};

pub const UC_MN: usize = 0;
pub const UC_MC: usize = 1;
pub const UC_ME: usize = 2;
pub const UC_ND: usize = 3;
pub const UC_NL: usize = 4;
pub const UC_NO: usize = 5;
pub const UC_ZS: usize = 6;
pub const UC_LU: usize = 12;
pub const UC_LL: usize = 13;
pub const UC_LT: usize = 14;
pub const UC_LM: usize = 15;
pub const UC_LO: usize = 16;
pub const UC_C: usize = 33;
pub const UC_P: usize = 34;
pub const UC_CASED: usize = 35;
pub const UC_CASE_IGNORABLE: usize = 36;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseMode {
    Upper = 0,
    Lower,
    Title,
    Fold,
    UpperSimple,
    LowerSimple,
    TitleSimple,
    FoldSimple,
}

impl CaseMode {
    pub fn from_long(n: i64) -> Option<CaseMode> {
        Some(match n {
            0 => CaseMode::Upper,
            1 => CaseMode::Lower,
            2 => CaseMode::Title,
            3 => CaseMode::Fold,
            4 => CaseMode::UpperSimple,
            5 => CaseMode::LowerSimple,
            6 => CaseMode::TitleSimple,
            7 => CaseMode::FoldSimple,
            _ => return None,
        })
    }
}

fn prop_lookup(code: u32, n: usize) -> bool {
    let mut l = _ucprop_offsets[n] as i64;
    let mut r = _ucprop_offsets[n + 1] as i64 - 1;
    while l <= r {
        let mut m = (l + r) >> 1;
        m -= m & 1;
        if code > _ucprop_ranges[(m + 1) as usize] {
            l = m + 2;
        } else if code < _ucprop_ranges[m as usize] {
            r = m - 2;
        } else {
            return true;
        }
    }
    false
}

#[inline]
pub fn is_prop1(code: u32, prop: usize) -> bool {
    prop_lookup(code, prop)
}

pub fn is_prop(code: u32, props: &[usize]) -> bool {
    props.iter().any(|&p| prop_lookup(code, p))
}

#[inline]
pub fn is_cased(c: u32) -> bool {
    prop_lookup(c, UC_CASED)
}

#[inline]
pub fn is_case_ignorable(c: u32) -> bool {
    prop_lookup(c, UC_CASE_IGNORABLE)
}

#[inline]
fn mph_hash(d: u32, x: u32) -> u32 {
    let x = x ^ d;
    ((x >> 16) ^ x).wrapping_mul(0x45d9f3b)
}

const CODE_NOT_FOUND: u32 = u32::MAX;

#[inline]
fn mph_lookup(code: u32, g_table: &[i16], table: &[u32]) -> u32 {
    let g_size = g_table.len() as u32;
    let table_size = (table.len() / 2) as u32;
    let g = g_table[(mph_hash(0, code) % g_size) as usize];
    let idx = if g <= 0 { (-(g as i32)) as u32 } else { mph_hash(g as u32, code) % table_size };
    if table[(2 * idx) as usize] == code {
        return table[(2 * idx + 1) as usize];
    }
    CODE_NOT_FOUND
}

fn is_8859_9(enc: &Encoding) -> bool {
    enc.no == EncNo::Iso8859_9
}

pub fn toupper_raw(code: u32, enc: &Encoding) -> u32 {
    if code < 0xB5 {
        if (0x61..=0x7A).contains(&code) {
            if is_8859_9(enc) && code == 0x69 {
                return 0x130;
            }
            return code - 0x20;
        }
        code
    } else {
        let n = mph_lookup(code, &_uccase_upper_g, &_uccase_upper_table);
        if n != CODE_NOT_FOUND { n } else { code }
    }
}

pub fn tolower_raw(code: u32, enc: &Encoding) -> u32 {
    if code < 0xC0 {
        if (0x41..=0x5A).contains(&code) {
            if is_8859_9(enc) && code == 0x49 {
                return 0x131;
            }
            return code + 0x20;
        }
        code
    } else {
        let n = mph_lookup(code, &_uccase_lower_g, &_uccase_lower_table);
        if n != CODE_NOT_FOUND {
            if is_8859_9(enc) && code == 0x130 {
                return 0x69;
            }
            return n;
        }
        code
    }
}

pub fn totitle_raw(code: u32, enc: &Encoding) -> u32 {
    let n = mph_lookup(code, &_uccase_title_g, &_uccase_title_table);
    if n != CODE_NOT_FOUND {
        return n;
    }
    toupper_raw(code, enc)
}

pub fn tofold_raw(code: u32, enc: &Encoding) -> u32 {
    if code < 0x80 {
        if (0x41..=0x5A).contains(&code) {
            if is_8859_9(enc) && code == 0x49 {
                return 0x131;
            }
            return code + 0x20;
        }
        code
    } else {
        let n = mph_lookup(code, &_uccase_fold_g, &_uccase_fold_table);
        if n != CODE_NOT_FOUND {
            if is_8859_9(enc) && code == 0x130 {
                return 0x69;
            }
            return n;
        }
        code
    }
}

#[inline]
fn simple(code: u32) -> u32 {
    if code > 0xffffff { _uccase_extra_table[(code & 0xffffff) as usize] } else { code }
}

pub fn tolower_simple(code: u32, enc: &Encoding) -> u32 {
    simple(tolower_raw(code, enc))
}
pub fn toupper_simple(code: u32, enc: &Encoding) -> u32 {
    simple(toupper_raw(code, enc))
}
pub fn totitle_simple(code: u32, enc: &Encoding) -> u32 {
    simple(totitle_raw(code, enc))
}
pub fn tofold_simple(code: u32, enc: &Encoding) -> u32 {
    simple(tofold_raw(code, enc))
}

#[inline]
fn emit_special(w: u32, out: &mut [u32; 192], p: &mut usize) {
    let len = w >> 24;
    let base = (w & 0xFFFFFF) as usize;
    for k in 0..len as usize {
        out[*p] = _uccase_extra_table[base + 1 + k];
        *p += 1;
    }
}

fn scan_ahead_for_cased_letter(rest: &[u32]) -> bool {
    for &w in rest {
        if is_cased(w) {
            return true;
        }
        if !is_case_ignorable(w) {
            return false;
        }
    }
    false
}

fn scan_back_for_cased_letter(buf: &[u32; 192], begin: usize, end: Option<usize>) -> bool {
    if let Some(mut end) = end {
        while end > begin {
            end -= 1;
            let w = buf[end];
            if is_cased(w) {
                return true;
            }
            if !is_case_ignorable(w) {
                return false;
            }
        }
    }
    false
}

/// C: `php_unicode_convert_case`
pub fn convert_case(mode: CaseMode, src: &[u8], src_enc: &Encoding, dst_enc: &Encoding, illegal_mode: u8, illegal_substchar: u32) -> Vec<u8> {
    let all = src_enc.decode(src);
    let mut buf = ConvBuf::new(src.len() + 1, illegal_substchar, illegal_mode);
    let from_wchar = match dst_enc.from_wchar {
        Some(f) => f,
        None => return src.to_vec(),
    };
    let mut converted = [0u32; 192];
    let mut converted_end: Option<usize> = None;
    let mut title_mode = false;
    let total = all.len();
    let mut pos = 0usize;
    if total == 0 {
        return Vec::new();
    }
    // Emulate C's processing in chunks of 64 codepoints (the special
    // casing rule for final sigma only looks back within that window).
    while pos < total {
        let chunk_end = (pos + 64).min(total);
        let wchar_buf = &all[pos..chunk_end];
        let out_len = wchar_buf.len();
        let mut p = 0usize;
        match mode {
            CaseMode::UpperSimple => {
                for &w in wchar_buf {
                    converted[p] = if w > 0xFFFFFF { w } else { toupper_simple(w, src_enc) };
                    p += 1;
                }
            }
            CaseMode::LowerSimple => {
                for &w in wchar_buf {
                    converted[p] = if w > 0xFFFFFF { w } else { tolower_simple(w, src_enc) };
                    p += 1;
                }
            }
            CaseMode::FoldSimple => {
                for &w in wchar_buf {
                    converted[p] = if w > 0xFFFFFF { w } else { tofold_simple(w, src_enc) };
                    p += 1;
                }
            }
            CaseMode::TitleSimple => {
                for &w in wchar_buf {
                    if w > 0xFFFFFF {
                        converted[p] = w;
                        p += 1;
                        continue;
                    }
                    converted[p] = if title_mode { tolower_simple(w, src_enc) } else { totitle_simple(w, src_enc) };
                    p += 1;
                    if !is_case_ignorable(w) {
                        title_mode = is_cased(w);
                    }
                }
            }
            CaseMode::Upper => {
                for &w in wchar_buf {
                    if w > 0xFFFFFF {
                        converted[p] = w;
                        p += 1;
                        continue;
                    }
                    let w = toupper_raw(w, src_enc);
                    if w > 0xFFFFFF {
                        emit_special(w, &mut converted, &mut p);
                    } else {
                        converted[p] = w;
                        p += 1;
                    }
                }
            }
            CaseMode::Fold => {
                for &w in wchar_buf {
                    if w > 0xFFFFFF {
                        converted[p] = w;
                        p += 1;
                        continue;
                    }
                    let w = tofold_raw(w, src_enc);
                    if w > 0xFFFFFF {
                        emit_special(w, &mut converted, &mut p);
                    } else {
                        converted[p] = w;
                        p += 1;
                    }
                }
            }
            CaseMode::Lower | CaseMode::Title => {
                for i in 0..out_len {
                    let w = wchar_buf[i];
                    if w > 0xFFFFFF {
                        converted[p] = w;
                        p += 1;
                        continue;
                    }
                    let lowering = mode == CaseMode::Lower || title_mode;
                    let mut final_sigma = false;
                    if lowering && w == 0x3A3 {
                        let mut j = i as isize - 1;
                        while j >= 0 && is_case_ignorable(wchar_buf[j as usize]) {
                            j -= 1;
                        }
                        let before = if j >= 0 { is_cased(wchar_buf[j as usize]) } else { scan_back_for_cased_letter(&converted, p, converted_end) };
                        if before {
                            let mut j = i + 1;
                            while j < out_len && is_case_ignorable(wchar_buf[j]) {
                                j += 1;
                            }
                            let after_cased = if j >= out_len { scan_ahead_for_cased_letter(&all[chunk_end..]) } else { is_cased(wchar_buf[j]) };
                            if !after_cased {
                                final_sigma = true;
                            }
                        }
                    }
                    if final_sigma {
                        converted[p] = 0x3C2;
                        p += 1;
                    } else {
                        let w2 = if lowering { tolower_raw(w, src_enc) } else { totitle_raw(w, src_enc) };
                        if w2 > 0xFFFFFF {
                            emit_special(w2, &mut converted, &mut p);
                        } else {
                            converted[p] = w2;
                            p += 1;
                        }
                    }
                    if mode == CaseMode::Title && !is_case_ignorable(w) {
                        title_mode = is_cased(w);
                    }
                }
            }
        }
        converted_end = Some(p);
        pos = chunk_end;
        from_wchar(&converted[..p], &mut buf, pos >= total);
    }
    buf.out
}
