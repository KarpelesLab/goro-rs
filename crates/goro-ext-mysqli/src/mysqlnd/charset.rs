//! Port of php-src `ext/mysqlnd/mysqlnd_charset.c`: the MySQL character set
//! table, per-encoding multibyte length/validity checks, and the charset-aware
//! string escapers used by `mysqli_real_escape_string()`.
#![allow(dead_code)]

/// `MYSQLND_UTF8_MB3_DEFAULT_ID`
pub const UTF8_MB3_DEFAULT_ID: u32 = 33;
/// `MYSQLND_UTF8_MB4_DEFAULT_ID`
pub const UTF8_MB4_DEFAULT_ID: u32 = 45;

/// Mirrors the C `ENUMERATE_ENCODINGS_CHARLEN` / `ENUMERATE_ENCODINGS_VALID`
/// ids (both lists enumerate the same encodings in the same order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enc {
    Null,
    Big5,
    Ujis,
    Sjis,
    Euckr,
    Gb2312,
    Gbk,
    Utf8mb3,
    Ucs2,
    Eucjpms,
    Utf8,
    Utf16,
    Utf32,
    Cp932,
    Gb18030,
}

/// `MYSQLND_CHARSET`
#[derive(Debug)]
pub struct Charset {
    pub nr: u32,
    pub name: &'static str,
    pub collation: &'static str,
    pub char_minlen: u32,
    pub char_maxlen: u32,
    pub comment: &'static str,
    mb_charlen: Enc,
    mb_valid: Enc,
    lowest_mb_byte: u32,
}

impl Charset {
    /// `mysqlnd_mbcharlen_dispatch(cset->mb_charlen, c)`
    pub fn mb_charlen(&self, c: u32) -> u32 {
        mbcharlen_dispatch(self.mb_charlen, c)
    }

    /// `mysqlnd_mbvalid_dispatch(cset->mb_valid, start, end)`: number of bytes
    /// of the valid multibyte character at the start of `s`, or 0.
    pub fn mb_valid(&self, s: &[u8]) -> u32 {
        mbvalid_dispatch(self.mb_valid, s)
    }

    pub fn is_multibyte(&self) -> bool {
        self.char_maxlen > 1
    }

    /// `cset->lowest_mb_byte` (0x100 for single-byte charsets).
    pub fn lowest_mb_byte(&self) -> u32 {
        self.lowest_mb_byte
    }
}

/// Byte at `i`, or 0 past the end. The C code reads through raw pointers into
/// NUL-terminated zend_strings, so the byte just past the end is `'\0'`.
#[inline]
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/* {{{ utf8 functions */
fn check_mb_utf8mb3_sequence(s: &[u8]) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let c = s[0];
    if c < 0x80 {
        return 1; /* single byte character */
    }
    if c < 0xC2 {
        return 0; /* invalid mb character */
    }
    if c < 0xE0 {
        if s.len() < 2 {
            return 0; /* too small */
        }
        if (s[1] ^ 0x80) >= 0x40 {
            return 0;
        }
        return 2;
    }
    if c < 0xF0 {
        if s.len() < 3 {
            return 0; /* too small */
        }
        if !((s[1] ^ 0x80) < 0x40 && (s[2] ^ 0x80) < 0x40 && (c >= 0xE1 || s[1] >= 0xA0)) {
            return 0; /* invalid utf8 character */
        }
        return 3;
    }
    0
}

fn check_mb_utf8_sequence(s: &[u8]) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let c = s[0];
    if c < 0x80 {
        return 1; /* single byte character */
    }
    if c < 0xC2 {
        return 0; /* invalid mb character */
    }
    if c < 0xE0 {
        if s.len() < 2 {
            return 0; /* too small */
        }
        if (s[1] ^ 0x80) >= 0x40 {
            return 0;
        }
        return 2;
    }
    if c < 0xF0 {
        if s.len() < 3 {
            return 0; /* too small */
        }
        if !((s[1] ^ 0x80) < 0x40 && (s[2] ^ 0x80) < 0x40 && (c >= 0xE1 || s[1] >= 0xA0)) {
            return 0; /* invalid utf8 character */
        }
        return 3;
    }
    if c < 0xF5 {
        if s.len() < 4 {
            return 0; /* too small */
        }
        /*
          Valid codes:
          [F0][90..BF][80..BF][80..BF]
          [F1][80..BF][80..BF][80..BF]
          [F2][80..BF][80..BF][80..BF]
          [F3][80..BF][80..BF][80..BF]
          [F4][80..8F][80..BF][80..BF]
        */
        if !((s[1] ^ 0x80) < 0x40
            && (s[2] ^ 0x80) < 0x40
            && (s[3] ^ 0x80) < 0x40
            && (c >= 0xf1 || s[1] >= 0x90)
            && (c <= 0xf3 || s[1] <= 0x8F))
        {
            return 0; /* invalid utf8 character */
        }
        return 4;
    }
    0
}

fn check_mb_utf8mb3_valid(s: &[u8]) -> u32 {
    let len = check_mb_utf8mb3_sequence(s);
    if len > 1 { len } else { 0 }
}

fn check_mb_utf8_valid(s: &[u8]) -> u32 {
    let len = check_mb_utf8_sequence(s);
    if len > 1 { len } else { 0 }
}

fn mysqlnd_mbcharlen_utf8mb3(utf8: u32) -> u32 {
    if utf8 < 0x80 {
        return 1; /* single byte character */
    }
    if utf8 < 0xC2 {
        return 0; /* invalid multibyte header */
    }
    if utf8 < 0xE0 {
        return 2; /* double byte character */
    }
    if utf8 < 0xF0 {
        return 3; /* triple byte character */
    }
    0
}

fn mysqlnd_mbcharlen_utf8(utf8: u32) -> u32 {
    if utf8 < 0x80 {
        return 1; /* single byte character */
    }
    if utf8 < 0xC2 {
        return 0; /* invalid multibyte header */
    }
    if utf8 < 0xE0 {
        return 2; /* double byte character */
    }
    if utf8 < 0xF0 {
        return 3; /* triple byte character */
    }
    if utf8 < 0xF8 {
        return 4; /* four byte character */
    }
    0
}
/* }}} */

/* {{{ big5 functions */
#[inline]
fn valid_big5head(c: u8) -> bool {
    (0xA1..=0xF9).contains(&c)
}
#[inline]
fn valid_big5tail(c: u8) -> bool {
    (0x40..=0x7E).contains(&c) || (0xA1..=0xFE).contains(&c)
}

fn check_mb_big5(s: &[u8]) -> u32 {
    if !s.is_empty() && valid_big5head(s[0]) && s.len() > 1 && valid_big5tail(s[1]) {
        2
    } else {
        0
    }
}

fn mysqlnd_mbcharlen_big5(big5: u32) -> u32 {
    if valid_big5head(big5 as u8) { 2 } else { 1 }
}
/* }}} */

/* {{{ cp932 functions */
#[inline]
fn valid_cp932head(c: u8) -> bool {
    (0x81..=0x9F).contains(&c) || (0xE0..=0xFC).contains(&c)
}
#[inline]
fn valid_cp932tail(c: u8) -> bool {
    (0x40..=0x7E).contains(&c) || (0x80..=0xFC).contains(&c)
}

fn check_mb_cp932(s: &[u8]) -> u32 {
    if !s.is_empty() && valid_cp932head(s[0]) && s.len() > 1 && valid_cp932tail(s[1]) {
        2
    } else {
        0
    }
}

fn mysqlnd_mbcharlen_cp932(cp932: u32) -> u32 {
    if valid_cp932head(cp932 as u8) { 2 } else { 1 }
}
/* }}} */

/* {{{ euckr functions */
#[inline]
fn valid_euckr(c: u8) -> bool {
    (0xA1..=0xFE).contains(&c)
}

fn check_mb_euckr(s: &[u8]) -> u32 {
    if s.len() <= 1 {
        return 0; /* invalid length */
    }
    if s[0] < 0x80 {
        return 0; /* invalid euckr character */
    }
    if valid_euckr(s[1]) {
        return 2;
    }
    0
}

fn mysqlnd_mbcharlen_euckr(kr: u32) -> u32 {
    if valid_euckr(kr as u8) { 2 } else { 1 }
}
/* }}} */

/* {{{ eucjpms functions */
#[inline]
fn valid_eucjpms(c: u32) -> bool {
    (0xA1..=0xFE).contains(&(c & 0xFF))
}
#[inline]
fn valid_eucjpms_kata(c: u32) -> bool {
    (0xA1..=0xDF).contains(&(c & 0xFF))
}
#[inline]
fn valid_eucjpms_ss2(c: u32) -> bool {
    (c & 0xFF) == 0x8E
}
#[inline]
fn valid_eucjpms_ss3(c: u32) -> bool {
    (c & 0xFF) == 0x8F
}

fn check_mb_eucjpms(s: &[u8]) -> u32 {
    if s.is_empty() || s[0] < 0x80 {
        return 0; /* invalid eucjpms character */
    }
    let s0 = s[0] as u32;
    if valid_eucjpms(s0) && s.len() > 1 && valid_eucjpms(s[1] as u32) {
        return 2;
    }
    if valid_eucjpms_ss2(s0) && s.len() > 1 && valid_eucjpms_kata(s[1] as u32) {
        return 2;
    }
    if valid_eucjpms_ss3(s0)
        && s.len() > 2
        && valid_eucjpms(s[1] as u32)
        && valid_eucjpms(s[2] as u32)
    {
        return 3;
    }
    0
}

fn mysqlnd_mbcharlen_eucjpms(jpms: u32) -> u32 {
    if valid_eucjpms(jpms) || valid_eucjpms_ss2(jpms) {
        return 2;
    }
    if valid_eucjpms_ss3(jpms) {
        return 3;
    }
    1
}
/* }}} */

/* {{{ gb2312 functions */
#[inline]
fn valid_gb2312_head(c: u8) -> bool {
    (0xA1..=0xF7).contains(&c)
}
#[inline]
fn valid_gb2312_tail(c: u8) -> bool {
    (0xA1..=0xFE).contains(&c)
}

fn check_mb_gb2312(s: &[u8]) -> u32 {
    if !s.is_empty() && valid_gb2312_head(s[0]) && s.len() > 1 && valid_gb2312_tail(s[1]) {
        2
    } else {
        0
    }
}

fn mysqlnd_mbcharlen_gb2312(gb: u32) -> u32 {
    if valid_gb2312_head(gb as u8) { 2 } else { 1 }
}
/* }}} */

/* {{{ gbk functions */
#[inline]
fn valid_gbk_head(c: u8) -> bool {
    (0x81..=0xFE).contains(&c)
}
#[inline]
fn valid_gbk_tail(c: u8) -> bool {
    (0x40..=0x7E).contains(&c) || (0x80..=0xFE).contains(&c)
}

fn check_mb_gbk(s: &[u8]) -> u32 {
    if !s.is_empty() && valid_gbk_head(s[0]) && s.len() > 1 && valid_gbk_tail(s[1]) {
        2
    } else {
        0
    }
}

fn mysqlnd_mbcharlen_gbk(gbk: u32) -> u32 {
    if valid_gbk_head(gbk as u8) { 2 } else { 1 }
}
/* }}} */

/* {{{ sjis functions */
#[inline]
fn valid_sjis_head(c: u8) -> bool {
    (0x81..=0x9F).contains(&c) || (0xE0..=0xFC).contains(&c)
}
#[inline]
fn valid_sjis_tail(c: u8) -> bool {
    (0x40..=0x7E).contains(&c) || (0x80..=0xFC).contains(&c)
}

fn check_mb_sjis(s: &[u8]) -> u32 {
    if !s.is_empty() && valid_sjis_head(s[0]) && s.len() > 1 && valid_sjis_tail(s[1]) {
        2
    } else {
        0
    }
}

fn mysqlnd_mbcharlen_sjis(sjis: u32) -> u32 {
    if valid_sjis_head(sjis as u8) { 2 } else { 1 }
}
/* }}} */

/* {{{ ucs2 functions */
fn check_mb_ucs2(_s: &[u8]) -> u32 {
    2 /* always 2 */
}

fn mysqlnd_mbcharlen_ucs2(_ucs2: u32) -> u32 {
    2 /* always 2 */
}
/* }}} */

/* {{{ ujis functions */
#[inline]
fn valid_ujis(c: u32) -> bool {
    (0xA1..=0xFE).contains(&(c & 0xFF))
}
#[inline]
fn valid_ujis_kata(c: u32) -> bool {
    (0xA1..=0xDF).contains(&(c & 0xFF))
}
#[inline]
fn valid_ujis_ss2(c: u32) -> bool {
    (c & 0xFF) == 0x8E
}
#[inline]
fn valid_ujis_ss3(c: u32) -> bool {
    (c & 0xFF) == 0x8F
}

fn check_mb_ujis(s: &[u8]) -> u32 {
    if s.is_empty() || s[0] < 0x80 {
        return 0; /* invalid ujis character */
    }
    let s0 = s[0] as u32;
    // The C code reads start[1] without a length check (it relies on the
    // zend_string NUL terminator); `at()` reproduces that.
    let s1 = at(s, 1) as u32;
    if valid_ujis(s0) && valid_ujis(s1) {
        return 2;
    }
    if valid_ujis_ss2(s0) && valid_ujis_kata(s1) {
        return 2;
    }
    if valid_ujis_ss3(s0) && s.len() > 2 && valid_ujis(s1) && valid_ujis(s[2] as u32) {
        return 3;
    }
    0
}

fn mysqlnd_mbcharlen_ujis(ujis: u32) -> u32 {
    if valid_ujis(ujis) || valid_ujis_ss2(ujis) {
        2
    } else if valid_ujis_ss3(ujis) {
        3
    } else {
        1
    }
}
/* }}} */

/* {{{ utf16 functions */
#[inline]
fn utf16_high_head(x: u8) -> bool {
    (x & 0xFC) == 0xD8
}
#[inline]
fn utf16_low_head(x: u8) -> bool {
    (x & 0xFC) == 0xDC
}

fn check_mb_utf16(s: &[u8]) -> u32 {
    if s.len() < 2 {
        return 0;
    }
    if utf16_high_head(s[0]) {
        return if s.len() >= 4 && utf16_low_head(s[2]) {
            4
        } else {
            0
        };
    }
    if utf16_low_head(s[0]) {
        return 0;
    }
    2
}

fn mysqlnd_mbcharlen_utf16(utf16: u32) -> u32 {
    if utf16_high_head(utf16 as u8) { 4 } else { 2 }
}
/* }}} */

/* {{{ utf32 functions */
fn check_mb_utf32(_s: &[u8]) -> u32 {
    4
}

fn mysqlnd_mbcharlen_utf32(_utf32: u32) -> u32 {
    4
}
/* }}} */

/* {{{ gb18030 functions */
#[inline]
fn is_gb18030_odd(c: u8) -> bool {
    (0x81..=0xFE).contains(&c)
}
#[inline]
fn is_gb18030_even_2(c: u8) -> bool {
    (0x40..=0x7E).contains(&c) || (0x80..=0xFE).contains(&c)
}
#[inline]
fn is_gb18030_even_4(c: u8) -> bool {
    (0x30..=0x39).contains(&c)
}

fn mysqlnd_mbcharlen_gb18030(c: u32) -> u32 {
    if c <= 0xFF {
        return u32::from(!is_gb18030_odd(c as u8));
    }
    if c > 0xFFFF || !is_gb18030_odd(((c >> 8) & 0xFF) as u8) {
        return 0;
    }
    if is_gb18030_even_2((c & 0xFF) as u8) {
        return 2;
    }
    if is_gb18030_even_4((c & 0xFF) as u8) {
        return 4;
    }
    0
}

fn my_ismbchar_gb18030(s: &[u8]) -> u32 {
    if s.len() <= 1 || !is_gb18030_odd(s[0]) {
        return 0;
    }
    if is_gb18030_even_2(s[1]) {
        return 2;
    } else if s.len() > 3
        && is_gb18030_even_4(s[1])
        && is_gb18030_odd(s[2])
        && is_gb18030_even_4(s[3])
    {
        return 4;
    }
    0
}
/* }}} */

/// `mysqlnd_mbcharlen_dispatch`. The C `mysqlnd_mbcharlen_null` is
/// `ZEND_UNREACHABLE(); return 0;` — we return 0.
fn mbcharlen_dispatch(encoding: Enc, c: u32) -> u32 {
    match encoding {
        Enc::Null => 0,
        Enc::Big5 => mysqlnd_mbcharlen_big5(c),
        Enc::Ujis => mysqlnd_mbcharlen_ujis(c),
        Enc::Sjis => mysqlnd_mbcharlen_sjis(c),
        Enc::Euckr => mysqlnd_mbcharlen_euckr(c),
        Enc::Gb2312 => mysqlnd_mbcharlen_gb2312(c),
        Enc::Gbk => mysqlnd_mbcharlen_gbk(c),
        Enc::Utf8mb3 => mysqlnd_mbcharlen_utf8mb3(c),
        Enc::Ucs2 => mysqlnd_mbcharlen_ucs2(c),
        Enc::Eucjpms => mysqlnd_mbcharlen_eucjpms(c),
        Enc::Utf8 => mysqlnd_mbcharlen_utf8(c),
        Enc::Utf16 => mysqlnd_mbcharlen_utf16(c),
        Enc::Utf32 => mysqlnd_mbcharlen_utf32(c),
        Enc::Cp932 => mysqlnd_mbcharlen_cp932(c),
        Enc::Gb18030 => mysqlnd_mbcharlen_gb18030(c),
    }
}

/// `mysqlnd_mbvalid_dispatch`. The C `check_null` is
/// `ZEND_UNREACHABLE(); return 0;` — we return 0.
fn mbvalid_dispatch(encoding: Enc, s: &[u8]) -> u32 {
    match encoding {
        Enc::Null => 0,
        Enc::Big5 => check_mb_big5(s),
        Enc::Ujis => check_mb_ujis(s),
        Enc::Sjis => check_mb_sjis(s),
        Enc::Euckr => check_mb_euckr(s),
        Enc::Gb2312 => check_mb_gb2312(s),
        Enc::Gbk => check_mb_gbk(s),
        Enc::Utf8mb3 => check_mb_utf8mb3_valid(s),
        Enc::Ucs2 => check_mb_ucs2(s),
        Enc::Eucjpms => check_mb_eucjpms(s),
        Enc::Utf8 => check_mb_utf8_valid(s),
        Enc::Utf16 => check_mb_utf16(s),
        Enc::Utf32 => check_mb_utf32(s),
        Enc::Cp932 => check_mb_cp932(s),
        Enc::Gb18030 => my_ismbchar_gb18030(s),
    }
}

/// `mysqlnd_find_charset_nr`
pub fn find_charset_nr(nr: u32) -> Option<&'static Charset> {
    CHARSETS.iter().find(|c| c.nr == nr)
}

/// `mysqlnd_find_charset_name`: first entry whose `name` matches under
/// `strcasecmp` (ASCII case-insensitive; the C string ends at the first NUL).
pub fn find_charset_name(name: &str) -> Option<&'static Charset> {
    let name = name.as_bytes();
    let name = match name.iter().position(|&b| b == 0) {
        Some(p) => &name[..p],
        None => name,
    };
    CHARSETS
        .iter()
        .find(|c| c.name.as_bytes().eq_ignore_ascii_case(name))
}

/// Copies the valid multibyte char of `len` bytes at `s[i..]`. ucs2/utf32
/// report a fixed length without a bounds check (C over-reads); clamp it.
#[inline]
fn copy_mb(out: &mut Vec<u8>, s: &[u8], i: usize, len: usize) -> usize {
    let end = (i + len).min(s.len());
    out.extend_from_slice(&s[i..end]);
    end
}

/// `mysqlnd_cset_escape_quotes` (NO_BACKSLASH_ESCAPES mode): doubles `'`.
pub fn escape_quotes(cs: &Charset, s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 * s.len());
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        /* check unicode characters */
        if (b as u32) >= cs.lowest_mb_byte {
            let len = mbvalid_dispatch(cs.mb_valid, &s[i..]) as usize;
            if len != 0 {
                /* copy mb char without escaping it */
                i = copy_mb(&mut out, s, i, len);
                continue;
            }
        }
        if b == b'\'' {
            out.push(b'\'');
            out.push(b'\'');
        } else {
            out.push(b);
        }
        i += 1;
    }
    out
}

/// `mysqlnd_cset_escape_slashes`: backslash-escapes `\0 \n \r \\ ' " \032`,
/// leaving valid multibyte characters untouched; a byte that starts an
/// invalid/partial multibyte sequence is itself backslash-escaped.
pub fn escape_slashes(cs: &Charset, s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 * s.len());
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        let mut esc: u8 = 0;

        /* check unicode characters */
        if (b as u32) >= cs.lowest_mb_byte {
            let len = mbvalid_dispatch(cs.mb_valid, &s[i..]) as usize;
            if len != 0 {
                /* copy mb char without escaping it */
                i = copy_mb(&mut out, s, i, len);
                continue;
            // C passes `*escapestr` (a `char`, signed on x86/x86-64) to an
            // `unsigned int` parameter, so bytes >= 0x80 arrive sign-extended
            // (e.g. 0xC3 -> 0xFFFFFFC3); mirror the x86 behaviour.
            } else if mbcharlen_dispatch(cs.mb_charlen, b as i8 as i32 as u32) > 1 {
                esc = b;
            }
        }
        if esc == 0 {
            esc = match b {
                0 => b'0',
                b'\n' => b'n',
                b'\r' => b'r',
                b'\\' | b'\'' | b'"' => b,
                b'\x1a' => b'Z',
                _ => 0,
            };
        }
        if esc != 0 {
            /* copy escaped character */
            out.push(b'\\');
            out.push(esc);
        } else {
            /* copy non escaped character */
            out.push(b);
        }
        i += 1;
    }
    out
}

#[allow(clippy::too_many_arguments)]
const fn cs(
    nr: u32,
    name: &'static str,
    collation: &'static str,
    char_minlen: u32,
    char_maxlen: u32,
    comment: &'static str,
    enc: Enc,
    lowest_mb_byte: u32,
) -> Charset {
    Charset {
        nr,
        name,
        collation,
        char_minlen,
        char_maxlen,
        comment,
        mb_charlen: enc,
        mb_valid: enc,
        lowest_mb_byte,
    }
}

/// `mysqlnd_charsets[]` (without the `{0, NULL, ...}` sentinel), same order.
pub static CHARSETS: &[Charset] = &[
    cs(1, "big5", "big5_chinese_ci", 1, 2, "", Enc::Big5, 0xa1),
    cs(3, "dec8", "dec8_swedish_ci", 1, 1, "", Enc::Null, 0x100),
    cs(4, "cp850", "cp850_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(6, "hp8", "hp8_english_ci", 1, 1, "", Enc::Null, 0x100),
    cs(7, "koi8r", "koi8r_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(8, "latin1", "latin1_swedish_ci", 1, 1, "", Enc::Null, 0x100),
    cs(5, "latin1", "latin1_german1_ci", 1, 1, "", Enc::Null, 0x100),
    cs(9, "latin2", "latin2_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(2, "latin2", "latin2_czech_cs", 1, 1, "", Enc::Null, 0x100),
    cs(10, "swe7", "swe7_swedish_ci", 1, 1, "", Enc::Null, 0x100),
    cs(11, "ascii", "ascii_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(12, "ujis", "ujis_japanese_ci", 1, 3, "", Enc::Ujis, 0x80),
    cs(13, "sjis", "sjis_japanese_ci", 1, 2, "", Enc::Sjis, 0x80),
    cs(
        16,
        "hebrew",
        "hebrew_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(17, "filename", "filename", 1, 5, "", Enc::Null, 0x100),
    cs(18, "tis620", "tis620_thai_ci", 1, 1, "", Enc::Null, 0x100),
    cs(19, "euckr", "euckr_korean_ci", 1, 2, "", Enc::Euckr, 0x80),
    cs(
        21,
        "latin2",
        "latin2_hungarian_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        27,
        "latin2",
        "latin2_croatian_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(22, "koi8u", "koi8u_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(
        24,
        "gb2312",
        "gb2312_chinese_ci",
        1,
        2,
        "",
        Enc::Gb2312,
        0xa1,
    ),
    cs(25, "greek", "greek_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(
        26,
        "cp1250",
        "cp1250_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(28, "gbk", "gbk_chinese_ci", 1, 2, "", Enc::Gbk, 0x80),
    cs(
        30,
        "latin5",
        "latin5_turkish_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        31,
        "latin1",
        "latin1_german2_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(15, "latin1", "latin1_danish_ci", 1, 1, "", Enc::Null, 0x100),
    cs(
        32,
        "armscii8",
        "armscii8_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        33,
        "utf8",
        "utf8_general_ci",
        1,
        3,
        "UTF-8 Unicode",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(
        35,
        "ucs2",
        "ucs2_general_ci",
        2,
        2,
        "UCS-2 Unicode",
        Enc::Ucs2,
        0x0,
    ),
    cs(36, "cp866", "cp866_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(
        37,
        "keybcs2",
        "keybcs2_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(38, "macce", "macce_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(
        39,
        "macroman",
        "macroman_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(40, "cp852", "cp852_general_ci", 1, 1, "", Enc::Null, 0x100),
    cs(
        41,
        "latin7",
        "latin7_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        20,
        "latin7",
        "latin7_estonian_cs",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        57,
        "cp1256",
        "cp1256_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        59,
        "cp1257",
        "cp1257_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(63, "binary", "binary", 1, 1, "", Enc::Null, 0x100),
    cs(
        97,
        "eucjpms",
        "eucjpms_japanese_ci",
        1,
        3,
        "",
        Enc::Eucjpms,
        0x80,
    ),
    cs(
        29,
        "cp1257",
        "cp1257_lithuanian_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        31,
        "latin1",
        "latin1_german2_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(34, "cp1250", "cp1250_czech_cs", 1, 1, "", Enc::Null, 0x100),
    cs(
        42,
        "latin7",
        "latin7_general_cs",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(43, "macce", "macce_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        44,
        "cp1250",
        "cp1250_croatian_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        45,
        "utf8mb4",
        "utf8mb4_general_ci",
        1,
        4,
        "UTF-8 Unicode",
        Enc::Utf8,
        0x80,
    ),
    cs(
        46,
        "utf8mb4",
        "utf8mb4_bin",
        1,
        4,
        "UTF-8 Unicode",
        Enc::Utf8,
        0x80,
    ),
    cs(47, "latin1", "latin1_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        48,
        "latin1",
        "latin1_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        49,
        "latin1",
        "latin1_general_cs",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        51,
        "cp1251",
        "cp1251_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        14,
        "cp1251",
        "cp1251_bulgarian_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(
        23,
        "cp1251",
        "cp1251_ukrainian_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(50, "cp1251", "cp1251_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        52,
        "cp1251",
        "cp1251_general_cs",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(53, "macroman", "macroman_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        54,
        "utf16",
        "utf16_general_ci",
        2,
        4,
        "UTF-16 Unicode",
        Enc::Utf16,
        0x0,
    ),
    cs(
        55,
        "utf16",
        "utf16_bin",
        2,
        4,
        "UTF-16 Unicode",
        Enc::Utf16,
        0x0,
    ),
    cs(
        56,
        "utf16le",
        "utf16le_general_ci",
        2,
        4,
        "UTF-16LE Unicode",
        Enc::Utf16,
        0x0,
    ),
    cs(58, "cp1257", "cp1257_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        60,
        "utf32",
        "utf32_general_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        61,
        "utf32",
        "utf32_bin",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        62,
        "utf16le",
        "utf16le_bin",
        2,
        4,
        "UTF-16LE Unicode",
        Enc::Utf16,
        0x0,
    ),
    cs(64, "armscii8", "armscii8_bin", 1, 1, "", Enc::Null, 0x100),
    cs(65, "ascii", "ascii_bin", 1, 1, "", Enc::Null, 0x100),
    cs(66, "cp1250", "cp1250_bin", 1, 1, "", Enc::Null, 0x100),
    cs(67, "cp1256", "cp1256_bin", 1, 1, "", Enc::Null, 0x100),
    cs(68, "cp866", "cp866_bin", 1, 1, "", Enc::Null, 0x100),
    cs(69, "dec8", "dec8_bin", 1, 1, "", Enc::Null, 0x100),
    cs(70, "greek", "greek_bin", 1, 1, "", Enc::Null, 0x100),
    cs(71, "hebrew", "hebrew_bin", 1, 1, "", Enc::Null, 0x100),
    cs(72, "hp8", "hp8_bin", 1, 1, "", Enc::Null, 0x100),
    cs(73, "keybcs2", "keybcs2_bin", 1, 1, "", Enc::Null, 0x100),
    cs(74, "koi8r", "koi8r_bin", 1, 1, "", Enc::Null, 0x100),
    cs(75, "koi8u", "koi8u_bin", 1, 1, "", Enc::Null, 0x100),
    cs(77, "latin2", "latin2_bin", 1, 1, "", Enc::Null, 0x100),
    cs(78, "latin5", "latin5_bin", 1, 1, "", Enc::Null, 0x100),
    cs(79, "latin7", "latin7_bin", 1, 1, "", Enc::Null, 0x100),
    cs(80, "cp850", "cp850_bin", 1, 1, "", Enc::Null, 0x100),
    cs(81, "cp852", "cp852_bin", 1, 1, "", Enc::Null, 0x100),
    cs(82, "swe7", "swe7_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        83,
        "utf8",
        "utf8_bin",
        1,
        3,
        "UTF-8 Unicode",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(84, "big5", "big5_bin", 1, 2, "", Enc::Big5, 0xa1),
    cs(85, "euckr", "euckr_bin", 1, 2, "", Enc::Euckr, 0x80),
    cs(86, "gb2312", "gb2312_bin", 1, 2, "", Enc::Gb2312, 0xa1),
    cs(87, "gbk", "gbk_bin", 1, 2, "", Enc::Gbk, 0x80),
    cs(88, "sjis", "sjis_bin", 1, 2, "", Enc::Sjis, 0x80),
    cs(89, "tis620", "tis620_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        90,
        "ucs2",
        "ucs2_bin",
        2,
        2,
        "UCS-2 Unicode",
        Enc::Ucs2,
        0x0,
    ),
    cs(91, "ujis", "ujis_bin", 1, 3, "", Enc::Ujis, 0x80),
    cs(
        92,
        "geostd8",
        "geostd8_general_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(93, "geostd8", "geostd8_bin", 1, 1, "", Enc::Null, 0x100),
    cs(
        94,
        "latin1",
        "latin1_spanish_ci",
        1,
        1,
        "",
        Enc::Null,
        0x100,
    ),
    cs(95, "cp932", "cp932_japanese_ci", 1, 2, "", Enc::Cp932, 0x80),
    cs(96, "cp932", "cp932_bin", 1, 2, "", Enc::Cp932, 0x80),
    cs(
        97,
        "eucjpms",
        "eucjpms_japanese_ci",
        1,
        3,
        "",
        Enc::Eucjpms,
        0x80,
    ),
    cs(98, "eucjpms", "eucjpms_bin", 1, 3, "", Enc::Eucjpms, 0x80),
    cs(99, "cp1250", "cp1250_polish_ci", 1, 1, "", Enc::Null, 0x100),
    cs(128, "ucs2", "ucs2_unicode_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(129, "ucs2", "ucs2_icelandic_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(130, "ucs2", "ucs2_latvian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(131, "ucs2", "ucs2_romanian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(132, "ucs2", "ucs2_slovenian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(133, "ucs2", "ucs2_polish_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(134, "ucs2", "ucs2_estonian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(135, "ucs2", "ucs2_spanish_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(136, "ucs2", "ucs2_swedish_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(137, "ucs2", "ucs2_turkish_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(138, "ucs2", "ucs2_czech_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(139, "ucs2", "ucs2_danish_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(140, "ucs2", "ucs2_lithuanian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(141, "ucs2", "ucs2_slovak_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(142, "ucs2", "ucs2_spanish2_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(143, "ucs2", "ucs2_roman_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(144, "ucs2", "ucs2_persian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(145, "ucs2", "ucs2_esperanto_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(146, "ucs2", "ucs2_hungarian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(147, "ucs2", "ucs2_sinhala_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(148, "ucs2", "ucs2_german2_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(149, "ucs2", "ucs2_croatian_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(150, "ucs2", "ucs2_unicode_520_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(151, "ucs2", "ucs2_vietnamese_ci", 2, 2, "", Enc::Ucs2, 0x0),
    cs(
        160,
        "utf32",
        "utf32_unicode_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        161,
        "utf32",
        "utf32_icelandic_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        162,
        "utf32",
        "utf32_latvian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        163,
        "utf32",
        "utf32_romanian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        164,
        "utf32",
        "utf32_slovenian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        165,
        "utf32",
        "utf32_polish_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        166,
        "utf32",
        "utf32_estonian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        167,
        "utf32",
        "utf32_spanish_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        168,
        "utf32",
        "utf32_swedish_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        169,
        "utf32",
        "utf32_turkish_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        170,
        "utf32",
        "utf32_czech_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        171,
        "utf32",
        "utf32_danish_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        172,
        "utf32",
        "utf32_lithuanian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        173,
        "utf32",
        "utf32_slovak_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        174,
        "utf32",
        "utf32_spanish2_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        175,
        "utf32",
        "utf32_roman_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        176,
        "utf32",
        "utf32_persian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        177,
        "utf32",
        "utf32_esperanto_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        178,
        "utf32",
        "utf32_hungarian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        179,
        "utf32",
        "utf32_sinhala_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        180,
        "utf32",
        "utf32_german2_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        181,
        "utf32",
        "utf32_croatian_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        182,
        "utf32",
        "utf32_unicode_520_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(
        183,
        "utf32",
        "utf32_vietnamese_ci",
        4,
        4,
        "UTF-32 Unicode",
        Enc::Utf32,
        0x0,
    ),
    cs(192, "utf8", "utf8_unicode_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(
        193,
        "utf8",
        "utf8_icelandic_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(194, "utf8", "utf8_latvian_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(
        195,
        "utf8",
        "utf8_romanian_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(
        196,
        "utf8",
        "utf8_slovenian_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(197, "utf8", "utf8_polish_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(
        198,
        "utf8",
        "utf8_estonian_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(199, "utf8", "utf8_spanish_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(200, "utf8", "utf8_swedish_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(201, "utf8", "utf8_turkish_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(202, "utf8", "utf8_czech_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(203, "utf8", "utf8_danish_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(
        204,
        "utf8",
        "utf8_lithuanian_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(205, "utf8", "utf8_slovak_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(
        206,
        "utf8",
        "utf8_spanish2_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(207, "utf8", "utf8_roman_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(208, "utf8", "utf8_persian_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(
        209,
        "utf8",
        "utf8_esperanto_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(
        210,
        "utf8",
        "utf8_hungarian_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(211, "utf8", "utf8_sinhala_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(212, "utf8", "utf8_german2_ci", 1, 3, "", Enc::Utf8mb3, 0x80),
    cs(
        213,
        "utf8",
        "utf8_croatian_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(
        214,
        "utf8",
        "utf8_unicode_520_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(
        215,
        "utf8",
        "utf8_vietnamese_ci",
        1,
        3,
        "",
        Enc::Utf8mb3,
        0x80,
    ),
    cs(
        224,
        "utf8mb4",
        "utf8mb4_unicode_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        225,
        "utf8mb4",
        "utf8mb4_icelandic_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        226,
        "utf8mb4",
        "utf8mb4_latvian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        227,
        "utf8mb4",
        "utf8mb4_romanian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        228,
        "utf8mb4",
        "utf8mb4_slovenian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        229,
        "utf8mb4",
        "utf8mb4_polish_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        230,
        "utf8mb4",
        "utf8mb4_estonian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        231,
        "utf8mb4",
        "utf8mb4_spanish_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        232,
        "utf8mb4",
        "utf8mb4_swedish_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        233,
        "utf8mb4",
        "utf8mb4_turkish_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        234,
        "utf8mb4",
        "utf8mb4_czech_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        235,
        "utf8mb4",
        "utf8mb4_danish_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        236,
        "utf8mb4",
        "utf8mb4_lithuanian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        237,
        "utf8mb4",
        "utf8mb4_slovak_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        238,
        "utf8mb4",
        "utf8mb4_spanish2_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        239,
        "utf8mb4",
        "utf8mb4_roman_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        240,
        "utf8mb4",
        "utf8mb4_persian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        241,
        "utf8mb4",
        "utf8mb4_esperanto_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        242,
        "utf8mb4",
        "utf8mb4_hungarian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        243,
        "utf8mb4",
        "utf8mb4_sinhala_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        244,
        "utf8mb4",
        "utf8mb4_german2_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        245,
        "utf8mb4",
        "utf8mb4_croatian_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        246,
        "utf8mb4",
        "utf8mb4_unicode_520_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        247,
        "utf8mb4",
        "utf8mb4_vietnamese_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        248,
        "gb18030",
        "gb18030_chinese_ci",
        1,
        4,
        "",
        Enc::Gb18030,
        0x0,
    ),
    cs(249, "gb18030", "gb18030_bin", 1, 4, "", Enc::Gb18030, 0x0),
    cs(254, "utf8", "utf8_general_cs", 1, 3, "", Enc::Utf8, 0x80),
    cs(
        255,
        "utf8mb4",
        "utf8mb4_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        256,
        "utf8mb4",
        "utf8mb4_de_pb_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        257,
        "utf8mb4",
        "utf8mb4_is_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        258,
        "utf8mb4",
        "utf8mb4_lv_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        259,
        "utf8mb4",
        "utf8mb4_ro_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        260,
        "utf8mb4",
        "utf8mb4_sl_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        261,
        "utf8mb4",
        "utf8mb4_pl_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        262,
        "utf8mb4",
        "utf8mb4_et_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        263,
        "utf8mb4",
        "utf8mb4_es_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        264,
        "utf8mb4",
        "utf8mb4_sv_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        265,
        "utf8mb4",
        "utf8mb4_tr_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        266,
        "utf8mb4",
        "utf8mb4_cs_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        267,
        "utf8mb4",
        "utf8mb4_da_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        268,
        "utf8mb4",
        "utf8mb4_lt_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        269,
        "utf8mb4",
        "utf8mb4_sk_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        270,
        "utf8mb4",
        "utf8mb4_es_trad_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        271,
        "utf8mb4",
        "utf8mb4_la_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        272,
        "utf8mb4",
        "utf8mb4_fa_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        273,
        "utf8mb4",
        "utf8mb4_eo_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        274,
        "utf8mb4",
        "utf8mb4_hu_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        275,
        "utf8mb4",
        "utf8mb4_hr_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        276,
        "utf8mb4",
        "utf8mb4_si_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        277,
        "utf8mb4",
        "utf8mb4_vi_0900_ai_ci",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        278,
        "utf8mb4",
        "utf8mb4_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        279,
        "utf8mb4",
        "utf8mb4_de_pb_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        280,
        "utf8mb4",
        "utf8mb4_is_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        281,
        "utf8mb4",
        "utf8mb4_lv_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        282,
        "utf8mb4",
        "utf8mb4_ro_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        283,
        "utf8mb4",
        "utf8mb4_sl_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        284,
        "utf8mb4",
        "utf8mb4_pl_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        285,
        "utf8mb4",
        "utf8mb4_et_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        286,
        "utf8mb4",
        "utf8mb4_es_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        287,
        "utf8mb4",
        "utf8mb4_sv_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        288,
        "utf8mb4",
        "utf8mb4_tr_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        289,
        "utf8mb4",
        "utf8mb4_cs_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        290,
        "utf8mb4",
        "utf8mb4_da_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        291,
        "utf8mb4",
        "utf8mb4_lt_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        292,
        "utf8mb4",
        "utf8mb4_sk_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        293,
        "utf8mb4",
        "utf8mb4_es_trad_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        294,
        "utf8mb4",
        "utf8mb4_la_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        295,
        "utf8mb4",
        "utf8mb4_fa_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        296,
        "utf8mb4",
        "utf8mb4_eo_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        297,
        "utf8mb4",
        "utf8mb4_hu_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        298,
        "utf8mb4",
        "utf8mb4_hr_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        299,
        "utf8mb4",
        "utf8mb4_si_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        300,
        "utf8mb4",
        "utf8mb4_vi_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
    cs(
        303,
        "utf8mb4",
        "utf8mb4_ja_0900_as_cs",
        1,
        4,
        "",
        Enc::Utf8,
        0x80,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lookups() {
        assert_eq!(CHARSETS.len(), 246);
        assert_eq!(find_charset_name("utf8mb4").unwrap().nr, 45);
        assert_eq!(find_charset_name("UTF8MB4").unwrap().nr, 45);
        assert_eq!(find_charset_name("utf8").unwrap().nr, 33);
        assert_eq!(find_charset_name("latin1").unwrap().nr, 8);
        assert!(find_charset_name("nope").is_none());
        assert_eq!(find_charset_nr(8).unwrap().name, "latin1");
        assert_eq!(find_charset_nr(5).unwrap().collation, "latin1_german1_ci");
        assert!(find_charset_nr(0).is_none());
        let u = find_charset_nr(255).unwrap();
        assert_eq!(
            (u.name, u.collation, u.char_maxlen),
            ("utf8mb4", "utf8mb4_0900_ai_ci", 4)
        );
        assert!(u.is_multibyte());
        assert!(!find_charset_nr(8).unwrap().is_multibyte());
    }

    #[test]
    fn utf8_escape_slashes() {
        let cs = find_charset_name("utf8mb4").unwrap();
        assert_eq!(
            escape_slashes(cs, b"'\"\\\n\r\0\x1a"),
            b"\\'\\\"\\\\\\n\\r\\0\\Z".to_vec()
        );
        // valid multibyte untouched; invalid lead byte copied (sign-extension
        // makes mbcharlen return 0, so it is not escaped)
        assert_eq!(
            escape_slashes(cs, "é'".as_bytes()),
            "é\\'".as_bytes().to_vec()
        );
        assert_eq!(escape_slashes(cs, b"\xC3'"), b"\xC3\\'".to_vec());
        assert_eq!(
            escape_slashes(cs, "\u{1F600}".as_bytes()),
            "\u{1F600}".as_bytes()
        );
        let mb3 = find_charset_name("utf8").unwrap();
        assert_eq!(mb3.mb_valid("\u{1F600}".as_bytes()), 0);
    }

    #[test]
    fn utf8_escape_quotes() {
        let cs = find_charset_name("utf8mb4").unwrap();
        assert_eq!(escape_quotes(cs, b"a'b\\\"c"), b"a''b\\\"c".to_vec());
    }

    #[test]
    fn big5_second_byte_backslash() {
        let cs = find_charset_name("big5").unwrap();
        // 0xA5 0x5C is a valid big5 char: the 0x5C must not be escaped
        assert_eq!(escape_slashes(cs, b"\xA5\x5C'"), b"\xA5\x5C\\'".to_vec());
        // a lone head byte at the end is a partial sequence: escaped byte-wise
        assert_eq!(escape_slashes(cs, b"\xA5"), b"\\\xA5".to_vec());
        assert_eq!(cs.mb_charlen(0xA5), 2);
        assert_eq!(cs.mb_charlen(0x41), 1);
    }

    #[test]
    fn other_mb_checks() {
        let euckr = find_charset_name("euckr").unwrap();
        assert_eq!(euckr.mb_valid(b"\xB0\xA1"), 2);
        assert_eq!(euckr.mb_valid(b"\xB0"), 0);
        let ujis = find_charset_name("ujis").unwrap();
        assert_eq!(ujis.mb_valid(b"\x8F\xA1\xA1"), 3);
        assert_eq!(ujis.mb_valid(b"\x8E\xA1"), 2);
        let gbk = find_charset_name("gbk").unwrap();
        assert_eq!(escape_slashes(gbk, b"\xBF\x5C"), b"\xBF\x5C".to_vec());
        let gb18030 = find_charset_name("gb18030").unwrap();
        assert_eq!(gb18030.mb_valid(b"\x81\x30\x81\x30"), 4);
        assert_eq!(escape_slashes(gb18030, b"a'"), b"a\\'".to_vec());
        let ucs2 = find_charset_name("ucs2").unwrap();
        assert_eq!(escape_slashes(ucs2, b"\0'\0"), b"\0'\0".to_vec());
    }
}
