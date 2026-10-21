//! Small shared helpers: UTF-8 handling (pcre2_ord2utf.c, pcre2_valid_utf.c),
//! newline checks (pcre2_newline.c), pcre2_find_bracket.c, pcre2_xclass.c,
//! pcre2_extuni.c and pcre2_script_run.c.

use crate::consts::*;
use crate::tables;

pub const UTF8_TABLE1: [u32; 6] = [0x7f, 0x7ff, 0xffff, 0x1fffff, 0x3ffffff, 0x7fffffff];
pub const UTF8_TABLE2: [u32; 6] = [0, 0xc0, 0xe0, 0xf0, 0xf8, 0xfc];

#[inline]
pub fn get_extralen(c: u8) -> usize {
    tables::UTF8_TABLE4[(c & 0x3f) as usize] as usize
}

#[inline]
pub fn has_extralen(c: u8) -> bool {
    c >= 0xc0
}

#[inline]
pub fn not_firstcu(c: u8) -> bool {
    (c & 0xc0) == 0x80
}

/// Encode a code point as UTF-8 into `buf`, returning the number of bytes.
pub fn ord2utf(cvalue: u32, buf: &mut [u8]) -> usize {
    let mut i = 0;
    while i < UTF8_TABLE1.len() {
        if cvalue <= UTF8_TABLE1[i] {
            break;
        }
        i += 1;
    }
    let mut c = cvalue;
    let mut j = i;
    while j > 0 {
        buf[j] = (0x80 | (c & 0x3f)) as u8;
        c >>= 6;
        j -= 1;
    }
    buf[0] = (UTF8_TABLE2[i] | c) as u8;
    i + 1
}

#[inline]
fn at(s: &[u8], i: usize) -> u32 {
    *s.get(i).unwrap_or(&0) as u32
}

/// GETUTF8 on a lead byte `c` already read at position `i`: returns the
/// decoded character (does not validate).
#[inline]
pub fn getutf8(s: &[u8], i: usize, c: u32) -> (u32, usize) {
    if (c & 0x20) == 0 {
        (((c & 0x1f) << 6) | (at(s, i + 1) & 0x3f), 2)
    } else if (c & 0x10) == 0 {
        (
            ((c & 0x0f) << 12) | ((at(s, i + 1) & 0x3f) << 6) | (at(s, i + 2) & 0x3f),
            3,
        )
    } else if (c & 0x08) == 0 {
        (
            ((c & 0x07) << 18)
                | ((at(s, i + 1) & 0x3f) << 12)
                | ((at(s, i + 2) & 0x3f) << 6)
                | (at(s, i + 3) & 0x3f),
            4,
        )
    } else if (c & 0x04) == 0 {
        (
            ((c & 0x03) << 24)
                | ((at(s, i + 1) & 0x3f) << 18)
                | ((at(s, i + 2) & 0x3f) << 12)
                | ((at(s, i + 3) & 0x3f) << 6)
                | (at(s, i + 4) & 0x3f),
            5,
        )
    } else {
        (
            ((c & 0x01) << 30)
                | ((at(s, i + 1) & 0x3f) << 24)
                | ((at(s, i + 2) & 0x3f) << 18)
                | ((at(s, i + 3) & 0x3f) << 12)
                | ((at(s, i + 4) & 0x3f) << 6)
                | (at(s, i + 5) & 0x3f),
            6,
        )
    }
}

/// GETCHAR / GETCHARLEN: character at `i` and its length in code units.
#[inline]
pub fn getchar(s: &[u8], i: usize) -> (u32, usize) {
    let c = at(s, i);
    if c >= 0xc0 { getutf8(s, i, c) } else { (c, 1) }
}

/// GETCHARTEST: as getchar, but only decodes when `utf` is set.
#[inline]
pub fn getchar_test(s: &[u8], i: usize, utf: bool) -> (u32, usize) {
    let c = at(s, i);
    if utf && c >= 0xc0 {
        getutf8(s, i, c)
    } else {
        (c, 1)
    }
}

/// BACKCHAR: move back to the start of the character containing `i`.
#[inline]
pub fn backchar(s: &[u8], mut i: usize) -> usize {
    while i > 0 && (at(s, i) & 0xc0) == 0x80 {
        i -= 1;
    }
    i
}

/// FORWARDCHARTEST
#[inline]
pub fn forwardchar_test(s: &[u8], mut i: usize, end: usize) -> usize {
    while i < end && (s[i] & 0xc0) == 0x80 {
        i += 1;
    }
    i
}

/// PRIV(is_newline): checks for a newline at `ptr`; `ptr` < `endptr`.
pub fn is_newline(s: &[u8], ptr: usize, nltype: u32, endptr: usize, utf: bool) -> Option<usize> {
    let c = if utf { getchar(s, ptr).0 } else { at(s, ptr) };
    if nltype == NLTYPE_ANYCRLF {
        match c {
            0x0a => Some(1),
            0x0d => Some(if ptr + 1 < endptr && at(s, ptr + 1) == 0x0a {
                2
            } else {
                1
            }),
            _ => None,
        }
    } else {
        match c {
            0x0a | 0x0b | 0x0c => Some(1),
            0x0d => Some(if ptr + 1 < endptr && at(s, ptr + 1) == 0x0a {
                2
            } else {
                1
            }),
            0x85 => Some(if utf { 2 } else { 1 }),
            0x2028 | 0x2029 => Some(3),
            _ => None,
        }
    }
}

/// PRIV(was_newline): checks for a newline immediately before `ptr`;
/// `ptr` > `startptr`.
pub fn was_newline(s: &[u8], ptr: usize, nltype: u32, startptr: usize, utf: bool) -> Option<usize> {
    let mut p = ptr - 1;
    let c = if utf {
        p = backchar(s, p);
        getchar(s, p).0
    } else {
        at(s, p)
    };
    if nltype == NLTYPE_ANYCRLF {
        match c {
            0x0a => Some(if p > startptr && at(s, p - 1) == 0x0d {
                2
            } else {
                1
            }),
            0x0d => Some(1),
            _ => None,
        }
    } else {
        match c {
            0x0a => Some(if p > startptr && at(s, p - 1) == 0x0d {
                2
            } else {
                1
            }),
            0x0b | 0x0c | 0x0d => Some(1),
            0x85 => Some(if utf { 2 } else { 1 }),
            0x2028 | 0x2029 => Some(3),
            _ => None,
        }
    }
}

/// PRIV(valid_utf) for UTF-8. On error returns (error code, offset).
pub fn valid_utf(s: &[u8]) -> Result<(), (i32, usize)> {
    let mut length = s.len();
    let mut p = 0usize;
    while length > 0 {
        let c = s[p] as u32;
        length -= 1;
        if c < 128 {
            p += 1;
            continue;
        }
        if c < 0xc0 {
            return Err((PCRE2_ERROR_UTF8_ERR20, p));
        }
        if c >= 0xfe {
            return Err((PCRE2_ERROR_UTF8_ERR21, p));
        }
        let ab = tables::UTF8_TABLE4[(c & 0x3f) as usize] as usize;
        if length < ab {
            let code = match ab - length {
                1 => PCRE2_ERROR_UTF8_ERR1,
                2 => PCRE2_ERROR_UTF8_ERR2,
                3 => PCRE2_ERROR_UTF8_ERR3,
                4 => PCRE2_ERROR_UTF8_ERR4,
                _ => PCRE2_ERROR_UTF8_ERR5,
            };
            return Err((code, p));
        }
        length -= ab;
        p += 1;
        let d = s[p] as u32;
        if (d & 0xc0) != 0x80 {
            return Err((PCRE2_ERROR_UTF8_ERR6, p - 1));
        }
        match ab {
            1 => {
                if (c & 0x3e) == 0 {
                    return Err((PCRE2_ERROR_UTF8_ERR15, p - 1));
                }
            }
            2 => {
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR7, p - 2));
                }
                if c == 0xe0 && (d & 0x20) == 0 {
                    return Err((PCRE2_ERROR_UTF8_ERR16, p - 2));
                }
                if c == 0xed && d >= 0xa0 {
                    return Err((PCRE2_ERROR_UTF8_ERR14, p - 2));
                }
            }
            3 => {
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR7, p - 2));
                }
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR8, p - 3));
                }
                if c == 0xf0 && (d & 0x30) == 0 {
                    return Err((PCRE2_ERROR_UTF8_ERR17, p - 3));
                }
                if c > 0xf4 || (c == 0xf4 && d > 0x8f) {
                    return Err((PCRE2_ERROR_UTF8_ERR13, p - 3));
                }
            }
            4 => {
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR7, p - 2));
                }
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR8, p - 3));
                }
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR9, p - 4));
                }
                if c == 0xf8 && (d & 0x38) == 0 {
                    return Err((PCRE2_ERROR_UTF8_ERR18, p - 4));
                }
            }
            _ => {
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR7, p - 2));
                }
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR8, p - 3));
                }
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR9, p - 4));
                }
                p += 1;
                if (s[p] & 0xc0) != 0x80 {
                    return Err((PCRE2_ERROR_UTF8_ERR10, p - 5));
                }
                if c == 0xfc && (d & 0x3c) == 0 {
                    return Err((PCRE2_ERROR_UTF8_ERR19, p - 5));
                }
            }
        }
        if ab > 3 {
            return Err((
                if ab == 4 {
                    PCRE2_ERROR_UTF8_ERR11
                } else {
                    PCRE2_ERROR_UTF8_ERR12
                },
                p - ab,
            ));
        }
        p += 1;
    }
    Ok(())
}

#[inline]
pub fn get(code: &[u8], n: usize) -> usize {
    ((code[n] as usize) << 8) | code[n + 1] as usize
}

#[inline]
pub fn put(code: &mut [u8], n: usize, d: usize) {
    code[n] = (d >> 8) as u8;
    code[n + 1] = (d & 255) as u8;
}

#[inline]
pub fn get2(code: &[u8], n: usize) -> u32 {
    ((code[n] as u32) << 8) | code[n + 1] as u32
}

#[inline]
pub fn put2(code: &mut [u8], n: usize, d: u32) {
    code[n] = (d >> 8) as u8;
    code[n + 1] = (d & 255) as u8;
}

#[inline]
pub fn op_len(op: u8) -> usize {
    tables::OP_LENGTHS[op as usize] as usize
}

/// Is `c` one of the single-character repeat opcodes that may carry a
/// multi-byte UTF-8 character (used when skipping over code)?
#[inline]
pub fn is_char_repeat_op(c: u32) -> bool {
    matches!(
        c,
        OP_CHAR
            | OP_CHARI
            | OP_NOT
            | OP_NOTI
            | OP_EXACT
            | OP_EXACTI
            | OP_NOTEXACT
            | OP_NOTEXACTI
            | OP_UPTO
            | OP_UPTOI
            | OP_NOTUPTO
            | OP_NOTUPTOI
            | OP_MINUPTO
            | OP_MINUPTOI
            | OP_NOTMINUPTO
            | OP_NOTMINUPTOI
            | OP_POSUPTO
            | OP_POSUPTOI
            | OP_NOTPOSUPTO
            | OP_NOTPOSUPTOI
            | OP_STAR
            | OP_STARI
            | OP_NOTSTAR
            | OP_NOTSTARI
            | OP_MINSTAR
            | OP_MINSTARI
            | OP_NOTMINSTAR
            | OP_NOTMINSTARI
            | OP_POSSTAR
            | OP_POSSTARI
            | OP_NOTPOSSTAR
            | OP_NOTPOSSTARI
            | OP_PLUS
            | OP_PLUSI
            | OP_NOTPLUS
            | OP_NOTPLUSI
            | OP_MINPLUS
            | OP_MINPLUSI
            | OP_NOTMINPLUS
            | OP_NOTMINPLUSI
            | OP_POSPLUS
            | OP_POSPLUSI
            | OP_NOTPOSPLUS
            | OP_NOTPOSPLUSI
            | OP_QUERY
            | OP_QUERYI
            | OP_NOTQUERY
            | OP_NOTQUERYI
            | OP_MINQUERY
            | OP_MINQUERYI
            | OP_NOTMINQUERY
            | OP_NOTMINQUERYI
            | OP_POSQUERY
            | OP_POSQUERYI
            | OP_NOTPOSQUERY
            | OP_NOTPOSQUERYI
    )
}

/// Advance over the extra fields of the opcode at `p` (shared part of
/// find_bracket/find_recurse): returns the position after the item.
#[inline]
pub fn skip_simple_item(code: &[u8], mut p: usize, utf: bool) -> usize {
    let c = code[p] as u32;
    match c {
        OP_TYPESTAR | OP_TYPEMINSTAR | OP_TYPEPLUS | OP_TYPEMINPLUS | OP_TYPEQUERY
        | OP_TYPEMINQUERY | OP_TYPEPOSSTAR | OP_TYPEPOSPLUS | OP_TYPEPOSQUERY => {
            if code[p + 1] as u32 == OP_PROP || code[p + 1] as u32 == OP_NOTPROP {
                p += 2;
            }
        }
        OP_TYPEUPTO | OP_TYPEMINUPTO | OP_TYPEEXACT | OP_TYPEPOSUPTO => {
            let o = code[p + 1 + IMM2_SIZE] as u32;
            if o == OP_PROP || o == OP_NOTPROP {
                p += 2;
            }
        }
        OP_MARK | OP_COMMIT_ARG | OP_PRUNE_ARG | OP_SKIP_ARG | OP_THEN_ARG => {
            p += code[p + 1] as usize;
        }
        _ => {}
    }
    p += op_len(c as u8);
    if utf && is_char_repeat_op(c) && has_extralen(code[p - 1]) {
        p += get_extralen(code[p - 1]);
    }
    p
}

/// PRIV(find_bracket): find the bracket for capture group `number`
/// (or the first lookbehind REVERSE if `number` < 0).
pub fn find_bracket(code: &[u8], mut p: usize, utf: bool, number: i32) -> Option<usize> {
    loop {
        let c = code[p] as u32;
        if c == OP_END {
            return None;
        }
        if c == OP_XCLASS {
            p += get(code, p + 1);
        } else if c == OP_CALLOUT_STR {
            p += get(code, p + 1 + 2 * LINK_SIZE);
        } else if c == OP_REVERSE || c == OP_VREVERSE {
            if number < 0 {
                return Some(p);
            }
            p += op_len(c as u8);
        } else if c == OP_CBRA || c == OP_SCBRA || c == OP_CBRAPOS || c == OP_SCBRAPOS {
            let n = get2(code, p + 1 + LINK_SIZE) as i32;
            if n == number {
                return Some(p);
            }
            p += op_len(c as u8);
        } else {
            p = skip_simple_item(code, p, utf);
        }
    }
}

/// PRIV(xclass): match a character against an extended class whose data
/// (starting with the flags byte) is at `code[data..]`.
pub fn xclass(c: u32, code: &[u8], mut data: usize, _utf: bool) -> bool {
    let negated = (code[data] as u32 & XCL_NOT) != 0;
    if c < 256 {
        if (code[data] as u32 & XCL_HASPROP) == 0 {
            if (code[data] as u32 & XCL_MAP) == 0 {
                return negated;
            }
            return (code[data + 1 + (c / 8) as usize] & (1u8 << (c & 7))) != 0;
        }
        if (code[data] as u32 & XCL_MAP) != 0
            && (code[data + 1 + (c / 8) as usize] & (1u8 << (c & 7))) != 0
        {
            return !negated;
        }
    }
    let flags = code[data] as u32;
    data += 1;
    if (flags & XCL_MAP) != 0 {
        data += 32;
    }
    loop {
        let t = code[data] as u32;
        data += 1;
        if t == XCL_END {
            break;
        }
        if t == XCL_SINGLE {
            let (x, l) = getchar(code, data);
            data += l;
            if c == x {
                return !negated;
            }
        } else if t == XCL_RANGE {
            let (x, l) = getchar(code, data);
            data += l;
            let (y, l) = getchar(code, data);
            data += l;
            if c >= x && c <= y {
                return !negated;
            }
        } else {
            let prop = get_ucd(c);
            let isprop = t == XCL_PROP;
            let d1 = code[data + 1] as u32;
            let gentype = |ct: u8| tables::UCP_GENTYPE[ct as usize];
            match code[data] as u32 {
                PT_ANY => {
                    if isprop {
                        return !negated;
                    }
                }
                PT_LAMP => {
                    let ct = prop.chartype as u32;
                    if (ct == ucp_Lu || ct == ucp_Ll || ct == ucp_Lt) == isprop {
                        return !negated;
                    }
                }
                PT_GC => {
                    if (d1 == gentype(prop.chartype)) == isprop {
                        return !negated;
                    }
                }
                PT_PC => {
                    if (d1 == prop.chartype as u32) == isprop {
                        return !negated;
                    }
                }
                PT_SC => {
                    if (d1 == prop.script as u32) == isprop {
                        return !negated;
                    }
                }
                PT_SCX => {
                    let ok = d1 == prop.script as u32
                        || mapbit(
                            &tables::UCD_SCRIPT_SETS[ucd_scriptx_prop(prop) as usize..],
                            d1,
                        );
                    if ok == isprop {
                        return !negated;
                    }
                }
                PT_ALNUM => {
                    let g = gentype(prop.chartype);
                    if (g == ucp_L || g == ucp_N) == isprop {
                        return !negated;
                    }
                }
                PT_SPACE | PT_PXSPACE => {
                    if is_hspace(c) || is_vspace(c) {
                        if isprop {
                            return !negated;
                        }
                    } else if (gentype(prop.chartype) == ucp_Z) == isprop {
                        return !negated;
                    }
                }
                PT_WORD => {
                    let ct = prop.chartype as u32;
                    let g = gentype(prop.chartype);
                    if (g == ucp_L || g == ucp_N || ct == ucp_Mn || ct == ucp_Pc) == isprop {
                        return !negated;
                    }
                }
                PT_UCNC => {
                    if c < 0xa0 {
                        if (c == b'$' as u32 || c == b'@' as u32 || c == b'`' as u32) == isprop {
                            return !negated;
                        }
                    } else if !(0xd800..=0xdfff).contains(&c) == isprop {
                        return !negated;
                    }
                }
                PT_BIDICL => {
                    if (ucd_bidiclass_prop(prop) == d1) == isprop {
                        return !negated;
                    }
                }
                PT_BOOL => {
                    let ok = mapbit(
                        &tables::UCD_BOOLPROP_SETS[ucd_bprops_prop(prop) as usize..],
                        d1,
                    );
                    if ok == isprop {
                        return !negated;
                    }
                }
                PT_PXGRAPH => {
                    let ct = prop.chartype as u32;
                    let g = gentype(prop.chartype);
                    if (g != ucp_Z
                        && (g != ucp_C
                            || (ct == ucp_Cf
                                && c != 0x061c
                                && c != 0x180e
                                && !(0x2066..=0x2069).contains(&c))))
                        == isprop
                    {
                        return !negated;
                    }
                }
                PT_PXPRINT => {
                    let ct = prop.chartype as u32;
                    let g = gentype(prop.chartype);
                    if (ct != ucp_Zl
                        && ct != ucp_Zp
                        && (g != ucp_C
                            || (ct == ucp_Cf && c != 0x061c && !(0x2066..=0x2069).contains(&c))))
                        == isprop
                    {
                        return !negated;
                    }
                }
                PT_PXPUNCT => {
                    let g = gentype(prop.chartype);
                    if (g == ucp_P || (c < 128 && g == ucp_S)) == isprop {
                        return !negated;
                    }
                }
                PT_PXXDIGIT => {
                    if (matches!(c, 0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66)
                        || (0xff10..=0xff19).contains(&c)
                        || (0xff21..=0xff26).contains(&c)
                        || (0xff41..=0xff46).contains(&c))
                        == isprop
                    {
                        return !negated;
                    }
                }
                _ => return false,
            }
            data += 2;
        }
    }
    negated
}

/// PRIV(extuni): match an extended grapheme cluster continuing from `eptr`
/// after a first character `c`.
pub fn extuni(
    mut c: u32,
    s: &[u8],
    mut eptr: usize,
    start_subject: usize,
    end_subject: usize,
    utf: bool,
    mut xcount: Option<&mut i32>,
) -> usize {
    let mut was_ep_zwj = false;
    let mut lgb = ucd_graphbreak(c);
    while eptr < end_subject {
        let mut len = 1;
        if !utf {
            c = s[eptr] as u32;
        } else {
            let (cc, l) = getchar(s, eptr);
            c = cc;
            len = l;
        }
        let rgb = ucd_graphbreak(c);
        if (tables::UCP_GBTABLE[lgb as usize] & (1u32 << rgb)) == 0 {
            break;
        }
        if lgb == ucp_gbZWJ && rgb == ucp_gbExtended_Pictographic && !was_ep_zwj {
            break;
        }
        if lgb == ucp_gbRegional_Indicator && rgb == ucp_gbRegional_Indicator {
            let mut ricount = 0;
            let mut bptr = eptr - 1;
            if utf {
                bptr = backchar(s, bptr);
            }
            while bptr > start_subject {
                bptr -= 1;
                if utf {
                    bptr = backchar(s, bptr);
                    c = getchar(s, bptr).0;
                } else {
                    c = s[bptr] as u32;
                }
                if ucd_graphbreak(c) != ucp_gbRegional_Indicator {
                    break;
                }
                ricount += 1;
            }
            if (ricount & 1) != 0 {
                break;
            }
        }
        was_ep_zwj = lgb == ucp_gbExtended_Pictographic && rgb == ucp_gbZWJ;
        if rgb != ucp_gbExtend || lgb != ucp_gbExtended_Pictographic {
            lgb = rgb;
        }
        eptr += len;
        if let Some(x) = xcount.as_deref_mut() {
            *x += 1;
        }
    }
    eptr
}

const UCD_MAPSIZE: usize = (ucp_Unknown / 32 + 1) as usize;
const FULL_MAPSIZE: usize = (ucp_Script_Count / 32 + 1) as usize;
const FOUND_BOPOMOFO: u32 = 1;
const FOUND_HIRAGANA: u32 = 2;
const FOUND_KATAKANA: u32 = 4;
const FOUND_HANGUL: u32 = 8;

/// PRIV(script_run)
pub fn script_run(s: &[u8], mut ptr: usize, endptr: usize, utf: bool) -> bool {
    #[derive(PartialEq)]
    enum St {
        Unset,
        Map,
        HanPending,
        HanHiraKata,
        HanBopomofo,
        HanHangul,
    }
    let mut require_state = St::Unset;
    let mut require_map = [0u32; FULL_MAPSIZE];
    let mut map = [0u32; FULL_MAPSIZE];
    let mut require_digitset = 0u32;
    if ptr >= endptr {
        return true;
    }
    let (mut c, l) = getchar_test(s, ptr, utf);
    ptr += l;
    if ptr >= endptr {
        return true;
    }
    loop {
        let ucd = get_ucd(c);
        let script = ucd.script as u32;
        if script == ucp_Unknown {
            return false;
        }
        if ucd_scriptx_prop(ucd) != 0 || (script != ucp_Inherited && script != ucp_Common) {
            let off = ucd_scriptx_prop(ucd) as usize;
            map[..UCD_MAPSIZE].copy_from_slice(&tables::UCD_SCRIPT_SETS[off..off + UCD_MAPSIZE]);
            for m in map.iter_mut().skip(UCD_MAPSIZE) {
                *m = 0;
            }
            if script != ucp_Common && script != ucp_Inherited {
                map[(script / 32) as usize] |= 1u32 << (script % 32);
            }
            match require_state {
                St::Unset => match script {
                    ucp_Han => require_state = St::HanPending,
                    ucp_Hiragana | ucp_Katakana => require_state = St::HanHiraKata,
                    ucp_Bopomofo => require_state = St::HanBopomofo,
                    ucp_Hangul => require_state = St::HanHangul,
                    _ => {
                        require_map = map;
                        require_state = St::Map;
                    }
                },
                St::HanPending => {
                    if script != ucp_Han {
                        let mut chspecial = 0;
                        if mapbit(&map, ucp_Bopomofo) {
                            chspecial |= FOUND_BOPOMOFO;
                        }
                        if mapbit(&map, ucp_Hiragana) {
                            chspecial |= FOUND_HIRAGANA;
                        }
                        if mapbit(&map, ucp_Katakana) {
                            chspecial |= FOUND_KATAKANA;
                        }
                        if mapbit(&map, ucp_Hangul) {
                            chspecial |= FOUND_HANGUL;
                        }
                        if chspecial == 0 {
                            return false;
                        }
                        if chspecial == FOUND_BOPOMOFO {
                            require_state = St::HanBopomofo;
                        } else if chspecial == (FOUND_HIRAGANA | FOUND_KATAKANA) {
                            require_state = St::HanHiraKata;
                        }
                    }
                }
                St::HanHiraKata => {
                    if !mapbit(&map, ucp_Han)
                        && !mapbit(&map, ucp_Hiragana)
                        && !mapbit(&map, ucp_Katakana)
                    {
                        return false;
                    }
                }
                St::HanBopomofo => {
                    if !mapbit(&map, ucp_Han) && !mapbit(&map, ucp_Bopomofo) {
                        return false;
                    }
                }
                St::HanHangul => {
                    if !mapbit(&map, ucp_Han) && !mapbit(&map, ucp_Hangul) {
                        return false;
                    }
                }
                St::Map => {
                    let ok = (0..FULL_MAPSIZE).any(|i| (require_map[i] & map[i]) != 0);
                    if !ok {
                        return false;
                    }
                    match script {
                        ucp_Han => require_state = St::HanPending,
                        ucp_Hiragana | ucp_Katakana => require_state = St::HanHiraKata,
                        ucp_Bopomofo => require_state = St::HanBopomofo,
                        ucp_Hangul => require_state = St::HanHangul,
                        _ => {
                            for i in 0..FULL_MAPSIZE {
                                require_map[i] &= map[i];
                            }
                        }
                    }
                }
            }
        }
        if ucd.chartype as u32 == ucp_Nd {
            let ds = &tables::UCD_DIGIT_SETS;
            let digitset = if c <= ds[1] {
                1
            } else {
                let mut bot = 1usize;
                let mut top = ds[0] as usize;
                loop {
                    if top <= bot + 1 {
                        break top as u32;
                    }
                    let mid = (top + bot) / 2;
                    if c <= ds[mid] {
                        top = mid;
                    } else {
                        bot = mid;
                    }
                }
            };
            if require_digitset == 0 {
                require_digitset = digitset;
            } else if digitset != require_digitset {
                return false;
            }
        }
        if ptr >= endptr {
            return true;
        }
        let (cc, l) = getchar_test(s, ptr, utf);
        c = cc;
        ptr += l;
    }
}

/// pcre2_get_error_message()
pub fn error_message(enumber: i32) -> &'static str {
    use crate::errtexts::*;
    if enumber >= COMPILE_ERROR_BASE as i32 {
        COMPILE_ERROR_TEXTS
            .get((enumber - COMPILE_ERROR_BASE as i32) as usize)
            .copied()
            .unwrap_or("bad data value")
    } else if enumber < 0 {
        MATCH_ERROR_TEXTS
            .get((-enumber) as usize)
            .copied()
            .unwrap_or("bad data value")
    } else {
        "bad data value"
    }
}
