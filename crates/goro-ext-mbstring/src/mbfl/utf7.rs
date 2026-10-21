//! UTF-7 and UTF7-IMAP (C: mbfilter_utf7.c, mbfilter_utf7imap.c, utf7_helper.h)

use super::*;

/* Ways which a Base64-encoded section can end: */
const DASH: u8 = 0xFC;
const DIRECT: u8 = 0xFD;
const ASCII: u8 = 0xFE;
const ILLEGAL: u8 = 0xFF;

#[inline]
fn is_base64_end_valid(n: u8, gap: bool, is_surrogate: bool) -> bool {
    !(gap || is_surrogate || n == ASCII || n == ILLEGAL)
}

#[inline]
fn has_surrogate(cp: u16, is_surrogate: bool) -> bool {
    !is_surrogate && (0xD800..=0xDBFF).contains(&cp)
}

#[inline]
fn is_base64_end(c: u8) -> bool {
    c >= DASH
}

static BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static BASE64_TABLE_IMAP: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

/* ===== UTF-7 ===== */

fn is_optional_direct(c: u8) -> bool {
    matches!(
        c,
        b'!' | b'"' | b'#' | b'$' | b'%' | b'&' | b'*' | b';' | b'<' | b'=' | b'>' | b'@' | b'[' | b']' | b'^' | b'_' | b'`' | b'{' | b'|' | b'}'
    )
}

fn can_end_base64(c: u32) -> bool {
    c == b' ' as u32
        || c == b'\t' as u32
        || c == b'\r' as u32
        || c == b'\n' as u32
        || c == b'\'' as u32
        || c == b'(' as u32
        || c == b')' as u32
        || c == b',' as u32
        || c == b'.' as u32
        || c == b':' as u32
        || c == b'?' as u32
}

fn decode_base64_utf7(c: u8) -> u8 {
    match c {
        b'A'..=b'Z' => c - 65,
        b'a'..=b'z' => c - 71,
        b'0'..=b'9' => c + 4,
        b'+' => 62,
        b'/' => 63,
        b'-' => DASH,
        _ => {
            if can_end_base64(c as u32) || is_optional_direct(c) || c == 0 {
                DIRECT
            } else if c <= 0x7F {
                ASCII
            } else {
                ILLEGAL
            }
        }
    }
}

fn handle_utf16_cp_utf7(cp: u16, out: &mut Vec<u32>, surrogate1: &mut u16) {
    loop {
        if *surrogate1 != 0 {
            if (0xDC00..=0xDFFF).contains(&cp) {
                out.push((((*surrogate1 as u32) & 0x3FF) << 10) + (cp as u32 & 0x3FF) + 0x10000);
                *surrogate1 = 0;
            } else {
                out.push(MBFL_BAD_INPUT);
                *surrogate1 = 0;
                continue;
            }
        } else if (0xD800..=0xDBFF).contains(&cp) {
            *surrogate1 = cp;
        } else if (0xDC00..=0xDFFF).contains(&cp) {
            out.push(MBFL_BAD_INPUT);
        } else {
            out.push(cp as u32);
        }
        return;
    }
}

fn handle_base64_end_utf7(n: u8, p: &mut usize, out: &mut Vec<u32>, base64: &mut bool, abrupt: bool, surrogate1: &mut u16) {
    if abrupt || *surrogate1 != 0 {
        out.push(MBFL_BAD_INPUT);
        *surrogate1 = 0;
    }
    if n == ILLEGAL {
        out.push(MBFL_BAD_INPUT);
    } else if n == DIRECT || n == ASCII {
        *p -= 1; /* Unconsume byte */
    }
    *base64 = false;
}

fn mb_utf7_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    let mut base64 = *state & 1 != 0;
    let mut surrogate1: u16 = (*state >> 1) as u16;

    macro_rules! next {
        () => {{
            let c = decode_base64_utf7(input[p]);
            p += 1;
            c
        }};
    }

    while p < e {
        if base64 {
            let n1 = next!();
            if is_base64_end(n1) {
                handle_base64_end_utf7(n1, &mut p, out, &mut base64, false, &mut surrogate1);
                continue;
            } else if p == e {
                handle_base64_end_utf7(n1, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n2 = next!();
            if is_base64_end(n2) || p == e {
                handle_base64_end_utf7(n2, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n3 = next!();
            if is_base64_end(n3) {
                handle_base64_end_utf7(n3, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let cp = ((n1 as u32) << 10) | ((n2 as u32) << 4) | ((n3 as u32 & 0x3C) >> 2);
            handle_utf16_cp_utf7(cp as u16, out, &mut surrogate1);
            if p == e {
                if (n3 & 0x3) != 0 || surrogate1 != 0 {
                    out.push(MBFL_BAD_INPUT);
                    surrogate1 = 0;
                }
                break;
            }

            let n4 = next!();
            if is_base64_end(n4) {
                handle_base64_end_utf7(n4, &mut p, out, &mut base64, (n3 & 0x3) != 0, &mut surrogate1);
                continue;
            } else if p == e {
                handle_base64_end_utf7(n4, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n5 = next!();
            if is_base64_end(n5) || p == e {
                handle_base64_end_utf7(n5, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n6 = next!();
            if is_base64_end(n6) {
                handle_base64_end_utf7(n6, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let cp = ((n3 as u32) << 14) | ((n4 as u32) << 8) | ((n5 as u32) << 2) | ((n6 as u32 & 0x30) >> 4);
            handle_utf16_cp_utf7(cp as u16, out, &mut surrogate1);
            if p == e {
                if (n6 & 0xF) != 0 || surrogate1 != 0 {
                    out.push(MBFL_BAD_INPUT);
                    surrogate1 = 0;
                }
                break;
            }

            let n7 = next!();
            if is_base64_end(n7) {
                handle_base64_end_utf7(n7, &mut p, out, &mut base64, (n6 & 0xF) != 0, &mut surrogate1);
                continue;
            } else if p == e {
                handle_base64_end_utf7(n7, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n8 = next!();
            if is_base64_end(n8) {
                handle_base64_end_utf7(n8, &mut p, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let cp = ((n6 as u32) << 12) | ((n7 as u32) << 6) | n8 as u32;
            handle_utf16_cp_utf7(cp as u16, out, &mut surrogate1);
        } else {
            let c = input[p];
            p += 1;
            if c == b'+' {
                if p < e {
                    if input[p] == b'-' {
                        out.push('+' as u32);
                        p += 1;
                    } else {
                        base64 = true;
                    }
                }
            } else if c <= 0x7F {
                out.push(c as u32);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
    }

    if p == e && surrogate1 != 0 {
        out.push(MBFL_BAD_INPUT);
    }

    *state = ((surrogate1 as u32) << 1) | base64 as u32;
}

fn should_direct_encode(c: u32) -> bool {
    (c >= 'A' as u32 && c <= 'Z' as u32)
        || (c >= 'a' as u32 && c <= 'z' as u32)
        || (c >= '0' as u32 && c <= '9' as u32)
        || c == 0
        || c == '/' as u32
        || c == '-' as u32
        || can_end_base64(c)
}

/// Shared Base64-section encoder step; returns the new (nbits, cache)
#[inline]
fn encode_b64_cp(mut w: u32, nbits: &mut u8, cache: &mut u8, buf: &mut ConvBuf, table: &[u8; 64]) {
    let bits: u64;
    if w >= MBFL_WCSPLANE_SUPMIN {
        w -= 0x10000;
        bits = ((*cache as u64) << 32) | 0xD800DC00u64 | (((w & 0xFFC00) as u64) << 6) | (w & 0x3FF) as u64;
        *nbits += 32;
    } else {
        bits = ((*cache as u64) << 16) | w as u64;
        *nbits += 16;
    }
    while *nbits >= 6 {
        buf.add(table[((bits >> (*nbits - 6)) & 0x3F) as usize]);
        *nbits -= 6;
    }
    *cache = bits as u8;
}

fn mb_wchar_to_utf7(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let mut base64 = buf.state & 1 != 0;
    let mut nbits = ((buf.state >> 1) & 0x7) as u8;
    let mut cache = (buf.state >> 4) as u8;

    let mut i = 0usize;
    while i < input.len() {
        let w = input[i];
        i += 1;
        if base64 {
            if should_direct_encode(w) {
                base64 = false;
                i -= 1; /* Unconsume codepoint */
                if nbits != 0 {
                    buf.add(BASE64_TABLE[(((cache as u32) << (6 - nbits)) & 0x3F) as usize]);
                }
                nbits = 0;
                cache = 0;
                if !can_end_base64(w) {
                    buf.add(b'-');
                }
            } else if w >= MBFL_WCSPLANE_UTF32MAX {
                buf.state = ((cache as u32) << 4) | ((nbits as u32) << 1) | base64 as u32;
                mb_illegal_output(w, mb_wchar_to_utf7, buf);
                base64 = buf.state & 1 != 0;
                nbits = ((buf.state >> 1) & 0x7) as u8;
                cache = (buf.state >> 4) as u8;
            } else {
                encode_b64_cp(w, &mut nbits, &mut cache, buf, BASE64_TABLE);
            }
        } else if should_direct_encode(w) {
            buf.add(w as u8);
        } else if w >= MBFL_WCSPLANE_UTF32MAX {
            buf.state = 0;
            mb_illegal_output(w, mb_wchar_to_utf7, buf);
            base64 = buf.state & 1 != 0;
            nbits = ((buf.state >> 1) & 0x7) as u8;
            cache = (buf.state >> 4) as u8;
        } else {
            buf.add(b'+');
            base64 = true;
            i -= 1; /* Unconsume codepoint */
        }
    }

    if end {
        if nbits != 0 {
            buf.add(BASE64_TABLE[(((cache as u32) << (6 - nbits)) & 0x3F) as usize]);
        }
        if base64 {
            buf.add(b'-');
        }
    } else {
        buf.state = ((cache as u32) << 4) | ((nbits as u32) << 1) | base64 as u32;
    }
}

fn is_utf16_cp_valid_utf7(cp: u16, is_surrogate: bool) -> bool {
    if is_surrogate {
        (0xDC00..=0xDFFF).contains(&cp)
    } else {
        !(0xDC00..=0xDFFF).contains(&cp)
    }
}

fn can_encode_directly(c: u8) -> bool {
    should_direct_encode(c as u32) || is_optional_direct(c) || c == 0
}

fn mb_check_utf7(input: &[u8]) -> bool {
    let e = input.len();
    let mut p = 0usize;
    let mut base64 = false;
    let mut is_surrogate = false;

    macro_rules! next {
        () => {{
            let c = decode_base64_utf7(input[p]);
            p += 1;
            c
        }};
    }

    while p < e {
        if base64 {
            let n1 = next!();
            if is_base64_end(n1) {
                if !is_base64_end_valid(n1, false, is_surrogate) {
                    return false;
                }
                base64 = false;
                continue;
            } else if p == e {
                return false;
            }
            let n2 = next!();
            if is_base64_end(n2) || p == e {
                return false;
            }
            let n3 = next!();
            if is_base64_end(n3) {
                return false;
            }
            let cp1 = (((n1 as u32) << 10) | ((n2 as u32) << 4) | ((n3 as u32 & 0x3C) >> 2)) as u16;
            if !is_utf16_cp_valid_utf7(cp1, is_surrogate) {
                return false;
            }
            is_surrogate = has_surrogate(cp1, is_surrogate);
            if p == e {
                return !((n3 & 0x3) != 0 || is_surrogate);
            }

            let n4 = next!();
            if is_base64_end(n4) {
                if !is_base64_end_valid(n4, (n3 & 0x3) != 0, is_surrogate) {
                    return false;
                }
                base64 = false;
                continue;
            } else if p == e {
                return false;
            }
            let n5 = next!();
            if is_base64_end(n5) || p == e {
                return false;
            }
            let n6 = next!();
            if is_base64_end(n6) {
                return false;
            }
            let cp2 = (((n3 as u32) << 14) | ((n4 as u32) << 8) | ((n5 as u32) << 2) | ((n6 as u32 & 0x30) >> 4)) as u16;
            if !is_utf16_cp_valid_utf7(cp2, is_surrogate) {
                return false;
            }
            is_surrogate = has_surrogate(cp2, is_surrogate);
            if p == e {
                return !((n6 & 0xF) != 0 || is_surrogate);
            }

            let n7 = next!();
            if is_base64_end(n7) {
                if !is_base64_end_valid(n7, (n6 & 0xF) != 0, is_surrogate) {
                    return false;
                }
                base64 = false;
                continue;
            } else if p == e {
                return false;
            }
            let n8 = next!();
            if is_base64_end(n8) {
                return false;
            }
            let cp3 = (((n6 as u32) << 12) | ((n7 as u32) << 6) | n8 as u32) as u16;
            if !is_utf16_cp_valid_utf7(cp3, is_surrogate) {
                return false;
            }
            is_surrogate = has_surrogate(cp3, is_surrogate);
        } else {
            let c = input[p];
            p += 1;
            if c == b'+' {
                if p == e {
                    return !is_surrogate;
                }
                let n = decode_base64_utf7(input[p]);
                if n == DASH {
                    p += 1;
                } else if n > DASH {
                    return false;
                } else {
                    base64 = true;
                }
            } else if can_encode_directly(c) {
                continue;
            } else {
                return false;
            }
        }
    }
    !is_surrogate
}

pub static ENC_UTF7: Encoding = Encoding {
    no: EncNo::Utf7,
    name: "UTF-7",
    mime_name: Some("UTF-7"),
    aliases: &["utf7"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_utf7_to_wchar),
    from_wchar: Some(mb_wchar_to_utf7),
    check: Some(mb_check_utf7),
    cut: None,
};

/* ===== UTF7-IMAP ===== */

fn decode_base64_imap(c: u8) -> u8 {
    match c {
        b'A'..=b'Z' => c - 65,
        b'a'..=b'z' => c - 71,
        b'0'..=b'9' => c + 4,
        b'+' => 62,
        b',' => 63,
        b'-' => DASH,
        _ => ILLEGAL,
    }
}

fn handle_utf16_cp_imap(cp: u16, out: &mut Vec<u32>, surrogate1: &mut u16) {
    loop {
        if *surrogate1 != 0 {
            if (0xDC00..=0xDFFF).contains(&cp) {
                out.push((((*surrogate1 as u32) & 0x3FF) << 10) + (cp as u32 & 0x3FF) + 0x10000);
                *surrogate1 = 0;
            } else {
                out.push(MBFL_BAD_INPUT);
                *surrogate1 = 0;
                continue;
            }
        } else if (0xD800..=0xDBFF).contains(&cp) {
            *surrogate1 = cp;
        } else if (0xDC00..=0xDFFF).contains(&cp) {
            out.push(MBFL_BAD_INPUT);
        } else if (0x20..=0x7E).contains(&cp) && cp != b'&' as u16 {
            out.push(MBFL_BAD_INPUT);
        } else {
            out.push(cp as u32);
        }
        return;
    }
}

fn handle_base64_end_imap(n: u8, out: &mut Vec<u32>, base64: &mut bool, abrupt: bool, surrogate1: &mut u16) {
    if abrupt || n == ILLEGAL || *surrogate1 != 0 {
        out.push(MBFL_BAD_INPUT);
        *surrogate1 = 0;
    }
    *base64 = false;
}

fn mb_utf7imap_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    let mut base64 = *state & 1 != 0;
    let mut surrogate1: u16 = (*state >> 1) as u16;

    macro_rules! next {
        () => {{
            let c = decode_base64_imap(input[p]);
            p += 1;
            c
        }};
    }

    while p < e {
        if base64 {
            let n1 = next!();
            if is_base64_end(n1) {
                handle_base64_end_imap(n1, out, &mut base64, false, &mut surrogate1);
                continue;
            } else if p == e {
                handle_base64_end_imap(n1, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n2 = next!();
            if is_base64_end(n2) || p == e {
                handle_base64_end_imap(n2, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n3 = next!();
            if is_base64_end(n3) {
                handle_base64_end_imap(n3, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let cp = ((n1 as u32) << 10) | ((n2 as u32) << 4) | ((n3 as u32 & 0x3C) >> 2);
            handle_utf16_cp_imap(cp as u16, out, &mut surrogate1);
            if p == e {
                if (n3 & 0x3) != 0 || surrogate1 != 0 {
                    out.push(MBFL_BAD_INPUT);
                }
                break;
            }

            let n4 = next!();
            if is_base64_end(n4) {
                handle_base64_end_imap(n4, out, &mut base64, (n3 & 0x3) != 0, &mut surrogate1);
                continue;
            } else if p == e {
                handle_base64_end_imap(n4, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n5 = next!();
            if is_base64_end(n5) || p == e {
                handle_base64_end_imap(n5, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n6 = next!();
            if is_base64_end(n6) {
                handle_base64_end_imap(n6, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let cp = ((n3 as u32) << 14) | ((n4 as u32) << 8) | ((n5 as u32) << 2) | ((n6 as u32 & 0x30) >> 4);
            handle_utf16_cp_imap(cp as u16, out, &mut surrogate1);
            if p == e {
                if (n6 & 0xF) != 0 || surrogate1 != 0 {
                    out.push(MBFL_BAD_INPUT);
                }
                break;
            }

            let n7 = next!();
            if is_base64_end(n7) {
                handle_base64_end_imap(n7, out, &mut base64, (n6 & 0xF) != 0, &mut surrogate1);
                continue;
            } else if p == e {
                handle_base64_end_imap(n7, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let n8 = next!();
            if is_base64_end(n8) {
                handle_base64_end_imap(n8, out, &mut base64, true, &mut surrogate1);
                continue;
            }
            let cp = ((n6 as u32) << 12) | ((n7 as u32) << 6) | n8 as u32;
            handle_utf16_cp_imap(cp as u16, out, &mut surrogate1);
        } else {
            let c = input[p];
            p += 1;
            if c == b'&' {
                if p < e && input[p] == b'-' {
                    out.push('&' as u32);
                    p += 1;
                } else {
                    base64 = true;
                }
            } else if (0x20..=0x7E).contains(&c) {
                out.push(c as u32);
            } else {
                out.push(MBFL_BAD_INPUT);
            }
        }
    }

    if p == e && base64 {
        /* UTF7-IMAP doesn't allow strings to end in Base64 mode */
        out.push(MBFL_BAD_INPUT);
    }

    *state = ((surrogate1 as u32) << 1) | base64 as u32;
}

fn mb_wchar_to_utf7imap(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let mut base64 = buf.state & 1 != 0;
    let mut nbits = ((buf.state >> 1) & 0x7) as u8;
    let mut cache = (buf.state >> 4) as u8;

    let mut i = 0usize;
    while i < input.len() {
        let w = input[i];
        i += 1;
        if base64 {
            if (0x20..=0x7E).contains(&w) {
                base64 = false;
                i -= 1;
                if nbits != 0 {
                    buf.add(BASE64_TABLE_IMAP[(((cache as u32) << (6 - nbits)) & 0x3F) as usize]);
                }
                nbits = 0;
                cache = 0;
                buf.add(b'-');
            } else if w >= MBFL_WCSPLANE_UTF32MAX {
                buf.state = ((cache as u32) << 4) | ((nbits as u32) << 1) | base64 as u32;
                mb_illegal_output(w, mb_wchar_to_utf7imap, buf);
                base64 = buf.state & 1 != 0;
                nbits = ((buf.state >> 1) & 0x7) as u8;
                cache = (buf.state >> 4) as u8;
            } else {
                encode_b64_cp(w, &mut nbits, &mut cache, buf, BASE64_TABLE_IMAP);
            }
        } else if w == '&' as u32 {
            buf.add2(b'&', b'-');
        } else if (0x20..=0x7E).contains(&w) {
            buf.add(w as u8);
        } else if w >= MBFL_WCSPLANE_UTF32MAX {
            buf.state = 0;
            mb_illegal_output(w, mb_wchar_to_utf7imap, buf);
            base64 = buf.state & 1 != 0;
            nbits = ((buf.state >> 1) & 0x7) as u8;
            cache = (buf.state >> 4) as u8;
        } else {
            buf.add(b'&');
            base64 = true;
            i -= 1;
        }
    }

    if end {
        if nbits != 0 {
            buf.add(BASE64_TABLE_IMAP[(((cache as u32) << (6 - nbits)) & 0x3F) as usize]);
        }
        if base64 {
            buf.add(b'-');
        }
    } else {
        buf.state = ((cache as u32) << 4) | ((nbits as u32) << 1) | base64 as u32;
    }
}

fn is_utf16_cp_valid_imap(cp: u16, is_surrogate: bool) -> bool {
    if is_surrogate {
        (0xDC00..=0xDFFF).contains(&cp)
    } else if (0xDC00..=0xDFFF).contains(&cp) {
        false
    } else {
        !((0x20..=0x7E).contains(&cp) && cp != b'&' as u16)
    }
}

fn mb_check_utf7imap(input: &[u8]) -> bool {
    let e = input.len();
    let mut p = 0usize;
    let mut base64 = false;
    let mut is_surrogate = false;

    macro_rules! next {
        () => {{
            let c = decode_base64_imap(input[p]);
            p += 1;
            c
        }};
    }

    while p < e {
        if base64 {
            let n1 = next!();
            if is_base64_end(n1) {
                if !is_base64_end_valid(n1, false, is_surrogate) {
                    return false;
                }
                base64 = false;
                continue;
            } else if p == e {
                return false;
            }
            let n2 = next!();
            if is_base64_end(n2) || p == e {
                return false;
            }
            let n3 = next!();
            if is_base64_end(n3) {
                return false;
            }
            let cp1 = (((n1 as u32) << 10) | ((n2 as u32) << 4) | ((n3 as u32 & 0x3C) >> 2)) as u16;
            if !is_utf16_cp_valid_imap(cp1, is_surrogate) {
                return false;
            }
            is_surrogate = has_surrogate(cp1, is_surrogate);
            if p == e {
                return false;
            }

            let n4 = next!();
            if is_base64_end(n4) {
                if !is_base64_end_valid(n4, (n3 & 0x3) != 0, is_surrogate) {
                    return false;
                }
                base64 = false;
                continue;
            } else if p == e {
                return false;
            }
            let n5 = next!();
            if is_base64_end(n5) || p == e {
                return false;
            }
            let n6 = next!();
            if is_base64_end(n6) {
                return false;
            }
            let cp2 = (((n3 as u32) << 14) | ((n4 as u32) << 8) | ((n5 as u32) << 2) | ((n6 as u32 & 0x30) >> 4)) as u16;
            if !is_utf16_cp_valid_imap(cp2, is_surrogate) {
                return false;
            }
            is_surrogate = has_surrogate(cp2, is_surrogate);
            if p == e {
                return false;
            }

            let n7 = next!();
            if is_base64_end(n7) {
                if !is_base64_end_valid(n7, (n6 & 0xF) != 0, is_surrogate) {
                    return false;
                }
                base64 = false;
                continue;
            } else if p == e {
                return false;
            }
            let n8 = next!();
            if is_base64_end(n8) {
                return false;
            }
            let cp3 = (((n6 as u32) << 12) | ((n7 as u32) << 6) | n8 as u32) as u16;
            if !is_utf16_cp_valid_imap(cp3, is_surrogate) {
                return false;
            }
            is_surrogate = has_surrogate(cp3, is_surrogate);
        } else {
            let c = input[p];
            p += 1;
            if c == b'&' {
                if p == e {
                    return false;
                }
                let n = decode_base64_imap(input[p]);
                if n == DASH {
                    p += 1;
                } else if n == ILLEGAL {
                    return false;
                } else {
                    base64 = true;
                }
            } else if (0x20..=0x7E).contains(&c) {
                continue;
            } else {
                return false;
            }
        }
    }
    !base64
}

pub static ENC_UTF7IMAP: Encoding = Encoding {
    no: EncNo::Utf7imap,
    name: "UTF7-IMAP",
    mime_name: None,
    aliases: &["mUTF-7"],
    mblen_table: None,
    flag: 0,
    to_wchar: Some(mb_utf7imap_to_wchar),
    from_wchar: Some(mb_wchar_to_utf7imap),
    check: Some(mb_check_utf7imap),
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[&ENC_UTF7, &ENC_UTF7IMAP];
