//! UCS-2, UCS-4, UTF-16 and UTF-32 (C: mbfilter_ucs2.c, mbfilter_ucs4.c,
//! mbfilter_utf16.c, mbfilter_utf32.c)

use super::*;

const DETECTED_BE: u32 = 1;
const DETECTED_LE: u32 = 2;

/* ===== UCS-2 ===== */

fn mb_ucs2_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    if *state == DETECTED_BE {
        return mb_ucs2be_to_wchar(input, out, state);
    } else if *state == DETECTED_LE {
        return mb_ucs2le_to_wchar(input, out, state);
    }
    let mut input = input;
    if input.len() >= 2 {
        let w = ((input[0] as u32) << 8) | input[1] as u32;
        if w == 0xFFFE {
            *state = DETECTED_LE;
            return mb_ucs2le_to_wchar(&input[2..], out, state);
        } else if w == 0xFEFF {
            input = &input[2..];
        }
    }
    *state = DETECTED_BE;
    mb_ucs2be_to_wchar(input, out, state)
}

fn mb_ucs2be_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let mut chunks = input.chunks_exact(2);
    for c in &mut chunks {
        out.push(((c[0] as u32) << 8) | c[1] as u32);
    }
    if !chunks.remainder().is_empty() {
        out.push(MBFL_BAD_INPUT);
    }
}

fn mb_ucs2le_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let mut chunks = input.chunks_exact(2);
    for c in &mut chunks {
        out.push(((c[1] as u32) << 8) | c[0] as u32);
    }
    if !chunks.remainder().is_empty() {
        out.push(MBFL_BAD_INPUT);
    }
}

fn mb_wchar_to_ucs2be(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < MBFL_WCSPLANE_UCS2MAX {
            buf.add2((w >> 8) as u8, w as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_ucs2be, buf);
        }
    }
}

fn mb_wchar_to_ucs2le(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < MBFL_WCSPLANE_UCS2MAX {
            buf.add2(w as u8, (w >> 8) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_ucs2le, buf);
        }
    }
}

pub static ENC_UCS2: Encoding = Encoding {
    no: EncNo::Ucs2,
    name: "UCS-2",
    mime_name: Some("UCS-2"),
    aliases: &["ISO-10646-UCS-2", "UCS2", "UNICODE"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS2,
    to_wchar: Some(mb_ucs2_to_wchar),
    from_wchar: Some(mb_wchar_to_ucs2be),
    check: None,
    cut: None,
};

pub static ENC_UCS2BE: Encoding = Encoding {
    no: EncNo::Ucs2be,
    name: "UCS-2BE",
    mime_name: Some("UCS-2BE"),
    aliases: &["byte2be"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS2,
    to_wchar: Some(mb_ucs2be_to_wchar),
    from_wchar: Some(mb_wchar_to_ucs2be),
    check: None,
    cut: None,
};

pub static ENC_UCS2LE: Encoding = Encoding {
    no: EncNo::Ucs2le,
    name: "UCS-2LE",
    mime_name: Some("UCS-2LE"),
    aliases: &["byte2le"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS2,
    to_wchar: Some(mb_ucs2le_to_wchar),
    from_wchar: Some(mb_wchar_to_ucs2le),
    check: None,
    cut: None,
};

/* ===== UCS-4 ===== */

#[inline]
fn be32(c: &[u8]) -> u32 {
    ((c[0] as u32) << 24) | ((c[1] as u32) << 16) | ((c[2] as u32) << 8) | c[3] as u32
}

#[inline]
fn le32(c: &[u8]) -> u32 {
    ((c[3] as u32) << 24) | ((c[2] as u32) << 16) | ((c[1] as u32) << 8) | c[0] as u32
}

fn mb_ucs4_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    if *state == DETECTED_BE {
        return mb_ucs4be_to_wchar(input, out, state);
    } else if *state == DETECTED_LE {
        return mb_ucs4le_to_wchar(input, out, state);
    }
    let mut input = input;
    if input.len() >= 4 {
        let w = be32(input);
        if w == 0xFFFE0000 {
            *state = DETECTED_LE;
            return mb_ucs4le_to_wchar(&input[4..], out, state);
        } else if w == 0xFEFF {
            input = &input[4..];
        }
    }
    *state = DETECTED_BE;
    mb_ucs4be_to_wchar(input, out, state)
}

fn mb_ucs4be_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let mut chunks = input.chunks_exact(4);
    for c in &mut chunks {
        out.push(be32(c));
    }
    if !chunks.remainder().is_empty() {
        out.push(MBFL_BAD_INPUT);
    }
}

fn mb_ucs4le_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let mut chunks = input.chunks_exact(4);
    for c in &mut chunks {
        out.push(le32(c));
    }
    if !chunks.remainder().is_empty() {
        out.push(MBFL_BAD_INPUT);
    }
}

fn mb_wchar_to_ucs4be(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w != MBFL_BAD_INPUT {
            buf.add4((w >> 24) as u8, (w >> 16) as u8, (w >> 8) as u8, w as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_ucs4be, buf);
        }
    }
}

fn mb_wchar_to_ucs4le(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w != MBFL_BAD_INPUT {
            buf.add4(w as u8, (w >> 8) as u8, (w >> 16) as u8, (w >> 24) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_ucs4le, buf);
        }
    }
}

pub static ENC_UCS4: Encoding = Encoding {
    no: EncNo::Ucs4,
    name: "UCS-4",
    mime_name: Some("UCS-4"),
    aliases: &["ISO-10646-UCS-4", "UCS4"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS4,
    to_wchar: Some(mb_ucs4_to_wchar),
    from_wchar: Some(mb_wchar_to_ucs4be),
    check: None,
    cut: None,
};

pub static ENC_UCS4BE: Encoding = Encoding {
    no: EncNo::Ucs4be,
    name: "UCS-4BE",
    mime_name: Some("UCS-4BE"),
    aliases: &["byte4be"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS4,
    to_wchar: Some(mb_ucs4be_to_wchar),
    from_wchar: Some(mb_wchar_to_ucs4be),
    check: None,
    cut: None,
};

pub static ENC_UCS4LE: Encoding = Encoding {
    no: EncNo::Ucs4le,
    name: "UCS-4LE",
    mime_name: Some("UCS-4LE"),
    aliases: &["byte4le"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS4,
    to_wchar: Some(mb_ucs4le_to_wchar),
    from_wchar: Some(mb_wchar_to_ucs4le),
    check: None,
    cut: None,
};

/* ===== UTF-32 ===== */

#[inline]
fn utf32_valid(w: u32) -> u32 {
    if w < MBFL_WCSPLANE_UTF32MAX && !(0xD800..=0xDFFF).contains(&w) {
        w
    } else {
        MBFL_BAD_INPUT
    }
}

fn mb_utf32_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    if *state == DETECTED_BE {
        return mb_utf32be_to_wchar(input, out, state);
    } else if *state == DETECTED_LE {
        return mb_utf32le_to_wchar(input, out, state);
    }
    let mut input = input;
    if input.len() >= 4 {
        let w = be32(input);
        if w == 0xFFFE0000 {
            *state = DETECTED_LE;
            return mb_utf32le_to_wchar(&input[4..], out, state);
        } else if w == 0xFEFF {
            input = &input[4..];
        }
    }
    *state = DETECTED_BE;
    mb_utf32be_to_wchar(input, out, state)
}

fn mb_utf32be_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let mut chunks = input.chunks_exact(4);
    for c in &mut chunks {
        out.push(utf32_valid(be32(c)));
    }
    if !chunks.remainder().is_empty() {
        out.push(MBFL_BAD_INPUT);
    }
}

fn mb_utf32le_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let mut chunks = input.chunks_exact(4);
    for c in &mut chunks {
        out.push(utf32_valid(le32(c)));
    }
    if !chunks.remainder().is_empty() {
        out.push(MBFL_BAD_INPUT);
    }
}

fn mb_wchar_to_utf32be(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < MBFL_WCSPLANE_UTF32MAX {
            buf.add4((w >> 24) as u8, (w >> 16) as u8, (w >> 8) as u8, w as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_utf32be, buf);
        }
    }
}

fn mb_wchar_to_utf32le(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < MBFL_WCSPLANE_UTF32MAX {
            buf.add4(w as u8, (w >> 8) as u8, (w >> 16) as u8, (w >> 24) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_utf32le, buf);
        }
    }
}

pub static ENC_UTF32: Encoding = Encoding {
    no: EncNo::Utf32,
    name: "UTF-32",
    mime_name: Some("UTF-32"),
    aliases: &["utf32"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS4,
    to_wchar: Some(mb_utf32_to_wchar),
    from_wchar: Some(mb_wchar_to_utf32be),
    check: None,
    cut: None,
};

pub static ENC_UTF32BE: Encoding = Encoding {
    no: EncNo::Utf32be,
    name: "UTF-32BE",
    mime_name: Some("UTF-32BE"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS4,
    to_wchar: Some(mb_utf32be_to_wchar),
    from_wchar: Some(mb_wchar_to_utf32be),
    check: None,
    cut: None,
};

pub static ENC_UTF32LE: Encoding = Encoding {
    no: EncNo::Utf32le,
    name: "UTF-32LE",
    mime_name: Some("UTF-32LE"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_WCS4,
    to_wchar: Some(mb_utf32le_to_wchar),
    from_wchar: Some(mb_wchar_to_utf32le),
    check: None,
    cut: None,
};

/* ===== UTF-16 ===== */

fn mb_utf16_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    if *state == DETECTED_BE {
        return mb_utf16be_to_wchar(input, out, state);
    } else if *state == DETECTED_LE {
        return mb_utf16le_to_wchar(input, out, state);
    }
    let mut input = input;
    if input.len() >= 2 {
        let n = ((input[0] as u32) << 8) | input[1] as u32;
        if n == 0xFFFE {
            *state = DETECTED_LE;
            return mb_utf16le_to_wchar(&input[2..], out, state);
        }
        if n == 0xFEFF {
            input = &input[2..];
        }
    }
    *state = DETECTED_BE;
    mb_utf16be_to_wchar(input, out, state)
}

#[inline]
fn utf16_decode(input: &[u8], out: &mut Vec<u32>, le: bool) {
    let rd = |i: usize| -> u32 {
        if le {
            ((input[i + 1] as u32) << 8) | input[i] as u32
        } else {
            ((input[i] as u32) << 8) | input[i + 1] as u32
        }
    };
    let e = input.len() & !1;
    let mut p = 0usize;
    while p < e {
        let n = rd(p);
        p += 2;
        if (0xD800..=0xDBFF).contains(&n) {
            if p < e {
                let n2 = rd(p);
                p += 2;
                if (0xD800..=0xDBFF).contains(&n2) {
                    out.push(MBFL_BAD_INPUT);
                    p -= 2;
                } else if (0xDC00..=0xDFFF).contains(&n2) {
                    out.push((((n & 0x3FF) << 10) | (n2 & 0x3FF)) + 0x10000);
                } else {
                    out.push(MBFL_BAD_INPUT);
                    out.push(n2);
                }
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        } else if (0xDC00..=0xDFFF).contains(&n) {
            out.push(MBFL_BAD_INPUT);
        } else {
            out.push(n);
        }
    }
    if input.len() & 1 != 0 {
        out.push(MBFL_BAD_INPUT);
    }
}

pub fn mb_utf16be_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    utf16_decode(input, out, false);
}

pub fn mb_utf16le_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    utf16_decode(input, out, true);
}

pub fn mb_wchar_to_utf16be(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < MBFL_WCSPLANE_UCS2MAX {
            buf.add2((w >> 8) as u8, w as u8);
        } else if w < MBFL_WCSPLANE_UTF32MAX {
            let n1 = (((w >> 10) - 0x40) | 0xD800) as u16;
            let n2 = ((w & 0x3FF) | 0xDC00) as u16;
            buf.add4((n1 >> 8) as u8, n1 as u8, (n2 >> 8) as u8, n2 as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_utf16be, buf);
        }
    }
}

pub fn mb_wchar_to_utf16le(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    for &w in input {
        if w < MBFL_WCSPLANE_UCS2MAX {
            buf.add2(w as u8, (w >> 8) as u8);
        } else if w < MBFL_WCSPLANE_UTF32MAX {
            let n1 = (((w >> 10) - 0x40) | 0xD800) as u16;
            let n2 = ((w & 0x3FF) | 0xDC00) as u16;
            buf.add4(n1 as u8, (n1 >> 8) as u8, n2 as u8, (n2 >> 8) as u8);
        } else {
            mb_illegal_output(w, mb_wchar_to_utf16le, buf);
        }
    }
}

fn cut_utf16_common(s: &[u8], from: usize, len: usize, le: bool) -> Vec<u8> {
    let end = s.len();
    let mut len = len;
    let avail = end as isize - from as isize;
    if (len as isize) > avail {
        len = if avail < 0 { 0 } else { avail as usize };
    }
    let from = from & !1;
    len &= !1;
    let start = from;
    if len < 2 || (end as isize - start as isize) < 2 {
        return Vec::new();
    }
    // (C checks whether the 1st codepoint is the 2nd half of a surrogate
    // pair and adjusts `from`, but `start` was already computed, so that
    // adjustment has no effect.)
    let mut e = start + len;
    if e > end {
        e = end;
    }
    let ending_cp = if le {
        ((s[e - 1] as u32) << 8) + s[e - 2] as u32
    } else {
        ((s[e - 2] as u32) << 8) + s[e - 1] as u32
    };
    if (0xD800..=0xDBFF).contains(&ending_cp) {
        e -= 2;
    }
    s[start..e].to_vec()
}

fn mb_cut_utf16be(s: &[u8], from: usize, len: usize) -> Vec<u8> {
    cut_utf16_common(s, from, len, false)
}

fn mb_cut_utf16le(s: &[u8], from: usize, len: usize) -> Vec<u8> {
    cut_utf16_common(s, from, len, true)
}

fn mb_cut_utf16(s: &[u8], from: usize, len: usize) -> Vec<u8> {
    if len < 2 || s.len() < 2 {
        return Vec::new();
    }
    let cp = ((s[0] as u32) << 8) + s[1] as u32;
    let mut from = from;
    if cp == 0xFFFE {
        if from < 2 {
            from = 2;
        }
        mb_cut_utf16le(s, from, len)
    } else {
        if cp == 0xFEFF && from < 2 {
            from = 2;
        }
        mb_cut_utf16be(s, from, len)
    }
}

pub static ENC_UTF16: Encoding = Encoding {
    no: EncNo::Utf16,
    name: "UTF-16",
    mime_name: Some("UTF-16"),
    aliases: &["utf16"],
    mblen_table: None,
    flag: 0,
    to_wchar: Some(mb_utf16_to_wchar),
    from_wchar: Some(mb_wchar_to_utf16be),
    check: None,
    cut: Some(mb_cut_utf16),
};

pub static ENC_UTF16BE: Encoding = Encoding {
    no: EncNo::Utf16be,
    name: "UTF-16BE",
    mime_name: Some("UTF-16BE"),
    aliases: &[],
    mblen_table: None,
    flag: 0,
    to_wchar: Some(mb_utf16be_to_wchar),
    from_wchar: Some(mb_wchar_to_utf16be),
    check: None,
    cut: Some(mb_cut_utf16be),
};

pub static ENC_UTF16LE: Encoding = Encoding {
    no: EncNo::Utf16le,
    name: "UTF-16LE",
    mime_name: Some("UTF-16LE"),
    aliases: &[],
    mblen_table: None,
    flag: 0,
    to_wchar: Some(mb_utf16le_to_wchar),
    from_wchar: Some(mb_wchar_to_utf16le),
    check: None,
    cut: Some(mb_cut_utf16le),
};

pub static ENCODINGS: &[&Encoding] = &[
    &ENC_UCS4,
    &ENC_UCS4BE,
    &ENC_UCS4LE,
    &ENC_UCS2,
    &ENC_UCS2BE,
    &ENC_UCS2LE,
    &ENC_UTF32,
    &ENC_UTF32BE,
    &ENC_UTF32LE,
    &ENC_UTF16,
    &ENC_UTF16BE,
    &ENC_UTF16LE,
];
