//! First compile phase: pcre2_compile.c parse_regex() and its helpers
//! (read_number, read_repeat_counts, check_escape, get_ucp, read_name, ...),
//! which turn the pattern into the "parsed pattern" vector of u32 items.

use crate::cb::{Cb, NamedGroup};
use crate::consts::*;
use crate::tables;
use crate::util::*;

const ESCAPES_FIRST: u32 = b'0' as u32;
const ESCAPES_LAST: u32 = b'z' as u32;

#[rustfmt::skip]
static ESCAPES: [i16; 75] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    b':' as i16, b';' as i16, b'<' as i16, b'=' as i16, b'>' as i16, b'?' as i16,
    b'@' as i16, -(ESC_A as i16), -(ESC_B as i16), -(ESC_C as i16), -(ESC_D as i16),
    -(ESC_E as i16), 0, -(ESC_G as i16), -(ESC_H as i16), 0, 0, -(ESC_K as i16), 0, 0,
    -(ESC_N as i16), 0, -(ESC_P as i16), -(ESC_Q as i16), -(ESC_R as i16), -(ESC_S as i16),
    0, 0, -(ESC_V as i16), -(ESC_W as i16), -(ESC_X as i16), 0, -(ESC_Z as i16),
    b'[' as i16, b'\\' as i16, b']' as i16, b'^' as i16, b'_' as i16, b'`' as i16,
    7, -(ESC_b as i16), 0, -(ESC_d as i16), 27, 12, 0, -(ESC_h as i16), 0, 0,
    -(ESC_k as i16), 0, 0, 10, 0, -(ESC_p as i16), 0, 13, -(ESC_s as i16), 9, 0,
    -(ESC_v as i16), -(ESC_w as i16), 0, 0, -(ESC_z as i16),
];

#[inline]
pub fn xdigit(c: u32) -> u32 {
    match c {
        0x30..=0x39 => c - 0x30,
        0x41..=0x46 => c - 0x41 + 10,
        0x61..=0x66 => c - 0x61 + 10,
        _ => 0xff,
    }
}

#[inline]
fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

#[inline]
fn c_isspace(c: u32) -> bool {
    matches!(c, 0x20 | 0x09..=0x0d)
}

struct Verb {
    len: usize,
    meta: u32,
    has_arg: i32,
}

static VERBNAMES: &[&[u8]] = &[
    b"", b"MARK", b"ACCEPT", b"F", b"FAIL", b"COMMIT", b"PRUNE", b"SKIP", b"THEN",
];
static VERBS: [Verb; 9] = [
    Verb {
        len: 0,
        meta: META_MARK,
        has_arg: 1,
    },
    Verb {
        len: 4,
        meta: META_MARK,
        has_arg: 1,
    },
    Verb {
        len: 6,
        meta: META_ACCEPT,
        has_arg: -1,
    },
    Verb {
        len: 1,
        meta: META_FAIL,
        has_arg: -1,
    },
    Verb {
        len: 4,
        meta: META_FAIL,
        has_arg: -1,
    },
    Verb {
        len: 6,
        meta: META_COMMIT,
        has_arg: 0,
    },
    Verb {
        len: 5,
        meta: META_PRUNE,
        has_arg: 0,
    },
    Verb {
        len: 4,
        meta: META_SKIP,
        has_arg: 0,
    },
    Verb {
        len: 4,
        meta: META_THEN,
        has_arg: 0,
    },
];

static ALASNAMES: &[(&[u8], u32)] = &[
    (b"pla", META_LOOKAHEAD),
    (b"plb", META_LOOKBEHIND),
    (b"napla", META_LOOKAHEAD_NA),
    (b"naplb", META_LOOKBEHIND_NA),
    (b"nla", META_LOOKAHEADNOT),
    (b"nlb", META_LOOKBEHINDNOT),
    (b"positive_lookahead", META_LOOKAHEAD),
    (b"positive_lookbehind", META_LOOKBEHIND),
    (b"non_atomic_positive_lookahead", META_LOOKAHEAD_NA),
    (b"non_atomic_positive_lookbehind", META_LOOKBEHIND_NA),
    (b"negative_lookahead", META_LOOKAHEADNOT),
    (b"negative_lookbehind", META_LOOKBEHINDNOT),
    (b"atomic", META_ATOMIC),
    (b"sr", META_SCRIPT_RUN),
    (b"asr", META_ATOMIC_SCRIPT_RUN),
    (b"script_run", META_SCRIPT_RUN),
    (b"atomic_script_run", META_ATOMIC_SCRIPT_RUN),
];

pub static POSIX_NAMES: &[&[u8]] = &[
    b"alpha", b"lower", b"upper", b"alnum", b"ascii", b"blank", b"cntrl", b"digit", b"graph",
    b"print", b"punct", b"space", b"word", b"xdigit",
];
pub const PC_DIGIT: i32 = 7;
pub const PC_GRAPH: i32 = 8;
pub const PC_PRINT: i32 = 9;
pub const PC_PUNCT: i32 = 10;
pub const PC_XDIGIT: i32 = 13;

static POSIX_SUBSTITUTES: [i32; 28] = [
    PT_GC as i32,
    ucp_L as i32,
    PT_PC as i32,
    ucp_Ll as i32,
    PT_PC as i32,
    ucp_Lu as i32,
    PT_ALNUM as i32,
    0,
    -1,
    0,
    -1,
    1,
    PT_PC as i32,
    ucp_Cc as i32,
    PT_PC as i32,
    ucp_Nd as i32,
    PT_PXGRAPH as i32,
    0,
    PT_PXPRINT as i32,
    0,
    PT_PXPUNCT as i32,
    0,
    PT_PXSPACE as i32,
    0,
    PT_WORD as i32,
    0,
    PT_PXXDIGIT as i32,
    0,
];

const NSF_RESET: u16 = 0x0001;
const NSF_CONDASSERT: u16 = 0x0002;
const NSF_ATOMICSR: u16 = 0x0004;

const PARSE_TRACKED_OPTIONS: u32 = PCRE2_CASELESS
    | PCRE2_DOTALL
    | PCRE2_DUPNAMES
    | PCRE2_EXTENDED
    | PCRE2_EXTENDED_MORE
    | PCRE2_MULTILINE
    | PCRE2_NO_AUTO_CAPTURE
    | PCRE2_UNGREEDY;
const PARSE_TRACKED_EXTRA_OPTIONS: u32 = PCRE2_EXTRA_CASELESS_RESTRICT
    | PCRE2_EXTRA_ASCII_BSD
    | PCRE2_EXTRA_ASCII_BSS
    | PCRE2_EXTRA_ASCII_BSW
    | PCRE2_EXTRA_ASCII_DIGIT
    | PCRE2_EXTRA_ASCII_POSIX;

const RANGE_NO: u32 = 0;
const RANGE_STARTED: u32 = 1;
const RANGE_OK_ESCAPED: u32 = 2;
const RANGE_OK_LITERAL: u32 = 3;

#[derive(Clone, Copy, Default)]
struct NestSave {
    nest_depth: u16,
    reset_group: u16,
    max_group: u16,
    flags: u16,
    options: u32,
    xoptions: u32,
}

/// read_number(): returns (yield, value).
pub fn read_number(
    pat: &[u8],
    ptrptr: &mut usize,
    ptrend: usize,
    allow_sign: i32,
    mut max_value: u32,
    max_error: i32,
    intptr: &mut i32,
    errorcodeptr: &mut i32,
) -> bool {
    let mut sign = 0;
    let mut n: u32 = 0;
    let mut ptr = *ptrptr;
    let mut yield_ = false;
    *errorcodeptr = 0;
    if allow_sign >= 0 && ptr < ptrend {
        if pat[ptr] == b'+' {
            sign = 1;
            max_value = max_value.wrapping_sub(allow_sign as u32);
            ptr += 1;
        } else if pat[ptr] == b'-' {
            sign = -1;
            ptr += 1;
        }
    }
    if ptr >= ptrend || !is_digit(pat[ptr]) {
        return false;
    }
    'exit: {
        while ptr < ptrend && is_digit(pat[ptr]) {
            n = n.wrapping_mul(10).wrapping_add((pat[ptr] - b'0') as u32);
            ptr += 1;
            if n > max_value {
                *errorcodeptr = max_error;
                break 'exit;
            }
        }
        if allow_sign >= 0 && sign != 0 {
            if n == 0 {
                *errorcodeptr = ERR26;
                break 'exit;
            }
            if sign > 0 {
                n = n.wrapping_add(allow_sign as u32);
            } else if (n as i32) > allow_sign {
                *errorcodeptr = ERR15;
                break 'exit;
            } else {
                n = (allow_sign as u32).wrapping_add(1).wrapping_sub(n);
            }
        }
        yield_ = true;
    }
    *intptr = n as i32;
    *ptrptr = ptr;
    yield_
}

#[inline]
fn skip_sp(pat: &[u8], mut p: usize, end: usize) -> usize {
    while p < end && (pat[p] == b' ' || pat[p] == b'\t') {
        p += 1;
    }
    p
}

/// read_repeat_counts()
pub fn read_repeat_counts(
    pat: &[u8],
    ptrptr: &mut usize,
    ptrend: usize,
    minp: Option<&mut u32>,
    maxp: Option<&mut u32>,
    errorcodeptr: &mut i32,
) -> bool {
    let mut p = *ptrptr;
    let mut yield_ = false;
    let mut had_minimum = false;
    let mut min: i32 = 0;
    let mut max: i32 = REPEAT_UNLIMITED as i32;
    *errorcodeptr = 0;
    p = skip_sp(pat, p, ptrend);
    let mut pp = p;
    if pp < ptrend && is_digit(pat[pp]) {
        had_minimum = true;
        pp += 1;
        while pp < ptrend && is_digit(pat[pp]) {
            pp += 1;
        }
    }
    pp = skip_sp(pat, pp, ptrend);
    if pp >= ptrend {
        return false;
    }
    if pat[pp] == b'}' {
        if !had_minimum {
            return false;
        }
    } else {
        let ch = pat[pp];
        pp += 1;
        if ch != b',' {
            return false;
        }
        pp = skip_sp(pat, pp, ptrend);
        if pp >= ptrend {
            return false;
        }
        if is_digit(pat[pp]) {
            pp += 1;
            while pp < ptrend && is_digit(pat[pp]) {
                pp += 1;
            }
        } else if !had_minimum {
            return false;
        }
        pp = skip_sp(pat, pp, ptrend);
        if pp >= ptrend || pat[pp] != b'}' {
            return false;
        }
    }
    'exit: {
        if !read_number(
            pat,
            &mut p,
            ptrend,
            -1,
            MAX_REPEAT_COUNT,
            ERR5,
            &mut min,
            errorcodeptr,
        ) {
            if *errorcodeptr != 0 {
                break 'exit;
            }
            p += 1;
            p = skip_sp(pat, p, ptrend);
            if !read_number(
                pat,
                &mut p,
                ptrend,
                -1,
                MAX_REPEAT_COUNT,
                ERR5,
                &mut max,
                errorcodeptr,
            ) && *errorcodeptr != 0
            {
                break 'exit;
            }
        } else {
            p = skip_sp(pat, p, ptrend);
            if pat[p] == b'}' {
                max = min;
            } else {
                p += 1;
                p = skip_sp(pat, p, ptrend);
                if !read_number(
                    pat,
                    &mut p,
                    ptrend,
                    -1,
                    MAX_REPEAT_COUNT,
                    ERR5,
                    &mut max,
                    errorcodeptr,
                ) && *errorcodeptr != 0
                {
                    break 'exit;
                }
                if max < min {
                    *errorcodeptr = ERR4;
                    break 'exit;
                }
            }
        }
        p = skip_sp(pat, p, ptrend);
        p += 1;
        yield_ = true;
        if let Some(m) = minp {
            *m = min as u32;
        }
        if let Some(m) = maxp {
            *m = max as u32;
        }
    }
    *ptrptr = p;
    yield_
}

/// PRIV(check_escape)(). Returns 0 for a data character (in *chptr), a
/// positive ESC_xxx value, or a negated back reference number.
#[allow(clippy::too_many_arguments)]
pub fn check_escape(
    pat: &[u8],
    ptrptr: &mut usize,
    ptrend: usize,
    chptr: &mut u32,
    errorcodeptr: &mut i32,
    options: u32,
    xoptions: u32,
    isclass: bool,
    cb: &mut Cb,
) -> i32 {
    let utf = (options & PCRE2_UTF) != 0;
    let alt_bsux = ((options & PCRE2_ALT_BSUX) | (xoptions & PCRE2_EXTRA_ALT_BSUX)) != 0;
    let mut ptr = *ptrptr;
    let mut escape: i32 = 0;
    if ptr >= ptrend {
        *errorcodeptr = ERR1;
        return 0;
    }
    let (mut c, l) = getchar_test(pat, ptr, utf);
    ptr += l;
    *errorcodeptr = 0;
    let i_esc = if (ESCAPES_FIRST..=ESCAPES_LAST).contains(&c) {
        ESCAPES[(c - ESCAPES_FIRST) as usize] as i32
    } else {
        0
    };
    // Shared target for \x{ and \N{U+ (COME_FROM_NU).
    let mut come_from_nu = false;
    if !(ESCAPES_FIRST..=ESCAPES_LAST).contains(&c) {
        // Not an escape-table character: a literal.
    } else if i_esc != 0 {
        if i_esc > 0 {
            c = i_esc as u32;
            if c == CHAR_CR && (xoptions & PCRE2_EXTRA_ESCAPED_CR_IS_LF) != 0 {
                c = CHAR_LF;
            }
        } else {
            escape = -i_esc;
            if escape == ESC_P as i32 || escape == ESC_p as i32 || escape == ESC_X as i32 {
                cb.external_flags |= PCRE2_HASBKPORX;
            }
            if escape == ESC_N as i32 && ptr < ptrend && pat[ptr] == b'{' {
                let mut p = ptr + 1;
                p = skip_sp(pat, p, ptrend);
                if ptrend - p > 1 && pat[p] == b'U' && pat[p + 1] == b'+' {
                    if utf {
                        ptr = p + 2;
                        escape = 0;
                        come_from_nu = true;
                    } else {
                        *errorcodeptr = ERR93;
                    }
                } else if !read_repeat_counts(pat, &mut p, ptrend, None, None, errorcodeptr)
                    && *errorcodeptr == 0
                {
                    *errorcodeptr = ERR37;
                }
            }
        }
    } else {
        // Escapes that need further processing, including octal/backrefs.
        let mut s: i32 = 0;
        let mut i: i32 = 0;
        'sw: {
            if come_from_nu {
                break 'sw;
            }
            match c as u8 {
                b'F' | b'l' | b'L' => {
                    *errorcodeptr = ERR37;
                }
                b'u' => {
                    if !alt_bsux {
                        *errorcodeptr = ERR37;
                    } else {
                        if ptr >= ptrend {
                            break 'sw;
                        }
                        if pat[ptr] == b'{' && (xoptions & PCRE2_EXTRA_ALT_BSUX) != 0 {
                            let mut hptr = ptr + 1;
                            let mut cc: u32 = 0;
                            let mut broke = false;
                            while hptr < ptrend {
                                let xc = xdigit(pat[hptr] as u32);
                                if xc == 0xff {
                                    break;
                                }
                                if (cc & 0xf0000000) != 0 {
                                    *errorcodeptr = ERR77;
                                    ptr = hptr;
                                    broke = true;
                                    break;
                                }
                                cc = (cc << 4) | xc;
                                hptr += 1;
                            }
                            if broke {
                                // fall through to the hptr checks as in C
                            }
                            if hptr == ptr + 1 || hptr >= ptrend || pat[hptr] != b'}' {
                                escape = ESC_ub as i32;
                                ptr += 1;
                                break 'sw;
                            }
                            c = cc;
                            ptr = hptr + 1;
                        } else {
                            if ptrend - ptr < 4 {
                                break 'sw;
                            }
                            let mut cc = xdigit(pat[ptr] as u32);
                            if cc == 0xff {
                                break 'sw;
                            }
                            let mut xc = xdigit(pat[ptr + 1] as u32);
                            if xc == 0xff {
                                break 'sw;
                            }
                            cc = (cc << 4) | xc;
                            xc = xdigit(pat[ptr + 2] as u32);
                            if xc == 0xff {
                                break 'sw;
                            }
                            cc = (cc << 4) | xc;
                            xc = xdigit(pat[ptr + 3] as u32);
                            if xc == 0xff {
                                break 'sw;
                            }
                            c = (cc << 4) | xc;
                            ptr += 4;
                        }
                        if utf {
                            if c > 0x10ffff {
                                *errorcodeptr = ERR77;
                            } else if (0xd800..=0xdfff).contains(&c)
                                && (xoptions & PCRE2_EXTRA_ALLOW_SURROGATE_ESCAPES) == 0
                            {
                                *errorcodeptr = ERR73;
                            }
                        } else if c > MAX_NON_UTF_CHAR {
                            *errorcodeptr = ERR77;
                        }
                    }
                }
                b'U' => {
                    if !alt_bsux {
                        *errorcodeptr = ERR37;
                    }
                }
                b'g' => {
                    if isclass {
                        break 'sw;
                    }
                    if ptr >= ptrend {
                        *errorcodeptr = ERR57;
                        break 'sw;
                    }
                    if pat[ptr] == b'<' || pat[ptr] == b'\'' {
                        escape = ESC_g as i32;
                        break 'sw;
                    }
                    if pat[ptr] == b'{' {
                        let mut p = ptr + 1;
                        p = skip_sp(pat, p, ptrend);
                        if !read_number(
                            pat,
                            &mut p,
                            ptrend,
                            cb.bracount as i32,
                            MAX_GROUP_NUMBER,
                            ERR61,
                            &mut s,
                            errorcodeptr,
                        ) {
                            if *errorcodeptr == 0 {
                                escape = ESC_k as i32;
                            }
                            break 'sw;
                        }
                        p = skip_sp(pat, p, ptrend);
                        if p >= ptrend || pat[p] != b'}' {
                            *errorcodeptr = ERR57;
                            break 'sw;
                        }
                        ptr = p + 1;
                    } else if !read_number(
                        pat,
                        &mut ptr,
                        ptrend,
                        cb.bracount as i32,
                        MAX_GROUP_NUMBER,
                        ERR61,
                        &mut s,
                        errorcodeptr,
                    ) {
                        if *errorcodeptr == 0 {
                            *errorcodeptr = ERR57;
                        }
                        break 'sw;
                    }
                    if s <= 0 {
                        *errorcodeptr = ERR15;
                        break 'sw;
                    }
                    escape = -s;
                }
                b'1'..=b'9' | b'0' => {
                    if c != b'0' as u32 {
                        if !isclass {
                            let oldptr = ptr;
                            ptr -= 1;
                            if read_number(
                                pat,
                                &mut ptr,
                                ptrend,
                                -1,
                                (i32::MAX / 10 - 1) as u32,
                                0,
                                &mut s,
                                errorcodeptr,
                            ) && (s < 10 || pat[oldptr - 1] >= b'8' || s <= cb.bracount as i32)
                            {
                                if s > MAX_GROUP_NUMBER as i32 {
                                    *errorcodeptr = ERR61;
                                } else {
                                    escape = -s;
                                }
                                break 'sw;
                            }
                            ptr = oldptr;
                        }
                        if c >= b'8' as u32 {
                            break 'sw;
                        }
                    }
                    // Octal (CHAR_0 case, also reached by fall-through).
                    c -= b'0' as u32;
                    loop {
                        let ok = i < 2;
                        i += 1;
                        if !(ok && ptr < ptrend && pat[ptr] >= b'0' && pat[ptr] <= b'7') {
                            break;
                        }
                        c = c * 8 + (pat[ptr] - b'0') as u32;
                        ptr += 1;
                    }
                    if !utf && c > 0xff {
                        *errorcodeptr = ERR51;
                    }
                }
                b'o' => {
                    if ptr >= ptrend || {
                        let ch = pat[ptr];
                        ptr += 1;
                        ch != b'{'
                    } {
                        ptr -= 1;
                        *errorcodeptr = ERR55;
                        break 'sw;
                    }
                    ptr = skip_sp(pat, ptr, ptrend);
                    if ptr >= ptrend || pat[ptr] == b'}' {
                        *errorcodeptr = ERR78;
                        break 'sw;
                    }
                    c = 0;
                    let mut overflow = false;
                    while ptr < ptrend && pat[ptr] >= b'0' && pat[ptr] <= b'7' {
                        let cc = pat[ptr] as u32;
                        ptr += 1;
                        if c == 0 && cc == b'0' as u32 {
                            continue;
                        }
                        c = (c << 3) + (cc - b'0' as u32);
                        if c > (if utf { 0x10ffff } else { 0xff }) {
                            overflow = true;
                            break;
                        }
                    }
                    ptr = skip_sp(pat, ptr, ptrend);
                    if overflow {
                        while ptr < ptrend && pat[ptr] >= b'0' && pat[ptr] <= b'7' {
                            ptr += 1;
                        }
                        *errorcodeptr = ERR34;
                    } else if ptr < ptrend && {
                        let ch = pat[ptr];
                        ptr += 1;
                        ch == b'}'
                    } {
                        if utf
                            && (0xd800..=0xdfff).contains(&c)
                            && (xoptions & PCRE2_EXTRA_ALLOW_SURROGATE_ESCAPES) == 0
                        {
                            ptr -= 1;
                            *errorcodeptr = ERR73;
                        }
                    } else {
                        ptr -= 1;
                        *errorcodeptr = ERR64;
                    }
                }
                b'x' => {
                    if alt_bsux {
                        if ptrend - ptr < 2 {
                            break 'sw;
                        }
                        let cc = xdigit(pat[ptr] as u32);
                        if cc == 0xff {
                            break 'sw;
                        }
                        let xc = xdigit(pat[ptr + 1] as u32);
                        if xc == 0xff {
                            break 'sw;
                        }
                        c = (cc << 4) | xc;
                        ptr += 2;
                    } else if ptr < ptrend && pat[ptr] == b'{' {
                        ptr += 1;
                        ptr = skip_sp(pat, ptr, ptrend);
                        come_from_nu = true;
                    } else {
                        c = 0;
                        if ptr >= ptrend {
                            break 'sw;
                        }
                        let cc = xdigit(pat[ptr] as u32);
                        if cc == 0xff {
                            break 'sw;
                        }
                        ptr += 1;
                        c = cc;
                        if ptr >= ptrend {
                            break 'sw;
                        }
                        let cc = xdigit(pat[ptr] as u32);
                        if cc == 0xff {
                            break 'sw;
                        }
                        ptr += 1;
                        c = (c << 4) | cc;
                    }
                }
                b'c' => {
                    if ptr >= ptrend {
                        *errorcodeptr = ERR2;
                        break 'sw;
                    }
                    c = pat[ptr] as u32;
                    if (b'a' as u32..=b'z' as u32).contains(&c) {
                        c -= 32;
                    }
                    if !(32..=126).contains(&c) {
                        *errorcodeptr = ERR68;
                        break 'sw;
                    }
                    c ^= 0x40;
                    ptr += 1;
                }
                _ => {
                    *errorcodeptr = ERR3;
                    *ptrptr = ptr - 1;
                    return 0;
                }
            }
        }
    }
    if come_from_nu {
        // \x{...} or \N{U+...} hex digits.
        'nu: {
            if ptr >= ptrend || pat[ptr] == b'}' {
                *errorcodeptr = ERR78;
                break 'nu;
            }
            c = 0;
            let mut overflow = false;
            while ptr < ptrend {
                let cc = xdigit(pat[ptr] as u32);
                if cc == 0xff {
                    break;
                }
                ptr += 1;
                if c == 0 && cc == 0 {
                    continue;
                }
                c = (c << 4) | cc;
                if (utf && c > 0x10ffff) || (!utf && c > MAX_NON_UTF_CHAR) {
                    overflow = true;
                    break;
                }
            }
            ptr = skip_sp(pat, ptr, ptrend);
            if overflow {
                while ptr < ptrend && xdigit(pat[ptr] as u32) != 0xff {
                    ptr += 1;
                }
                *errorcodeptr = ERR34;
            } else if ptr < ptrend && {
                let ch = pat[ptr];
                ptr += 1;
                ch == b'}'
            } {
                if utf
                    && (0xd800..=0xdfff).contains(&c)
                    && (xoptions & PCRE2_EXTRA_ALLOW_SURROGATE_ESCAPES) == 0
                {
                    ptr -= 1;
                    *errorcodeptr = ERR73;
                }
            } else {
                ptr -= 1;
                *errorcodeptr = ERR67;
            }
        }
    }
    *ptrptr = ptr;
    *chptr = c;
    escape
}

/// get_ucp(): read a Unicode property name after \p or \P.
pub fn get_ucp(
    cb: &Cb,
    ptrptr: &mut usize,
    negptr: &mut bool,
    ptypeptr: &mut u16,
    pdataptr: &mut u16,
    errorcodeptr: &mut i32,
) -> bool {
    let pat = cb.pat;
    let end = pat.len();
    let mut ptr = *ptrptr;
    let mut name: Vec<u8> = Vec::with_capacity(50);
    let mut vptr: Option<usize> = None;
    let mut ptscript = PT_NOTSCRIPT;
    'err: {
        if ptr >= end {
            break 'err;
        }
        let mut c = pat[ptr] as u32;
        ptr += 1;
        *negptr = false;
        if c == b'{' as u32 {
            if ptr >= end {
                break 'err;
            }
            if pat[ptr] == b'^' {
                *negptr = true;
                ptr += 1;
            }
            let mut i = 0;
            while i < 49 {
                if ptr >= end {
                    break 'err;
                }
                c = pat[ptr] as u32;
                ptr += 1;
                while c == b'_' as u32 || c == b'-' as u32 || c_isspace(c) {
                    if ptr >= end {
                        break 'err;
                    }
                    c = pat[ptr] as u32;
                    ptr += 1;
                }
                if c == 0 {
                    break 'err;
                }
                if c == b'}' as u32 {
                    break;
                }
                name.push((c as u8).to_ascii_lowercase());
                if (c == b':' as u32 || c == b'=' as u32) && vptr.is_none() {
                    vptr = Some(i);
                }
                i += 1;
            }
            if c != b'}' as u32 {
                break 'err;
            }
        } else if (cb.ctypes(c) & ctype_letter) != 0 {
            name.push((c as u8).to_ascii_lowercase());
        } else {
            break 'err;
        }
        *ptrptr = ptr;

        if let Some(v) = vptr {
            let key = &name[..v];
            let mut prefix: &[u8] = b"";
            if key == b"bidiclass" || key == b"bc" {
                prefix = b"bidi";
            } else if key == b"script" || key == b"sc" {
                ptscript = PT_SC;
            } else if key == b"scriptextensions" || key == b"scx" {
                ptscript = PT_SCX;
            } else {
                *errorcodeptr = ERR47;
                return false;
            }
            let mut newname = prefix.to_vec();
            newname.extend_from_slice(&name[v + 1..]);
            name = newname;
        }

        // Binary chop search of the property table.
        let mut bot = 0usize;
        let mut top = tables::UTT.len();
        while bot < top {
            let i = (bot + top) >> 1;
            let off = tables::UTT[i].0 as usize;
            let tn = &tables::UTT_NAMES[off..];
            let tlen = tn.iter().position(|&b| b == 0).unwrap_or(tn.len());
            let r = name.as_slice().cmp(&tn[..tlen]);
            if r == std::cmp::Ordering::Equal {
                *pdataptr = tables::UTT[i].2;
                if vptr.is_none() || ptscript == PT_NOTSCRIPT {
                    *ptypeptr = tables::UTT[i].1;
                    return true;
                }
                match tables::UTT[i].1 as u32 {
                    PT_SC => {
                        *ptypeptr = PT_SC as u16;
                        return true;
                    }
                    PT_SCX => {
                        *ptypeptr = ptscript as u16;
                        return true;
                    }
                    _ => {}
                }
                break;
            }
            if r == std::cmp::Ordering::Greater {
                bot = i + 1;
            } else {
                top = i;
            }
        }
        *errorcodeptr = ERR47;
        return false;
    }
    *errorcodeptr = ERR46;
    *ptrptr = ptr;
    false
}

/// check_posix_syntax(): ptr points at the terminator character (: . or =).
pub fn check_posix_syntax(pat: &[u8], mut ptr: usize, ptrend: usize, endptr: &mut usize) -> bool {
    let terminator = pat[ptr];
    ptr += 1;
    while ptrend - ptr >= 2 {
        if pat[ptr] == b'\\' && (pat[ptr + 1] == b']' || pat[ptr + 1] == b'\\') {
            ptr += 1;
        } else if (pat[ptr] == b'[' && pat[ptr + 1] == terminator) || pat[ptr] == b']' {
            return false;
        } else if pat[ptr] == terminator && pat[ptr + 1] == b']' {
            *endptr = ptr;
            return true;
        }
        ptr += 1;
    }
    false
}

pub fn check_posix_name(pat: &[u8], ptr: usize, len: usize) -> i32 {
    for (i, n) in POSIX_NAMES.iter().enumerate() {
        if len == n.len() && pat.len() >= ptr + len && &pat[ptr..ptr + len] == *n {
            return i as i32;
        }
    }
    -1
}

/// read_name(): `*ptrptr` points at the character before the name.
#[allow(clippy::too_many_arguments)]
pub fn read_name(
    cb: &Cb,
    ptrptr: &mut usize,
    ptrend: usize,
    utf: bool,
    terminator: u32,
    offsetptr: &mut usize,
    nameptr: &mut usize,
    namelenptr: &mut u32,
    errorcodeptr: &mut i32,
) -> bool {
    let pat = cb.pat;
    let mut ptr = *ptrptr;
    let is_group = pat[ptr] != b'*';
    ptr += 1;
    let is_braced = terminator == b'}' as u32;
    'failed: {
        if is_braced {
            ptr = skip_sp(pat, ptr, ptrend);
        }
        if ptr >= ptrend {
            *errorcodeptr = if is_group { ERR62 } else { ERR60 };
            break 'failed;
        }
        *nameptr = ptr;
        *offsetptr = ptr;
        if utf && is_group {
            let (mut c, _) = getchar(pat, ptr);
            let mut ty = ucd_chartype(c);
            if ty == ucp_Nd {
                *errorcodeptr = ERR44;
                break 'failed;
            }
            loop {
                if ty != ucp_Nd && tables::UCP_GENTYPE[ty as usize] != ucp_L && c != b'_' as u32 {
                    break;
                }
                ptr += 1;
                ptr = forwardchar_test(pat, ptr, ptrend);
                if ptr >= ptrend {
                    break;
                }
                c = getchar(pat, ptr).0;
                ty = ucd_chartype(c);
            }
        } else {
            if is_group && is_digit(pat[ptr]) {
                *errorcodeptr = ERR44;
                break 'failed;
            }
            while ptr < ptrend && (cb.ctypes(pat[ptr] as u32) & ctype_word) != 0 {
                ptr += 1;
            }
        }
        if ptr > *nameptr + MAX_NAME_SIZE {
            *errorcodeptr = ERR48;
            break 'failed;
        }
        *namelenptr = (ptr - *nameptr) as u32;
        if is_group {
            if ptr == *nameptr {
                *errorcodeptr = ERR62;
                break 'failed;
            }
            if is_braced {
                ptr = skip_sp(pat, ptr, ptrend);
            }
            if ptr >= ptrend || pat[ptr] as u32 != terminator {
                *errorcodeptr = ERR42;
                break 'failed;
            }
            ptr += 1;
        }
        *ptrptr = ptr;
        return true;
    }
    *ptrptr = ptr;
    false
}

impl<'a> Cb<'a> {
    /// Write a parsed-pattern item, growing the vector when needed.
    #[inline]
    fn pw(&mut self, i: usize, v: u32) {
        if i >= self.parsed.len() {
            self.parsed.resize(i + 64, 0);
        }
        self.parsed[i] = v;
    }
    #[inline]
    fn pr(&self, i: usize) -> u32 {
        *self.parsed.get(i).unwrap_or(&0)
    }

    /// manage_callouts()
    fn manage_callouts(
        &mut self,
        ptr: usize,
        pcallout: &mut Option<usize>,
        auto_callout: bool,
        mut pp: usize,
    ) -> usize {
        let mut previous_callout = *pcallout;
        if let Some(pc) = previous_callout {
            let v = (ptr - self.pr(pc + 1) as usize) as u32;
            self.pw(pc + 2, v);
        }
        if !auto_callout {
            previous_callout = None;
        } else {
            if previous_callout.is_none()
                || previous_callout != pp.checked_sub(4)
                || self.pr(previous_callout.unwrap() + 3) != 255
            {
                previous_callout = Some(pp);
                pp += 4;
                let pc = previous_callout.unwrap();
                self.pw(pc, META_CALLOUT_NUMBER);
                self.pw(pc + 2, 0);
                self.pw(pc + 3, 255);
            }
            let pc = previous_callout.unwrap();
            self.pw(pc + 1, ptr as u32);
        }
        *pcallout = previous_callout;
        pp
    }

    /// handle_escdsw()
    fn handle_escdsw(&mut self, escape: u32, mut pp: usize, options: u32, xoptions: u32) -> usize {
        let mut ascii_option = 0;
        let mut prop = ESC_p;
        match escape {
            ESC_D => {
                prop = ESC_P;
                ascii_option = PCRE2_EXTRA_ASCII_BSD;
            }
            ESC_d => ascii_option = PCRE2_EXTRA_ASCII_BSD,
            ESC_S => {
                prop = ESC_P;
                ascii_option = PCRE2_EXTRA_ASCII_BSS;
            }
            ESC_s => ascii_option = PCRE2_EXTRA_ASCII_BSS,
            ESC_W => {
                prop = ESC_P;
                ascii_option = PCRE2_EXTRA_ASCII_BSW;
            }
            ESC_w => ascii_option = PCRE2_EXTRA_ASCII_BSW,
            _ => {}
        }
        if (options & PCRE2_UCP) == 0 || (xoptions & ascii_option) != 0 {
            self.pw(pp, META_ESCAPE + escape);
            pp += 1;
        } else {
            self.pw(pp, META_ESCAPE + prop);
            pp += 1;
            match escape {
                ESC_d | ESC_D => {
                    self.pw(pp, (PT_PC << 16) | ucp_Nd);
                    pp += 1;
                }
                ESC_s | ESC_S => {
                    self.pw(pp, PT_SPACE << 16);
                    pp += 1;
                }
                ESC_w | ESC_W => {
                    self.pw(pp, PT_WORD << 16);
                    pp += 1;
                }
                _ => {}
            }
        }
        pp
    }

    #[inline]
    fn put_offset(&mut self, pp: &mut usize, off: usize) {
        self.pw(*pp, (off >> 32) as u32);
        self.pw(*pp + 1, (off & 0xffffffff) as u32);
        *pp += 2;
    }

    /// parse_regex(): returns 0 on success or an error code (with
    /// cb.erroroffset set).
    #[allow(unused_assignments)]
    pub fn parse_regex(
        &mut self,
        start: usize,
        mut options: u32,
        has_lookbehind: &mut bool,
    ) -> i32 {
        let pat = self.pat;
        let mut ptr = start;
        let ptrend = pat.len();
        let mut c: u32;
        let mut namelen: u32 = 0;
        let mut class_range_state: u32;
        let mut verblengthptr: usize = 0;
        let mut verbstartptr: usize = 0;
        let mut previous_callout: Option<usize> = None;
        let mut pp: usize = 0;
        let parsed_pattern_end = self.parsed_end;
        let mut this_parsed_item: usize = usize::MAX;
        let mut prev_parsed_item: usize = usize::MAX;
        let mut meta_quantifier: u32 = 0;
        let mut add_after_mark: u32 = 0;
        let mut xoptions = self.cx_extra_options;
        let mut nest_depth: u16 = 0;
        let mut after_manual_callout: i32 = 0;
        let mut expect_cond_assert: i32 = 0;
        let mut errorcode: i32 = 0;
        let mut inescq = false;
        let mut inverbname = false;
        let utf = (options & PCRE2_UTF) != 0;
        let auto_callout = (options & PCRE2_AUTO_CALLOUT) != 0;
        let mut okquantifier = false;
        let mut thisptr: usize;
        let mut name: usize = 0;
        let mut verbnamestart: usize = 0;
        let mut nests: Vec<NestSave> = Vec::new();
        let max_nests = (self.workspace_size - self.workspace_size % 16) / 16;

        macro_rules! push {
            ($v:expr) => {{
                let v = $v;
                self.pw(pp, v);
                pp += 1;
            }};
        }
        macro_rules! parsed_literal {
            ($v:expr) => {{
                push!($v);
                okquantifier = true;
            }};
        }
        macro_rules! getcharinctest {
            () => {{
                let (cc, l) = getchar_test(pat, ptr, utf);
                ptr += l;
                cc
            }};
        }

        // The 'failed block: break out of it with errorcode set to report an
        // error at `ptr`.
        'failed: {
            if (xoptions & PCRE2_EXTRA_MATCH_LINE) != 0 {
                push!(META_CIRCUMFLEX);
                push!(META_NOCAPTURE);
            } else if (xoptions & PCRE2_EXTRA_MATCH_WORD) != 0 {
                push!(META_ESCAPE + ESC_b);
                push!(META_NOCAPTURE);
            }

            if (options & PCRE2_LITERAL) != 0 {
                while ptr < ptrend {
                    if pp >= parsed_pattern_end {
                        errorcode = ERR63;
                        break 'failed;
                    }
                    thisptr = ptr;
                    c = getcharinctest!();
                    if auto_callout {
                        pp = self.manage_callouts(thisptr, &mut previous_callout, auto_callout, pp);
                    }
                    parsed_literal!(c);
                }
            } else {
                if (options & PCRE2_EXTENDED_MORE) != 0 {
                    options |= PCRE2_EXTENDED;
                }

                'mainloop: while ptr < ptrend {
                    let prev_expect_cond_assert: i32;
                    let mut min_repeat: u32 = 0;
                    let mut max_repeat: u32 = 0;
                    let prev_meta_quantifier: u32;
                    let prev_okquantifier: bool;
                    let mut tempptr: usize;
                    let mut offset: usize = 0;

                    if pp >= parsed_pattern_end {
                        errorcode = ERR63;
                        break 'failed;
                    }
                    if nest_depth as u32 > self.parens_nest_limit {
                        errorcode = ERR19;
                        break 'failed;
                    }
                    if this_parsed_item != pp {
                        prev_parsed_item = this_parsed_item;
                        this_parsed_item = pp;
                    }

                    thisptr = ptr;
                    c = getcharinctest!();

                    // Copy quoted literals until \E.
                    if inescq {
                        if c == b'\\' as u32 && ptr < ptrend && pat[ptr] == b'E' {
                            inescq = false;
                            ptr += 1;
                        } else {
                            if expect_cond_assert > 0 {
                                ptr -= 1;
                                errorcode = ERR28;
                                break 'failed;
                            }
                            if inverbname {
                                push!(c);
                            } else {
                                let amc = after_manual_callout;
                                after_manual_callout -= 1;
                                if amc <= 0 {
                                    pp = self.manage_callouts(
                                        thisptr,
                                        &mut previous_callout,
                                        auto_callout,
                                        pp,
                                    );
                                }
                                parsed_literal!(c);
                            }
                            meta_quantifier = 0;
                        }
                        continue;
                    }

                    // Verb names.
                    if inverbname
                        && (((options & (PCRE2_EXTENDED | PCRE2_ALT_VERBNAMES))
                            != (PCRE2_EXTENDED | PCRE2_ALT_VERBNAMES))
                            || (c > 255 && (c | 1) != 0x200f && (c | 1) != 0x2029)
                            || (c < 256
                                && c != b'#' as u32
                                && (self.ctypes(c) & ctype_space) == 0
                                && c != CHAR_NEL))
                    {
                        match c {
                            0x29 => {
                                // ')'
                                inverbname = false;
                                let verbnamelength = pp - verblengthptr - 1;
                                if ptr - verbnamestart - 1 > MAX_MARK {
                                    ptr -= 1;
                                    errorcode = ERR76;
                                    break 'failed;
                                }
                                self.pw(verblengthptr, verbnamelength as u32);
                                if add_after_mark != 0 {
                                    push!(add_after_mark);
                                    add_after_mark = 0;
                                }
                            }
                            0x5c => {
                                // '\\'
                                let escape = if (options & PCRE2_ALT_VERBNAMES) != 0 {
                                    let e = check_escape(
                                        pat,
                                        &mut ptr,
                                        ptrend,
                                        &mut c,
                                        &mut errorcode,
                                        options,
                                        xoptions,
                                        false,
                                        self,
                                    );
                                    if errorcode != 0 {
                                        break 'failed;
                                    }
                                    e
                                } else {
                                    0
                                };
                                match escape {
                                    0 => push!(c),
                                    e if e == ESC_ub as i32 => {
                                        push!(b'u' as u32);
                                        parsed_literal!(b'{' as u32);
                                    }
                                    e if e == ESC_Q as i32 => inescq = true,
                                    e if e == ESC_E as i32 => {}
                                    _ => {
                                        errorcode = ERR40;
                                        break 'failed;
                                    }
                                }
                            }
                            _ => push!(c),
                        }
                        continue;
                    }

                    // \Q and \E handling.
                    if c == b'\\' as u32 && ptr < ptrend && (pat[ptr] == b'Q' || pat[ptr] == b'E') {
                        inescq = pat[ptr] == b'Q';
                        ptr += 1;
                        continue;
                    }

                    // Skip white space and comments in extended mode.
                    if (options & PCRE2_EXTENDED) != 0 {
                        if c < 256 && (self.ctypes(c) & ctype_space) != 0 {
                            continue;
                        }
                        if c == CHAR_NEL || (c | 1) == 0x200f || (c | 1) == 0x2029 {
                            continue;
                        }
                        if c == b'#' as u32 {
                            while ptr < ptrend {
                                if self.is_newline_pat(ptr, utf) {
                                    ptr += self.nllen as usize;
                                    break;
                                }
                                ptr += 1;
                                if utf {
                                    ptr = forwardchar_test(pat, ptr, ptrend);
                                }
                            }
                            continue;
                        }
                    }

                    // Skip (?# comments.
                    if c == b'(' as u32
                        && ptrend - ptr >= 2
                        && pat[ptr] == b'?'
                        && pat[ptr + 1] == b'#'
                    {
                        loop {
                            ptr += 1;
                            if ptr >= ptrend || pat[ptr] == b')' {
                                break;
                            }
                        }
                        if ptr >= ptrend {
                            errorcode = ERR18;
                            break 'failed;
                        }
                        ptr += 1;
                        continue;
                    }

                    // Automatic callouts (not before quantifiers).
                    if c != b'*' as u32
                        && c != b'+' as u32
                        && c != b'?' as u32
                        && (c != b'{' as u32 || {
                            tempptr = ptr;
                            !read_repeat_counts(
                                pat,
                                &mut tempptr,
                                ptrend,
                                None,
                                None,
                                &mut errorcode,
                            )
                        })
                    {
                        let amc = after_manual_callout;
                        after_manual_callout -= 1;
                        if amc <= 0 {
                            pp = self.manage_callouts(
                                thisptr,
                                &mut previous_callout,
                                auto_callout,
                                pp,
                            );
                            this_parsed_item = pp;
                        }
                    }

                    // An assertion is expected after (?( or (?C.
                    if expect_cond_assert > 0 {
                        let mut ok = c == b'(' as u32
                            && ptrend - ptr >= 3
                            && (pat[ptr] == b'?' || pat[ptr] == b'*');
                        if ok {
                            if pat[ptr] == b'*' {
                                ok = (self.ctypes(pat[ptr + 1] as u32) & ctype_lcletter) != 0;
                            } else {
                                ok = match pat[ptr + 1] {
                                    b'C' => expect_cond_assert == 2,
                                    b'=' | b'!' => true,
                                    b'<' => pat[ptr + 2] == b'=' || pat[ptr + 2] == b'!',
                                    _ => false,
                                };
                            }
                        }
                        if !ok {
                            ptr -= 1;
                            errorcode = ERR28;
                            break 'failed;
                        }
                    }

                    prev_expect_cond_assert = expect_cond_assert;
                    expect_cond_assert = 0;
                    prev_okquantifier = okquantifier;
                    prev_meta_quantifier = meta_quantifier;
                    okquantifier = false;
                    meta_quantifier = 0;

                    // Possessive or lazy modifier after a quantifier.
                    if prev_meta_quantifier != 0 && (c == b'?' as u32 || c == b'+' as u32) {
                        let idx = if prev_meta_quantifier == META_MINMAX {
                            pp - 3
                        } else {
                            pp - 1
                        };
                        self.pw(
                            idx,
                            prev_meta_quantifier
                                + if c == b'?' as u32 {
                                    0x00020000
                                } else {
                                    0x00010000
                                },
                        );
                        continue;
                    }

                    // The main switch.
                    let mut check_quantifier = false;
                    match c {
                        0x5c => {
                            // Backslash
                            tempptr = ptr;
                            let mut escape = check_escape(
                                pat,
                                &mut ptr,
                                ptrend,
                                &mut c,
                                &mut errorcode,
                                options,
                                xoptions,
                                false,
                                self,
                            );
                            // ESCAPE_FAILED handling, possibly re-entered.
                            let mut escape_failed = errorcode != 0;
                            loop {
                                if escape_failed {
                                    if (xoptions & PCRE2_EXTRA_BAD_ESCAPE_IS_LITERAL) == 0 {
                                        break 'failed;
                                    }
                                    ptr = tempptr;
                                    if ptr >= ptrend {
                                        c = b'\\' as u32;
                                    } else {
                                        c = getcharinctest!();
                                    }
                                    escape = 0;
                                    escape_failed = false;
                                }
                                if escape == 0 {
                                    parsed_literal!(c);
                                } else if escape < 0 {
                                    offset = ptr - 1;
                                    let e = (-escape) as u32;
                                    push!(META_BACKREF | e);
                                    if e < 10 {
                                        if self.small_ref_offset[e as usize] == PCRE2_UNSET {
                                            self.small_ref_offset[e as usize] = offset;
                                        }
                                    } else {
                                        self.put_offset(&mut pp, offset);
                                    }
                                    okquantifier = true;
                                } else {
                                    let e = escape as u32;
                                    match e {
                                        ESC_C => {
                                            if (options & PCRE2_NEVER_BACKSLASH_C) != 0 {
                                                errorcode = ERR83;
                                                escape_failed = true;
                                                continue;
                                            }
                                            okquantifier = true;
                                            push!(META_ESCAPE + e);
                                        }
                                        ESC_ub => {
                                            push!(b'u' as u32);
                                            parsed_literal!(b'{' as u32);
                                        }
                                        ESC_X | ESC_H | ESC_h | ESC_N | ESC_R | ESC_V | ESC_v => {
                                            okquantifier = true;
                                            push!(META_ESCAPE + e);
                                        }
                                        ESC_d | ESC_D | ESC_s | ESC_S | ESC_w | ESC_W => {
                                            okquantifier = true;
                                            pp = self.handle_escdsw(e, pp, options, xoptions);
                                        }
                                        ESC_P | ESC_p => {
                                            let mut negated = false;
                                            let mut ptype: u16 = 0;
                                            let mut pdata: u16 = 0;
                                            if !get_ucp(
                                                self,
                                                &mut ptr,
                                                &mut negated,
                                                &mut ptype,
                                                &mut pdata,
                                                &mut errorcode,
                                            ) {
                                                escape_failed = true;
                                                continue;
                                            }
                                            let mut e2 = e;
                                            if negated {
                                                e2 = if e == ESC_P { ESC_p } else { ESC_P };
                                            }
                                            push!(META_ESCAPE + e2);
                                            push!(((ptype as u32) << 16) | pdata as u32);
                                            okquantifier = true;
                                        }
                                        ESC_g | ESC_k => {
                                            if ptr >= ptrend
                                                || (pat[ptr] != b'{'
                                                    && pat[ptr] != b'<'
                                                    && pat[ptr] != b'\'')
                                            {
                                                errorcode = if e == ESC_g { ERR57 } else { ERR69 };
                                                escape_failed = true;
                                                continue;
                                            }
                                            let terminator: u32 = if pat[ptr] == b'<' {
                                                b'>' as u32
                                            } else if pat[ptr] == b'\'' {
                                                b'\'' as u32
                                            } else {
                                                b'}' as u32
                                            };
                                            if e == ESC_g && terminator != b'}' as u32 {
                                                let mut p = ptr + 1;
                                                let mut i: i32 = 0;
                                                if read_number(
                                                    pat,
                                                    &mut p,
                                                    ptrend,
                                                    self.bracount as i32,
                                                    MAX_GROUP_NUMBER,
                                                    ERR61,
                                                    &mut i,
                                                    &mut errorcode,
                                                ) {
                                                    if p >= ptrend || pat[p] as u32 != terminator {
                                                        errorcode = ERR57;
                                                        escape_failed = true;
                                                        continue;
                                                    }
                                                    ptr = p;
                                                    // SET_RECURSION
                                                    push!(META_RECURSE | i as u32);
                                                    offset = ptr;
                                                    ptr += 1;
                                                    self.put_offset(&mut pp, offset);
                                                    okquantifier = true;
                                                    break;
                                                }
                                                if errorcode != 0 {
                                                    escape_failed = true;
                                                    continue;
                                                }
                                            }
                                            if !read_name(
                                                self,
                                                &mut ptr,
                                                ptrend,
                                                utf,
                                                terminator,
                                                &mut offset,
                                                &mut name,
                                                &mut namelen,
                                                &mut errorcode,
                                            ) {
                                                escape_failed = true;
                                                continue;
                                            }
                                            push!(if e == ESC_k || terminator == b'}' as u32 {
                                                META_BACKREF_BYNAME
                                            } else {
                                                META_RECURSE_BYNAME
                                            });
                                            push!(namelen);
                                            self.put_offset(&mut pp, offset);
                                            okquantifier = true;
                                        }
                                        _ => {
                                            push!(META_ESCAPE + e);
                                        }
                                    }
                                }
                                break;
                            }
                        }

                        0x5e => push!(META_CIRCUMFLEX),
                        0x24 => push!(META_DOLLAR),
                        0x2e => {
                            push!(META_DOT);
                            okquantifier = true;
                        }
                        0x2a => {
                            meta_quantifier = META_ASTERISK;
                            check_quantifier = true;
                        }
                        0x2b => {
                            meta_quantifier = META_PLUS;
                            check_quantifier = true;
                        }
                        0x3f => {
                            meta_quantifier = META_QUERY;
                            check_quantifier = true;
                        }
                        0x7b => {
                            if !read_repeat_counts(
                                pat,
                                &mut ptr,
                                ptrend,
                                Some(&mut min_repeat),
                                Some(&mut max_repeat),
                                &mut errorcode,
                            ) {
                                if errorcode != 0 {
                                    break 'failed;
                                }
                                parsed_literal!(c);
                            } else {
                                meta_quantifier = META_MINMAX;
                                check_quantifier = true;
                            }
                        }

                        0x5b => {
                            // Character class.
                            okquantifier = true;
                            if ptrend - ptr >= 6
                                && (&pat[ptr..ptr + 6] == b"[:<:]]"
                                    || &pat[ptr..ptr + 6] == b"[:>:]]")
                            {
                                push!(META_ESCAPE + ESC_b);
                                if pat[ptr + 2] == b'<' {
                                    push!(META_LOOKAHEAD);
                                } else {
                                    push!(META_LOOKBEHIND);
                                    *has_lookbehind = true;
                                    self.put_offset(&mut pp, 0);
                                }
                                if (options & PCRE2_UCP) == 0 {
                                    push!(META_ESCAPE + ESC_w);
                                } else {
                                    push!(META_ESCAPE + ESC_p);
                                    push!(PT_WORD << 16);
                                }
                                push!(META_KET);
                                ptr += 6;
                                continue 'mainloop;
                            }

                            tempptr = 0;
                            if ptr < ptrend
                                && (pat[ptr] == b':' || pat[ptr] == b'.' || pat[ptr] == b'=')
                                && check_posix_syntax(pat, ptr, ptrend, &mut tempptr)
                            {
                                let ch = pat[ptr];
                                ptr -= 1;
                                errorcode = if ch == b':' { ERR12 } else { ERR13 };
                                break 'failed;
                            }

                            let mut negate_class = false;
                            while ptr < ptrend {
                                c = getcharinctest!();
                                if c == b'\\' as u32 {
                                    if ptr < ptrend && pat[ptr] == b'E' {
                                        ptr += 1;
                                    } else if ptrend - ptr >= 3 && &pat[ptr..ptr + 3] == b"Q\\E" {
                                        ptr += 3;
                                    } else {
                                        break;
                                    }
                                } else if (options & PCRE2_EXTENDED_MORE) != 0
                                    && (c == b' ' as u32 || c == b'\t' as u32)
                                {
                                    continue;
                                } else if !negate_class && c == b'^' as u32 {
                                    negate_class = true;
                                } else {
                                    break;
                                }
                            }

                            if c == b']' as u32
                                && (self.external_options & PCRE2_ALLOW_EMPTY_CLASS) != 0
                            {
                                push!(if negate_class {
                                    META_CLASS_EMPTY_NOT
                                } else {
                                    META_CLASS_EMPTY
                                });
                                continue 'mainloop;
                            }

                            push!(if negate_class {
                                META_CLASS_NOT
                            } else {
                                META_CLASS
                            });
                            class_range_state = RANGE_NO;

                            loop {
                                let mut char_is_literal = true;
                                // Each iteration processes `c`; the labels
                                // CLASS_LITERAL / CLASS_CONTINUE are modelled
                                // with flags.
                                let mut goto_literal = false;
                                'item: {
                                    if inescq {
                                        if c == b'\\' as u32 && ptr < ptrend && pat[ptr] == b'E' {
                                            inescq = false;
                                            ptr += 1;
                                            break 'item;
                                        }
                                        goto_literal = true;
                                    } else if (options & PCRE2_EXTENDED_MORE) != 0
                                        && (c == b' ' as u32 || c == b'\t' as u32)
                                    {
                                        break 'item;
                                    } else if c == b'[' as u32
                                        && ptrend - ptr >= 3
                                        && (pat[ptr] == b':'
                                            || pat[ptr] == b'.'
                                            || pat[ptr] == b'=')
                                        && check_posix_syntax(pat, ptr, ptrend, &mut tempptr)
                                    {
                                        let mut posix_negate = false;
                                        if class_range_state == RANGE_STARTED {
                                            errorcode = ERR50;
                                            break 'failed;
                                        }
                                        if pat[ptr] != b':' {
                                            errorcode = ERR13;
                                            ptr -= 1;
                                            break 'failed;
                                        }
                                        ptr += 1;
                                        if pat[ptr] == b'^' {
                                            posix_negate = true;
                                            ptr += 1;
                                        }
                                        let posix_class = check_posix_name(pat, ptr, tempptr - ptr);
                                        if posix_class < 0 {
                                            errorcode = ERR30;
                                            break 'failed;
                                        }
                                        ptr = tempptr + 2;
                                        if ptr + 1 < ptrend
                                            && pat[ptr] == b'-'
                                            && pat[ptr + 1] != b']'
                                        {
                                            errorcode = ERR50;
                                            break 'failed;
                                        }
                                        class_range_state = RANGE_NO;
                                        if (options & PCRE2_UCP) != 0
                                            && (xoptions & PCRE2_EXTRA_ASCII_POSIX) == 0
                                            && !((xoptions & PCRE2_EXTRA_ASCII_DIGIT) != 0
                                                && (posix_class == PC_DIGIT
                                                    || posix_class == PC_XDIGIT))
                                        {
                                            let ptype = POSIX_SUBSTITUTES[2 * posix_class as usize];
                                            let pvalue =
                                                POSIX_SUBSTITUTES[2 * posix_class as usize + 1];
                                            if ptype >= 0 {
                                                push!(
                                                    META_ESCAPE
                                                        + if posix_negate { ESC_P } else { ESC_p }
                                                );
                                                push!(((ptype as u32) << 16) | pvalue as u32);
                                                break 'item;
                                            }
                                            if pvalue != 0 {
                                                push!(
                                                    META_ESCAPE
                                                        + if posix_negate { ESC_H } else { ESC_h }
                                                );
                                                break 'item;
                                            }
                                        }
                                        push!(if posix_negate {
                                            META_POSIX_NEG
                                        } else {
                                            META_POSIX
                                        });
                                        push!(posix_class as u32);
                                    } else if c == b'-' as u32
                                        && class_range_state >= RANGE_OK_ESCAPED
                                    {
                                        push!(if class_range_state == RANGE_OK_LITERAL {
                                            META_RANGE_LITERAL
                                        } else {
                                            META_RANGE_ESCAPED
                                        });
                                        class_range_state = RANGE_STARTED;
                                    } else if c != b'\\' as u32 {
                                        goto_literal = true;
                                    } else {
                                        tempptr = ptr;
                                        let mut escape = check_escape(
                                            pat,
                                            &mut ptr,
                                            ptrend,
                                            &mut c,
                                            &mut errorcode,
                                            options,
                                            xoptions,
                                            true,
                                            self,
                                        );
                                        if errorcode != 0 {
                                            if (xoptions & PCRE2_EXTRA_BAD_ESCAPE_IS_LITERAL) == 0 {
                                                break 'failed;
                                            }
                                            ptr = tempptr;
                                            if ptr >= ptrend {
                                                c = b'\\' as u32;
                                            } else {
                                                c = getcharinctest!();
                                            }
                                            escape = 0;
                                        }
                                        let e = escape as u32;
                                        if escape == 0 {
                                            char_is_literal = false;
                                            goto_literal = true;
                                        } else if e == ESC_b {
                                            c = CHAR_BS;
                                            char_is_literal = false;
                                            goto_literal = true;
                                        } else if e == ESC_Q {
                                            inescq = true;
                                            break 'item;
                                        } else if e == ESC_E {
                                            break 'item;
                                        } else if e == ESC_B || e == ESC_R || e == ESC_X {
                                            errorcode = ERR7;
                                            ptr -= 1;
                                            break 'failed;
                                        } else {
                                            if class_range_state == RANGE_STARTED {
                                                errorcode = ERR50;
                                                break 'failed;
                                            }
                                            class_range_state = RANGE_NO;
                                            match e {
                                                ESC_N => {
                                                    errorcode = ERR71;
                                                    break 'failed;
                                                }
                                                ESC_H | ESC_h | ESC_V | ESC_v => {
                                                    push!(META_ESCAPE + e)
                                                }
                                                ESC_d | ESC_D | ESC_s | ESC_S | ESC_w | ESC_W => {
                                                    pp = self
                                                        .handle_escdsw(e, pp, options, xoptions);
                                                }
                                                ESC_P | ESC_p => {
                                                    let mut negated = false;
                                                    let mut ptype: u16 = 0;
                                                    let mut pdata: u16 = 0;
                                                    if !get_ucp(
                                                        self,
                                                        &mut ptr,
                                                        &mut negated,
                                                        &mut ptype,
                                                        &mut pdata,
                                                        &mut errorcode,
                                                    ) {
                                                        break 'failed;
                                                    }
                                                    let mut e2 = e;
                                                    if negated {
                                                        e2 = if e == ESC_P { ESC_p } else { ESC_P };
                                                    }
                                                    push!(META_ESCAPE + e2);
                                                    push!(((ptype as u32) << 16) | pdata as u32);
                                                }
                                                _ => {
                                                    errorcode = ERR7;
                                                    ptr -= 1;
                                                    break 'failed;
                                                }
                                            }
                                            if ptr + 1 < ptrend
                                                && pat[ptr] == b'-'
                                                && pat[ptr + 1] != b']'
                                            {
                                                errorcode = ERR50;
                                                break 'failed;
                                            }
                                        }
                                    }

                                    if goto_literal {
                                        // CLASS_LITERAL
                                        if class_range_state == RANGE_STARTED {
                                            if c == self.pr(pp - 2) {
                                                pp -= 1;
                                            } else if self.pr(pp - 2) > c {
                                                errorcode = ERR8;
                                                ptr -= 1;
                                                break 'failed;
                                            } else {
                                                if !char_is_literal
                                                    && self.pr(pp - 1) == META_RANGE_LITERAL
                                                {
                                                    self.pw(pp - 1, META_RANGE_ESCAPED);
                                                }
                                                parsed_literal!(c);
                                            }
                                            class_range_state = RANGE_NO;
                                        } else {
                                            class_range_state = if char_is_literal {
                                                RANGE_OK_LITERAL
                                            } else {
                                                RANGE_OK_ESCAPED
                                            };
                                            parsed_literal!(c);
                                        }
                                    }
                                }
                                // CLASS_CONTINUE
                                if ptr >= ptrend {
                                    errorcode = ERR6;
                                    break 'failed;
                                }
                                c = getcharinctest!();
                                if c == b']' as u32 && !inescq {
                                    break;
                                }
                            }

                            if class_range_state == RANGE_STARTED {
                                self.pw(pp - 1, b'-' as u32);
                            }
                            push!(META_CLASS_END);
                        }

                        0x28 => {
                            // '(' - the many kinds of group.
                            if ptr >= ptrend {
                                errorcode = ERR14;
                                break 'failed;
                            }

                            #[derive(Clone, Copy, PartialEq)]
                            enum T {
                                None,
                                Atomic,
                                PosLookahead,
                                PosNaLookahead,
                                NegLookahead,
                                PostLookbehind,
                                PostAssertion,
                                RecursionByNumber,
                                SetRecursion(i32),
                                RecurseByName,
                                DefineName(u32),
                            }
                            let mut target = T::None;

                            if pat[ptr] != b'?' {
                                if pat[ptr] != b'*' {
                                    nest_depth += 1;
                                    if (options & PCRE2_NO_AUTO_CAPTURE) == 0 {
                                        if self.bracount >= MAX_GROUP_NUMBER {
                                            errorcode = ERR97;
                                            break 'failed;
                                        }
                                        self.bracount += 1;
                                        push!(META_CAPTURE | self.bracount);
                                    } else {
                                        push!(META_NOCAPTURE);
                                    }
                                } else if ptrend - ptr <= 1 || {
                                    c = pat[ptr + 1] as u32;
                                    c == b')' as u32
                                } {
                                    // (*) is a literal-ish error-free item: ignore
                                } else if (self.ctypes(c) & ctype_lcletter) != 0 {
                                    // Alpha assertion.
                                    let mut offset2 = 0;
                                    if !read_name(
                                        self,
                                        &mut ptr,
                                        ptrend,
                                        utf,
                                        0,
                                        &mut offset2,
                                        &mut name,
                                        &mut namelen,
                                        &mut errorcode,
                                    ) {
                                        break 'failed;
                                    }
                                    if ptr >= ptrend || pat[ptr] != b':' {
                                        errorcode = ERR95;
                                        break 'failed;
                                    }
                                    let nm = &pat[name..name + namelen as usize];
                                    let found = ALASNAMES.iter().find(|(n, _)| *n == nm);
                                    let meta = match found {
                                        Some((_, m)) => *m,
                                        None => {
                                            errorcode = ERR95;
                                            break 'failed;
                                        }
                                    };
                                    if prev_expect_cond_assert > 0
                                        && !(META_LOOKAHEAD..=META_LOOKBEHINDNOT).contains(&meta)
                                    {
                                        errorcode = if meta == META_LOOKAHEAD_NA
                                            || meta == META_LOOKBEHIND_NA
                                        {
                                            ERR98
                                        } else {
                                            ERR28
                                        };
                                        break 'failed;
                                    }
                                    match meta {
                                        META_ATOMIC => target = T::Atomic,
                                        META_LOOKAHEAD => target = T::PosLookahead,
                                        META_LOOKAHEAD_NA => target = T::PosNaLookahead,
                                        META_LOOKAHEADNOT => target = T::NegLookahead,
                                        META_LOOKBEHIND | META_LOOKBEHINDNOT
                                        | META_LOOKBEHIND_NA => {
                                            push!(meta);
                                            ptr -= 1;
                                            target = T::PostLookbehind;
                                        }
                                        META_SCRIPT_RUN | META_ATOMIC_SCRIPT_RUN => {
                                            push!(META_SCRIPT_RUN);
                                            nest_depth += 1;
                                            ptr += 1;
                                            if meta == META_ATOMIC_SCRIPT_RUN {
                                                push!(META_ATOMIC);
                                                if nests.len() >= max_nests {
                                                    errorcode = ERR84;
                                                    break 'failed;
                                                }
                                                nests.push(NestSave {
                                                    nest_depth,
                                                    flags: NSF_ATOMICSR,
                                                    options: options & PARSE_TRACKED_OPTIONS,
                                                    xoptions: xoptions
                                                        & PARSE_TRACKED_EXTRA_OPTIONS,
                                                    ..Default::default()
                                                });
                                            }
                                        }
                                        _ => {
                                            errorcode = ERR89;
                                            break 'failed;
                                        }
                                    }
                                } else {
                                    // Backtracking control verb.
                                    let mut offset2 = 0;
                                    if !read_name(
                                        self,
                                        &mut ptr,
                                        ptrend,
                                        utf,
                                        0,
                                        &mut offset2,
                                        &mut name,
                                        &mut namelen,
                                        &mut errorcode,
                                    ) {
                                        break 'failed;
                                    }
                                    if ptr >= ptrend || (pat[ptr] != b':' && pat[ptr] != b')') {
                                        errorcode = ERR60;
                                        break 'failed;
                                    }
                                    let nm = &pat[name..name + namelen as usize];
                                    let mut vi = usize::MAX;
                                    for (i, v) in VERBS.iter().enumerate() {
                                        if namelen as usize == v.len && VERBNAMES[i] == nm {
                                            vi = i;
                                            break;
                                        }
                                    }
                                    if vi == usize::MAX {
                                        errorcode = ERR60;
                                        break 'failed;
                                    }
                                    // Empty argument is treated as no argument.
                                    if pat[ptr] == b':' && ptr + 1 < ptrend && pat[ptr + 1] == b')'
                                    {
                                        ptr += 1;
                                    }
                                    if VERBS[vi].has_arg > 0 && pat[ptr] != b':' {
                                        errorcode = ERR66;
                                        break 'failed;
                                    }
                                    verbstartptr = pp;
                                    okquantifier = VERBS[vi].meta == META_ACCEPT;
                                    let ch = pat[ptr];
                                    ptr += 1;
                                    if ch == b':' {
                                        if VERBS[vi].has_arg < 0 {
                                            add_after_mark = VERBS[vi].meta;
                                            push!(META_MARK);
                                        } else {
                                            push!(
                                                VERBS[vi].meta
                                                    + if VERBS[vi].meta != META_MARK {
                                                        0x00010000
                                                    } else {
                                                        0
                                                    }
                                            );
                                        }
                                        verblengthptr = pp;
                                        pp += 1;
                                        verbnamestart = ptr;
                                        inverbname = true;
                                    } else {
                                        push!(VERBS[vi].meta);
                                    }
                                }
                                if target == T::None {
                                    continue 'mainloop;
                                }
                            } else {
                                // Handle (? groups.
                                ptr += 1;
                                if ptr >= ptrend {
                                    errorcode = ERR14;
                                    break 'failed;
                                }
                                match pat[ptr] {
                                    b'P' => {
                                        ptr += 1;
                                        if ptr >= ptrend {
                                            errorcode = ERR14;
                                            break 'failed;
                                        }
                                        if pat[ptr] == b'<' {
                                            target = T::DefineName(b'>' as u32);
                                        } else if pat[ptr] == b'>' {
                                            target = T::RecurseByName;
                                        } else if pat[ptr] != b'=' {
                                            errorcode = ERR41;
                                            break 'failed;
                                        } else {
                                            if !read_name(
                                                self,
                                                &mut ptr,
                                                ptrend,
                                                utf,
                                                b')' as u32,
                                                &mut offset,
                                                &mut name,
                                                &mut namelen,
                                                &mut errorcode,
                                            ) {
                                                break 'failed;
                                            }
                                            push!(META_BACKREF_BYNAME);
                                            push!(namelen);
                                            self.put_offset(&mut pp, offset);
                                            okquantifier = true;
                                            continue 'mainloop;
                                        }
                                    }
                                    b'R' => {
                                        ptr += 1;
                                        if ptr >= ptrend || pat[ptr] != b')' {
                                            errorcode = ERR58;
                                            break 'failed;
                                        }
                                        target = T::SetRecursion(0);
                                    }
                                    b'+' => {
                                        if ptrend - ptr < 2 || !is_digit(pat[ptr + 1]) {
                                            errorcode = ERR29;
                                            break 'failed;
                                        }
                                        target = T::RecursionByNumber;
                                    }
                                    b'0'..=b'9' => target = T::RecursionByNumber,
                                    b'&' => target = T::RecurseByName,
                                    b'C' => {
                                        // Callout.
                                        ptr += 1;
                                        if ptr >= ptrend {
                                            errorcode = ERR14;
                                            break 'failed;
                                        }
                                        expect_cond_assert = prev_expect_cond_assert - 1;
                                        if let Some(pc) = previous_callout {
                                            if (options & PCRE2_AUTO_CALLOUT) != 0
                                                && Some(pc) == pp.checked_sub(4)
                                                && self.pr(pp - 1) == 255
                                            {
                                                pp = pc;
                                            }
                                        }
                                        previous_callout = Some(pp);
                                        after_manual_callout = 1;
                                        if pat[ptr] != b')' && !is_digit(pat[ptr]) {
                                            let startptr = ptr;
                                            let delimiter = match pat[ptr] {
                                                b'`' => b'`',
                                                b'\'' => b'\'',
                                                b'"' => b'"',
                                                b'^' => b'^',
                                                b'%' => b'%',
                                                b'#' => b'#',
                                                b'$' => b'$',
                                                b'{' => b'}',
                                                _ => 0,
                                            };
                                            if delimiter == 0 {
                                                errorcode = ERR82;
                                                break 'failed;
                                            }
                                            self.pw(pp, META_CALLOUT_STRING);
                                            pp += 3;
                                            loop {
                                                ptr += 1;
                                                if ptr >= ptrend {
                                                    errorcode = ERR81;
                                                    ptr = startptr;
                                                    break 'failed;
                                                }
                                                if pat[ptr] == delimiter && {
                                                    ptr += 1;
                                                    ptr >= ptrend || pat[ptr] != delimiter
                                                } {
                                                    break;
                                                }
                                            }
                                            let calloutlength = ptr - startptr;
                                            push!(calloutlength as u32);
                                            self.put_offset(&mut pp, startptr);
                                        } else {
                                            let mut n: i32 = 0;
                                            self.pw(pp, META_CALLOUT_NUMBER);
                                            pp += 3;
                                            while ptr < ptrend && is_digit(pat[ptr]) {
                                                n = n * 10 + (pat[ptr] - b'0') as i32;
                                                ptr += 1;
                                                if n > 255 {
                                                    errorcode = ERR38;
                                                    break 'failed;
                                                }
                                            }
                                            push!(n as u32);
                                        }
                                        if ptr >= ptrend || pat[ptr] != b')' {
                                            errorcode = ERR39;
                                            break 'failed;
                                        }
                                        ptr += 1;
                                        let pc = previous_callout.unwrap();
                                        self.pw(pc + 1, ptr as u32);
                                        self.pw(pc + 2, 0);
                                        continue 'mainloop;
                                    }
                                    b'(' => {
                                        // Conditional group.
                                        ptr += 1;
                                        if ptr >= ptrend {
                                            errorcode = ERR14;
                                            break 'failed;
                                        }
                                        nest_depth += 1;
                                        if pat[ptr] == b'?' || pat[ptr] == b'*' {
                                            push!(META_COND_ASSERT);
                                            ptr -= 1;
                                            expect_cond_assert = 2;
                                            continue 'mainloop;
                                        }
                                        let mut i: i32 = 0;
                                        if read_number(
                                            pat,
                                            &mut ptr,
                                            ptrend,
                                            self.bracount as i32,
                                            MAX_GROUP_NUMBER,
                                            ERR61,
                                            &mut i,
                                            &mut errorcode,
                                        ) {
                                            if i <= 0 {
                                                errorcode = ERR15;
                                                break 'failed;
                                            }
                                            push!(META_COND_NUMBER);
                                            offset = ptr - 2;
                                            self.put_offset(&mut pp, offset);
                                            push!(i as u32);
                                        } else if errorcode != 0 {
                                            break 'failed;
                                        } else if ptrend - ptr >= 10
                                            && &pat[ptr..ptr + 7] == b"VERSION"
                                            && pat[ptr + 7] != b')'
                                        {
                                            let mut ge = 0u32;
                                            let mut major: i32 = 0;
                                            let mut minor: i32 = 0;
                                            ptr += 7;
                                            if pat[ptr] == b'>' {
                                                ge = 1;
                                                ptr += 1;
                                            }
                                            let bad = 'bv: {
                                                if pat[ptr] != b'=' || {
                                                    ptr += 1;
                                                    !is_digit(pat[ptr])
                                                } {
                                                    break 'bv true;
                                                }
                                                if !read_number(
                                                    pat,
                                                    &mut ptr,
                                                    ptrend,
                                                    -1,
                                                    1000,
                                                    ERR79,
                                                    &mut major,
                                                    &mut errorcode,
                                                ) {
                                                    break 'failed;
                                                }
                                                if ptr >= ptrend {
                                                    break 'bv true;
                                                }
                                                if pat[ptr] == b'.' {
                                                    ptr += 1;
                                                    if ptr >= ptrend || !is_digit(pat[ptr]) {
                                                        break 'bv true;
                                                    }
                                                    minor = ((pat[ptr] - b'0') * 10) as i32;
                                                    ptr += 1;
                                                    if ptr >= ptrend {
                                                        break 'bv true;
                                                    }
                                                    if is_digit(pat[ptr]) {
                                                        minor += (pat[ptr] - b'0') as i32;
                                                        ptr += 1;
                                                    }
                                                    if ptr >= ptrend || pat[ptr] != b')' {
                                                        break 'bv true;
                                                    }
                                                }
                                                false
                                            };
                                            if bad {
                                                errorcode = ERR79;
                                                break 'failed;
                                            }
                                            push!(META_COND_VERSION);
                                            push!(ge);
                                            push!(major as u32);
                                            push!(minor as u32);
                                        } else {
                                            let mut was_r_ampersand = false;
                                            let terminator: u32;
                                            if pat[ptr] == b'R'
                                                && ptrend - ptr > 1
                                                && pat[ptr + 1] == b'&'
                                            {
                                                terminator = b')' as u32;
                                                was_r_ampersand = true;
                                                ptr += 1;
                                            } else if pat[ptr] == b'<' {
                                                terminator = b'>' as u32;
                                            } else if pat[ptr] == b'\'' {
                                                terminator = b'\'' as u32;
                                            } else {
                                                terminator = b')' as u32;
                                                ptr -= 1;
                                            }
                                            if !read_name(
                                                self,
                                                &mut ptr,
                                                ptrend,
                                                utf,
                                                terminator,
                                                &mut offset,
                                                &mut name,
                                                &mut namelen,
                                                &mut errorcode,
                                            ) {
                                                break 'failed;
                                            }
                                            let item;
                                            if was_r_ampersand {
                                                item = META_COND_RNAME;
                                                ptr -= 1;
                                            } else if terminator == b')' as u32 {
                                                let nm = &pat[name..name + namelen as usize];
                                                if namelen == 6 && nm == b"DEFINE" {
                                                    item = META_COND_DEFINE;
                                                } else {
                                                    let mut i = 1usize;
                                                    while i < namelen as usize {
                                                        if !is_digit(nm[i]) {
                                                            break;
                                                        }
                                                        i += 1;
                                                    }
                                                    item = if nm[0] == b'R' && i >= namelen as usize
                                                    {
                                                        META_COND_RNUMBER
                                                    } else {
                                                        META_COND_NAME
                                                    };
                                                }
                                                ptr -= 1;
                                            } else {
                                                item = META_COND_NAME;
                                            }
                                            push!(item);
                                            if item != META_COND_DEFINE {
                                                push!(namelen);
                                            }
                                            self.put_offset(&mut pp, offset);
                                        }
                                        if ptr >= ptrend || pat[ptr] != b')' {
                                            errorcode = ERR24;
                                            break 'failed;
                                        }
                                        ptr += 1;
                                        continue 'mainloop;
                                    }
                                    b'>' => target = T::Atomic,
                                    b'=' => target = T::PosLookahead,
                                    b'*' => target = T::PosNaLookahead,
                                    b'!' => target = T::NegLookahead,
                                    b'<' => {
                                        if ptrend - ptr <= 1
                                            || (pat[ptr + 1] != b'='
                                                && pat[ptr + 1] != b'!'
                                                && pat[ptr + 1] != b'*')
                                        {
                                            target = T::DefineName(b'>' as u32);
                                        } else {
                                            push!(if pat[ptr + 1] == b'=' {
                                                META_LOOKBEHIND
                                            } else if pat[ptr + 1] == b'!' {
                                                META_LOOKBEHINDNOT
                                            } else {
                                                META_LOOKBEHIND_NA
                                            });
                                            target = T::PostLookbehind;
                                        }
                                    }
                                    b'\'' => target = T::DefineName(b'\'' as u32),
                                    _ => {
                                        if pat[ptr] == b'-'
                                            && ptrend - ptr > 1
                                            && is_digit(pat[ptr + 1])
                                        {
                                            target = T::RecursionByNumber;
                                        } else {
                                            // Option settings and non-capture groups.
                                            nest_depth += 1;
                                            if nests.len() >= max_nests {
                                                errorcode = ERR84;
                                                break 'failed;
                                            }
                                            nests.push(NestSave {
                                                nest_depth,
                                                flags: 0,
                                                options: options & PARSE_TRACKED_OPTIONS,
                                                xoptions: xoptions & PARSE_TRACKED_EXTRA_OPTIONS,
                                                ..Default::default()
                                            });
                                            if pat[ptr] == b'|' {
                                                let top = nests.last_mut().unwrap();
                                                top.reset_group = self.bracount as u16;
                                                top.max_group = self.bracount as u16;
                                                top.flags |= NSF_RESET;
                                                self.external_flags |= PCRE2_DUPCAPUSED;
                                                push!(META_NOCAPTURE);
                                                ptr += 1;
                                            } else {
                                                let mut hyphenok = true;
                                                let oldoptions = options;
                                                let oldxoptions = xoptions;
                                                let mut set: u32 = 0;
                                                let mut unset: u32 = 0;
                                                let mut xset: u32 = 0;
                                                let mut xunset: u32 = 0;
                                                let mut use_unset = false;
                                                if ptr < ptrend && pat[ptr] == b'^' {
                                                    options &= !(PCRE2_CASELESS
                                                        | PCRE2_MULTILINE
                                                        | PCRE2_NO_AUTO_CAPTURE
                                                        | PCRE2_DOTALL
                                                        | PCRE2_EXTENDED
                                                        | PCRE2_EXTENDED_MORE);
                                                    xoptions &= !PCRE2_EXTRA_CASELESS_RESTRICT;
                                                    hyphenok = false;
                                                    ptr += 1;
                                                }
                                                while ptr < ptrend
                                                    && pat[ptr] != b')'
                                                    && pat[ptr] != b':'
                                                {
                                                    let ch = pat[ptr];
                                                    ptr += 1;
                                                    let (optset, xoptset) = if use_unset {
                                                        (&mut unset, &mut xunset)
                                                    } else {
                                                        (&mut set, &mut xset)
                                                    };
                                                    match ch {
                                                        b'-' => {
                                                            if !hyphenok {
                                                                errorcode = ERR94;
                                                                ptr -= 1;
                                                                break 'failed;
                                                            }
                                                            use_unset = true;
                                                            hyphenok = false;
                                                        }
                                                        b'a' => {
                                                            if ptr < ptrend {
                                                                match pat[ptr] {
                                                                    b'D' => {
                                                                        *xoptset |=
                                                                            PCRE2_EXTRA_ASCII_BSD;
                                                                        ptr += 1;
                                                                        continue;
                                                                    }
                                                                    b'P' => {
                                                                        *xoptset |= PCRE2_EXTRA_ASCII_POSIX
                                                                            | PCRE2_EXTRA_ASCII_DIGIT;
                                                                        ptr += 1;
                                                                        continue;
                                                                    }
                                                                    b'S' => {
                                                                        *xoptset |=
                                                                            PCRE2_EXTRA_ASCII_BSS;
                                                                        ptr += 1;
                                                                        continue;
                                                                    }
                                                                    b'T' => {
                                                                        *xoptset |=
                                                                            PCRE2_EXTRA_ASCII_DIGIT;
                                                                        ptr += 1;
                                                                        continue;
                                                                    }
                                                                    b'W' => {
                                                                        *xoptset |=
                                                                            PCRE2_EXTRA_ASCII_BSW;
                                                                        ptr += 1;
                                                                        continue;
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                            *xoptset |= PCRE2_EXTRA_ASCII_BSD
                                                                | PCRE2_EXTRA_ASCII_BSS
                                                                | PCRE2_EXTRA_ASCII_BSW
                                                                | PCRE2_EXTRA_ASCII_DIGIT
                                                                | PCRE2_EXTRA_ASCII_POSIX;
                                                        }
                                                        b'J' => {
                                                            *optset |= PCRE2_DUPNAMES;
                                                            self.external_flags |= PCRE2_JCHANGED;
                                                        }
                                                        b'i' => *optset |= PCRE2_CASELESS,
                                                        b'm' => *optset |= PCRE2_MULTILINE,
                                                        b'n' => *optset |= PCRE2_NO_AUTO_CAPTURE,
                                                        b'r' => {
                                                            *xoptset |=
                                                                PCRE2_EXTRA_CASELESS_RESTRICT
                                                        }
                                                        b's' => *optset |= PCRE2_DOTALL,
                                                        b'U' => *optset |= PCRE2_UNGREEDY,
                                                        b'x' => {
                                                            *optset |= PCRE2_EXTENDED;
                                                            if ptr < ptrend && pat[ptr] == b'x' {
                                                                *optset |= PCRE2_EXTENDED_MORE;
                                                                ptr += 1;
                                                            }
                                                        }
                                                        _ => {
                                                            errorcode = ERR11;
                                                            ptr -= 1;
                                                            break 'failed;
                                                        }
                                                    }
                                                }
                                                if (set & (PCRE2_EXTENDED | PCRE2_EXTENDED_MORE))
                                                    == PCRE2_EXTENDED
                                                    || (unset & PCRE2_EXTENDED) != 0
                                                {
                                                    unset |= PCRE2_EXTENDED_MORE;
                                                }
                                                options = (options | set) & !unset;
                                                xoptions = (xoptions | xset) & !xunset;
                                                if ptr >= ptrend {
                                                    errorcode = ERR14;
                                                    break 'failed;
                                                }
                                                let ch = pat[ptr];
                                                ptr += 1;
                                                if ch == b')' {
                                                    nest_depth -= 1;
                                                    let n = nests.len();
                                                    if n > 1
                                                        && nests[n - 2].nest_depth == nest_depth
                                                    {
                                                        nests.pop();
                                                    } else {
                                                        nests[n - 1].nest_depth = nest_depth;
                                                    }
                                                } else {
                                                    push!(META_NOCAPTURE);
                                                }
                                                if options != oldoptions || xoptions != oldxoptions
                                                {
                                                    push!(META_OPTIONS);
                                                    push!(options);
                                                    push!(xoptions);
                                                }
                                            }
                                            continue 'mainloop;
                                        }
                                    }
                                }
                            }

                            // Execute the jump targets.
                            loop {
                                match target {
                                    T::None => break,
                                    T::RecursionByNumber => {
                                        let mut i: i32 = 0;
                                        let allow = if is_digit(pat[ptr]) {
                                            -1
                                        } else {
                                            self.bracount as i32
                                        };
                                        if !read_number(
                                            pat,
                                            &mut ptr,
                                            ptrend,
                                            allow,
                                            MAX_GROUP_NUMBER,
                                            ERR61,
                                            &mut i,
                                            &mut errorcode,
                                        ) {
                                            break 'failed;
                                        }
                                        if i < 0 {
                                            errorcode = ERR15;
                                            ptr -= 1;
                                            break 'failed;
                                        }
                                        if ptr >= ptrend || pat[ptr] != b')' {
                                            errorcode = ERR14;
                                            break 'failed;
                                        }
                                        target = T::SetRecursion(i);
                                    }
                                    T::SetRecursion(i) => {
                                        push!(META_RECURSE | i as u32);
                                        offset = ptr;
                                        ptr += 1;
                                        self.put_offset(&mut pp, offset);
                                        okquantifier = true;
                                        break;
                                    }
                                    T::RecurseByName => {
                                        if !read_name(
                                            self,
                                            &mut ptr,
                                            ptrend,
                                            utf,
                                            b')' as u32,
                                            &mut offset,
                                            &mut name,
                                            &mut namelen,
                                            &mut errorcode,
                                        ) {
                                            break 'failed;
                                        }
                                        push!(META_RECURSE_BYNAME);
                                        push!(namelen);
                                        self.put_offset(&mut pp, offset);
                                        okquantifier = true;
                                        break;
                                    }
                                    T::Atomic => {
                                        push!(META_ATOMIC);
                                        nest_depth += 1;
                                        ptr += 1;
                                        break;
                                    }
                                    T::PosLookahead => {
                                        push!(META_LOOKAHEAD);
                                        ptr += 1;
                                        target = T::PostAssertion;
                                    }
                                    T::PosNaLookahead => {
                                        push!(META_LOOKAHEAD_NA);
                                        ptr += 1;
                                        target = T::PostAssertion;
                                    }
                                    T::NegLookahead => {
                                        push!(META_LOOKAHEADNOT);
                                        ptr += 1;
                                        target = T::PostAssertion;
                                    }
                                    T::PostLookbehind => {
                                        *has_lookbehind = true;
                                        offset = ptr - 2;
                                        self.put_offset(&mut pp, offset);
                                        ptr += 2;
                                        target = T::PostAssertion;
                                    }
                                    T::PostAssertion => {
                                        nest_depth += 1;
                                        if prev_expect_cond_assert > 0 {
                                            if nests.len() >= max_nests {
                                                errorcode = ERR84;
                                                break 'failed;
                                            }
                                            nests.push(NestSave {
                                                nest_depth,
                                                flags: NSF_CONDASSERT,
                                                options: options & PARSE_TRACKED_OPTIONS,
                                                xoptions: xoptions & PARSE_TRACKED_EXTRA_OPTIONS,
                                                ..Default::default()
                                            });
                                        }
                                        break;
                                    }
                                    T::DefineName(terminator) => {
                                        if !read_name(
                                            self,
                                            &mut ptr,
                                            ptrend,
                                            utf,
                                            terminator,
                                            &mut offset,
                                            &mut name,
                                            &mut namelen,
                                            &mut errorcode,
                                        ) {
                                            break 'failed;
                                        }
                                        if self.bracount >= MAX_GROUP_NUMBER {
                                            errorcode = ERR97;
                                            break 'failed;
                                        }
                                        self.bracount += 1;
                                        push!(META_CAPTURE | self.bracount);
                                        nest_depth += 1;
                                        if self.names_found as usize >= MAX_NAME_COUNT {
                                            errorcode = ERR49;
                                            break 'failed;
                                        }
                                        if namelen + IMM2_SIZE as u32 + 1
                                            > self.name_entry_size as u32
                                        {
                                            self.name_entry_size =
                                                (namelen + IMM2_SIZE as u32 + 1) as u16;
                                        }
                                        let mut isdupname = false;
                                        let nm = &pat[name..name + namelen as usize];
                                        let mut i = 0usize;
                                        while i < self.names_found as usize {
                                            let ng = self.named_groups[i];
                                            if namelen == ng.length as u32
                                                && &pat[ng.name..ng.name + namelen as usize] == nm
                                            {
                                                if ng.number == self.bracount {
                                                    break;
                                                }
                                                if (options & PCRE2_DUPNAMES) == 0 {
                                                    errorcode = ERR43;
                                                    break 'failed;
                                                }
                                                isdupname = true;
                                                self.named_groups[i].isdup = true;
                                                self.dupnames = true;
                                            } else if ng.number == self.bracount {
                                                errorcode = ERR65;
                                                break 'failed;
                                            }
                                            i += 1;
                                        }
                                        if i < self.names_found as usize {
                                            break;
                                        }
                                        self.named_groups.push(NamedGroup {
                                            name,
                                            length: namelen as u16,
                                            number: self.bracount,
                                            isdup: isdupname,
                                        });
                                        self.names_found += 1;
                                        break;
                                    }
                                }
                            }
                        }

                        0x7c => {
                            // '|'
                            if let Some(top) = nests.last_mut() {
                                if top.nest_depth == nest_depth && (top.flags & NSF_RESET) != 0 {
                                    if self.bracount > top.max_group as u32 {
                                        top.max_group = self.bracount as u16;
                                    }
                                    self.bracount = top.reset_group as u32;
                                }
                            }
                            push!(META_ALT);
                        }

                        0x29 => {
                            // ')'
                            okquantifier = true;
                            if let Some(top) = nests.last().copied() {
                                if top.nest_depth == nest_depth {
                                    options = (options & !PARSE_TRACKED_OPTIONS) | top.options;
                                    xoptions =
                                        (xoptions & !PARSE_TRACKED_EXTRA_OPTIONS) | top.xoptions;
                                    if (top.flags & NSF_RESET) != 0
                                        && top.max_group as u32 > self.bracount
                                    {
                                        self.bracount = top.max_group as u32;
                                    }
                                    if (top.flags & NSF_CONDASSERT) != 0 {
                                        okquantifier = false;
                                    }
                                    if (top.flags & NSF_ATOMICSR) != 0 {
                                        push!(META_KET);
                                    }
                                    nests.pop();
                                }
                            }
                            if nest_depth == 0 {
                                errorcode = ERR22;
                                ptr -= 1;
                                break 'failed;
                            }
                            nest_depth -= 1;
                            push!(META_KET);
                        }

                        _ => {
                            parsed_literal!(c);
                        }
                    }

                    if check_quantifier {
                        // CHECK_QUANTIFIER
                        if !prev_okquantifier {
                            errorcode = ERR9;
                            ptr -= 1;
                            break 'failed;
                        }
                        if prev_parsed_item != usize::MAX
                            && self.pr(prev_parsed_item) == META_ACCEPT
                        {
                            // Wrap a quantified (*ACCEPT) in a non-capturing group.
                            let mut p = pp as isize - 1;
                            while p >= verbstartptr as isize {
                                let v = self.pr(p as usize);
                                self.pw(p as usize + 1, v);
                                p -= 1;
                            }
                            self.pw(verbstartptr, META_NOCAPTURE);
                            self.pw(pp + 1, META_KET);
                            pp += 2;
                        }
                        push!(meta_quantifier);
                        if c == b'{' as u32 {
                            push!(min_repeat);
                            push!(max_repeat);
                        }
                    }
                }

                if inverbname && ptr >= ptrend {
                    errorcode = ERR60;
                    break 'failed;
                }
            }

            // PARSED_END
            pp = self.manage_callouts(ptr, &mut previous_callout, auto_callout, pp);
            if (xoptions & PCRE2_EXTRA_MATCH_LINE) != 0 {
                push!(META_KET);
                push!(META_DOLLAR);
            } else if (xoptions & PCRE2_EXTRA_MATCH_WORD) != 0 {
                push!(META_KET);
                push!(META_ESCAPE + ESC_b);
            }
            if pp >= parsed_pattern_end {
                errorcode = ERR63;
                break 'failed;
            }
            self.pw(pp, META_END);
            if nest_depth == 0 {
                return 0;
            }
            errorcode = ERR14;
        }
        // FAILED
        self.erroroffset = ptr;
        errorcode
    }
}
