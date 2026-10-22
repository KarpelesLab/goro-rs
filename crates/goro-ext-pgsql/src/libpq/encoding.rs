//! The client encodings libpq knows (pg_wchar.h / encnames.c / wchar.c):
//! names, character lengths and validation.

pub const PG_SQL_ASCII: i32 = 0;
pub const PG_EUC_JP: i32 = 1;
pub const PG_EUC_CN: i32 = 2;
pub const PG_EUC_KR: i32 = 3;
pub const PG_EUC_TW: i32 = 4;
pub const PG_EUC_JIS_2004: i32 = 5;
pub const PG_UTF8: i32 = 6;
pub const PG_MULE_INTERNAL: i32 = 7;
pub const PG_SJIS: i32 = 35;
pub const PG_BIG5: i32 = 36;
pub const PG_GBK: i32 = 37;
pub const PG_UHC: i32 = 38;
pub const PG_GB18030: i32 = 39;
pub const PG_JOHAB: i32 = 40;
pub const PG_SHIFT_JIS_2004: i32 = 41;

/// pg_enc2name_tbl
pub const NAMES: &[&str] = &[
    "SQL_ASCII",
    "EUC_JP",
    "EUC_CN",
    "EUC_KR",
    "EUC_TW",
    "EUC_JIS_2004",
    "UTF8",
    "MULE_INTERNAL",
    "LATIN1",
    "LATIN2",
    "LATIN3",
    "LATIN4",
    "LATIN5",
    "LATIN6",
    "LATIN7",
    "LATIN8",
    "LATIN9",
    "LATIN10",
    "WIN1256",
    "WIN1258",
    "WIN866",
    "WIN874",
    "KOI8R",
    "WIN1251",
    "WIN1252",
    "ISO_8859_5",
    "ISO_8859_6",
    "ISO_8859_7",
    "ISO_8859_8",
    "WIN1250",
    "WIN1253",
    "WIN1254",
    "WIN1255",
    "WIN1257",
    "KOI8U",
    "SJIS",
    "BIG5",
    "GBK",
    "UHC",
    "GB18030",
    "JOHAB",
    "SHIFT_JIS_2004",
];

/// pg_encname_tbl aliases of multibyte encodings (already "cleaned":
/// lowercase alphanumerics)
const ALIASES: &[(&str, i32)] = &[
    ("mskanji", PG_SJIS),
    ("shiftjis", PG_SJIS),
    ("shiftjis2004", PG_SHIFT_JIS_2004),
    ("unicode", PG_UTF8),
    ("win932", PG_SJIS),
    ("win936", PG_GBK),
    ("win949", PG_UHC),
    ("win950", PG_BIG5),
    ("windows932", PG_SJIS),
    ("windows936", PG_GBK),
    ("windows949", PG_UHC),
    ("windows950", PG_BIG5),
];

fn by_name(n: &str) -> Option<i32> {
    NAMES
        .iter()
        .position(|x| x.eq_ignore_ascii_case(n))
        .map(|p| p as i32)
}

/// pg_char_to_encoding
pub fn char_to_encoding(name: &str) -> i32 {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    // canonical names first
    for (i, n) in NAMES.iter().enumerate() {
        let c: String = n
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if c == clean {
            return i as i32;
        }
    }
    let canonical = |alias: &str| -> Option<&'static str> {
        Some(match alias {
            "koi8" | "koi8r" => "KOI8R",
            "koi8u" => "KOI8U",
            "win" | "win1251" | "windows1251" => "WIN1251",
            "abc" | "tcvn" | "tcvn5712" | "vscii" | "win1258" | "windows1258" => "WIN1258",
            "alt" | "win866" | "windows866" => "WIN866",
            "win874" | "windows874" => "WIN874",
            "win1250" | "windows1250" => "WIN1250",
            "win1252" | "windows1252" => "WIN1252",
            "win1253" | "windows1253" => "WIN1253",
            "win1254" | "windows1254" => "WIN1254",
            "win1255" | "windows1255" => "WIN1255",
            "win1256" | "windows1256" => "WIN1256",
            "win1257" | "windows1257" => "WIN1257",
            "iso88595" => "ISO_8859_5",
            "iso88596" => "ISO_8859_6",
            "iso88597" => "ISO_8859_7",
            "iso88598" => "ISO_8859_8",
            "iso88591" => "LATIN1",
            "iso88592" => "LATIN2",
            "iso88593" => "LATIN3",
            "iso88594" => "LATIN4",
            "iso88599" => "LATIN5",
            "iso885910" => "LATIN6",
            "iso885913" => "LATIN7",
            "iso885914" => "LATIN8",
            "iso885915" => "LATIN9",
            "iso885916" => "LATIN10",
            _ => return None,
        })
    };
    if let Some(c) = canonical(&clean) {
        return by_name(c).unwrap_or(-1);
    }
    ALIASES
        .iter()
        .find(|(a, _)| *a == clean)
        .map(|(_, e)| *e)
        .unwrap_or(-1)
}

/// pg_encoding_to_char
pub fn encoding_to_char(enc: i32) -> &'static str {
    if enc >= 0 && (enc as usize) < NAMES.len() {
        NAMES[enc as usize]
    } else {
        ""
    }
}

/// pg_encoding_max_length
pub fn max_length(enc: i32) -> usize {
    match enc {
        PG_UTF8 | PG_EUC_TW | PG_MULE_INTERNAL | PG_GB18030 => 4,
        PG_EUC_JP | PG_EUC_JIS_2004 | PG_JOHAB => 3,
        PG_EUC_CN | PG_EUC_KR | PG_SJIS | PG_BIG5 | PG_GBK | PG_UHC | PG_SHIFT_JIS_2004 => 2,
        _ => 1,
    }
}

/// pg_encoding_mblen: the length of the character starting with `s[0]`
pub fn mblen(enc: i32, s: &[u8]) -> usize {
    let c = match s.first() {
        Some(&c) => c,
        None => return 1,
    };
    if c < 0x80 {
        return 1;
    }
    match enc {
        PG_UTF8 => {
            if c & 0xe0 == 0xc0 {
                2
            } else if c & 0xf0 == 0xe0 {
                3
            } else if c & 0xf8 == 0xf0 {
                4
            } else {
                1
            }
        }
        PG_EUC_JP | PG_EUC_JIS_2004 => {
            if c == 0x8e {
                2
            } else if c == 0x8f {
                3
            } else {
                2
            }
        }
        PG_EUC_CN | PG_EUC_KR | PG_BIG5 | PG_GBK | PG_UHC => 2,
        PG_EUC_TW => {
            if c == 0x8e {
                4
            } else if c == 0x8f {
                3
            } else {
                2
            }
        }
        PG_SJIS | PG_SHIFT_JIS_2004 => {
            if (0xa1..=0xdf).contains(&c) {
                1
            } else {
                2
            }
        }
        PG_GB18030 => match s.get(1) {
            Some(&d) if (0x30..=0x39).contains(&d) => 4,
            _ => 2,
        },
        PG_JOHAB => {
            if c == 0x8e || c == 0x8f {
                3
            } else {
                2
            }
        }
        PG_MULE_INTERNAL => {
            if (0x81..=0x8d).contains(&c) {
                2
            } else if (0x90..=0x9b).contains(&c) {
                3
            } else if c == 0x9c || c == 0x9d {
                4
            } else {
                1
            }
        }
        _ => 1,
    }
}

/// pg_encoding_verifymbchar: the character's length, or None if invalid.
pub fn verify_mbchar(enc: i32, s: &[u8]) -> Option<usize> {
    let l = mblen(enc, s);
    if s.len() < l {
        return None;
    }
    if l == 1 {
        if enc == PG_UTF8 && s[0] >= 0x80 {
            return None;
        }
        return Some(1);
    }
    match enc {
        PG_UTF8 => {
            let c = &s[..l];
            if c[1..].iter().any(|&b| b & 0xc0 != 0x80) {
                return None;
            }
            // overlongs and surrogates (pg_utf8_islegal)
            match l {
                2 => {
                    if c[0] < 0xc2 {
                        return None;
                    }
                }
                3 => {
                    if c[0] == 0xe0 && c[1] < 0xa0 {
                        return None;
                    }
                    if c[0] == 0xed && c[1] > 0x9f {
                        return None;
                    }
                }
                4 => {
                    if c[0] == 0xf0 && c[1] < 0x90 {
                        return None;
                    }
                    if c[0] > 0xf4 || (c[0] == 0xf4 && c[1] > 0x8f) {
                        return None;
                    }
                }
                _ => {}
            }
            Some(l)
        }
        _ => {
            // multibyte trail bytes must have the high bit set (EUC) or be
            // non-NUL (SJIS & co.)
            if s[1..l].contains(&0) {
                return None;
            }
            Some(l)
        }
    }
}

/// pg_encoding_verifymbstr: the length of the valid prefix
pub fn verify_mbstr(enc: i32, s: &[u8]) -> usize {
    let mut i = 0;
    while i < s.len() {
        if s[i] == 0 {
            return i;
        }
        if s[i] < 0x80 {
            i += 1;
            continue;
        }
        match verify_mbchar(enc, &s[i..]) {
            Some(l) => i += l,
            None => return i,
        }
    }
    i
}

/// pg_encoding_dsplen: the display width of the character
pub fn dsplen(enc: i32, s: &[u8]) -> usize {
    let c = match s.first() {
        Some(&c) => c,
        None => return 1,
    };
    if c < 0x20 || c == 0x7f {
        return 0;
    }
    if c < 0x80 {
        return 1;
    }
    if enc == PG_UTF8 {
        let l = mblen(enc, s).min(s.len());
        let cp = match std::str::from_utf8(&s[..l]) {
            Ok(t) => t.chars().next().map(|c| c as u32).unwrap_or(0),
            Err(_) => return 1,
        };
        // East Asian wide characters (ucs_wcwidth's wide ranges)
        let wide = (0x1100..=0x115f).contains(&cp)
            || (0x2e80..=0xa4cf).contains(&cp) && cp != 0x303f
            || (0xac00..=0xd7a3).contains(&cp)
            || (0xf900..=0xfaff).contains(&cp)
            || (0xfe30..=0xfe6f).contains(&cp)
            || (0xff00..=0xff60).contains(&cp)
            || (0xffe0..=0xffe6).contains(&cp)
            || (0x20000..=0x3fffd).contains(&cp);
        return if wide { 2 } else { 1 };
    }
    if max_length(enc) > 1 { 2 } else { 1 }
}
