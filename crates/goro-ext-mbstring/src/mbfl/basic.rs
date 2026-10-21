//! pass, wchar, 8bit, 7bit and UTF-8 (C: mbfilter_pass.c, mbfilter_wchar.c,
//! mbfilter_8bit.c, mbfilter_7bit.c, mbfilter_utf8.c)

use super::tables::filt_utf8::mblen_table_utf8;
use super::*;

pub static ENC_PASS: Encoding = Encoding {
    no: EncNo::Pass,
    name: "pass",
    mime_name: None,
    aliases: &["none"],
    mblen_table: None,
    flag: 0,
    to_wchar: None,
    from_wchar: None,
    check: None,
    cut: None,
};

pub static ENC_WCHAR: Encoding = Encoding {
    no: EncNo::Wchar,
    name: "wchar",
    mime_name: None,
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS4,
    to_wchar: None,
    from_wchar: None,
    check: None,
    cut: None,
};

/* 8bit */

fn mb_8bit_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    out.extend(input.iter().map(|&c| c as u32));
}

fn mb_wchar_to_8bit(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w <= 0xFF {
            buf.add(w as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_8bit, buf);
        }
    }
}

pub static ENC_8BIT: Encoding = Encoding {
    no: EncNo::EightBit,
    name: "8bit",
    mime_name: Some("8bit"),
    aliases: &["binary"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_SBCS,
    to_wchar: Some(mb_8bit_to_wchar),
    from_wchar: Some(mb_wchar_to_8bit),
    check: None,
    cut: None,
};

/* 7bit */

fn mb_7bit_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    out.extend(
        input
            .iter()
            .map(|&c| if c < 0x80 { c as u32 } else { MBFL_BAD_INPUT }),
    );
}

fn mb_wchar_to_7bit(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w <= 0x7F {
            buf.add(w as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_7bit, buf);
        }
    }
}

pub static ENC_7BIT: Encoding = Encoding {
    no: EncNo::SevenBit,
    name: "7bit",
    mime_name: Some("7bit"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_SBCS,
    to_wchar: Some(mb_7bit_to_wchar),
    from_wchar: Some(mb_wchar_to_7bit),
    check: None,
    cut: None,
};

/* UTF-8 */

pub fn mb_utf8_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c < 0x80 {
            out.push(c as u32);
        } else if c < 0xC2 {
            out.push(MBFL_BAD_INPUT);
        } else if c <= 0xDF {
            if p < e {
                let c2 = input[p];
                p += 1;
                if (c2 & 0xC0) != 0x80 {
                    out.push(MBFL_BAD_INPUT);
                    p -= 1;
                } else {
                    out.push((((c & 0x1F) as u32) << 6) | (c2 & 0x3F) as u32);
                }
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if c <= 0xEF {
            if e - p >= 2 {
                let c2 = input[p];
                let c3 = input[p + 1];
                p += 2;
                if (c2 & 0xC0) != 0x80 || (c == 0xE0 && c2 < 0xA0) || (c == 0xED && c2 >= 0xA0) {
                    out.push(MBFL_BAD_INPUT);
                    p -= 2;
                } else if (c3 & 0xC0) != 0x80 {
                    out.push(MBFL_BAD_INPUT);
                    p -= 1;
                } else {
                    out.push(
                        (((c & 0xF) as u32) << 12)
                            | (((c2 & 0x3F) as u32) << 6)
                            | (c3 & 0x3F) as u32,
                    );
                }
            } else {
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
            }
        } else if c <= 0xF4 {
            if e - p >= 3 {
                let c2 = input[p];
                let c3 = input[p + 1];
                let c4 = input[p + 2];
                p += 3;
                if (c2 & 0xC0) != 0x80 || (c == 0xF0 && c2 < 0x90) || (c == 0xF4 && c2 >= 0x90) {
                    out.push(MBFL_BAD_INPUT);
                    p -= 3;
                } else if (c3 & 0xC0) != 0x80 {
                    out.push(MBFL_BAD_INPUT);
                    p -= 2;
                } else if (c4 & 0xC0) != 0x80 {
                    out.push(MBFL_BAD_INPUT);
                    p -= 1;
                } else {
                    out.push(
                        (((c & 0x7) as u32) << 18)
                            | (((c2 & 0x3F) as u32) << 12)
                            | (((c3 & 0x3F) as u32) << 6)
                            | (c4 & 0x3F) as u32,
                    );
                }
            } else {
                out.push(MBFL_BAD_INPUT);
                if p < e {
                    let c2 = input[p];
                    if (c == 0xF0 && c2 >= 0x90)
                        || (c == 0xF4 && c2 < 0x90)
                        || (c >= 0xF1 && c <= 0xF3)
                    {
                        while p < e && (input[p] & 0xC0) == 0x80 {
                            p += 1;
                        }
                    }
                }
            }
        } else {
            out.push(MBFL_BAD_INPUT);
        }
    }
}

#[inline]
pub fn push_utf8(out: &mut Vec<u8>, w: u32) {
    if w < 0x80 {
        out.push(w as u8);
    } else if w < 0x800 {
        out.push((((w >> 6) & 0x1F) | 0xC0) as u8);
        out.push(((w & 0x3F) | 0x80) as u8);
    } else if w < 0x10000 {
        out.push((((w >> 12) & 0xF) | 0xE0) as u8);
        out.push((((w >> 6) & 0x3F) | 0x80) as u8);
        out.push(((w & 0x3F) | 0x80) as u8);
    } else {
        out.push((((w >> 18) & 0x7) | 0xF0) as u8);
        out.push((((w >> 12) & 0x3F) | 0x80) as u8);
        out.push((((w >> 6) & 0x3F) | 0x80) as u8);
        out.push(((w & 0x3F) | 0x80) as u8);
    }
}

pub fn mb_wchar_to_utf8(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < 0x110000 {
            push_utf8(&mut buf.out, w);
        } else {
            mb_illegal_output(w, mb_wchar_to_utf8, buf);
        }
    }
}

fn mb_cut_utf8(s: &[u8], from: usize, len: usize) -> Vec<u8> {
    let end = s.len();
    let mut start = from;
    // Byte values less than -64 are UTF-8 continuation bytes
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

pub static ENC_UTF8: Encoding = Encoding {
    no: EncNo::Utf8,
    name: "UTF-8",
    mime_name: Some("UTF-8"),
    aliases: &["utf8"],
    mblen_table: Some(&mblen_table_utf8),
    flag: 0,
    to_wchar: Some(mb_utf8_to_wchar),
    from_wchar: Some(mb_wchar_to_utf8),
    check: None,
    cut: Some(mb_cut_utf8),
};

pub static ENCODINGS: &[&Encoding] = &[&ENC_8BIT, &ENC_7BIT, &ENC_UTF8];
