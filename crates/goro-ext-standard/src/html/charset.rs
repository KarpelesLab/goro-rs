//! Charsets supported by the html functions (PHP manual, htmlspecialchars:
//! "Supported charsets"; names observed on PHP 8.5).

use super::tables;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charset {
    Utf8,
    Iso8859_1,
    Iso8859_5,
    Iso8859_15,
    Cp866,
    Cp1251,
    Cp1252,
    Koi8R,
    Big5,
    Gb2312,
    Big5Hkscs,
    ShiftJis,
    EucJp,
    MacRoman,
}

/// Accepted names, compared ASCII-case-insensitively (observed on PHP 8.5;
/// the manual's "1251" alias is not accepted).
const NAMES: &[(&str, Charset)] = &[
    ("utf-8", Charset::Utf8),
    ("iso-8859-1", Charset::Iso8859_1),
    ("iso8859-1", Charset::Iso8859_1),
    ("iso-8859-5", Charset::Iso8859_5),
    ("iso8859-5", Charset::Iso8859_5),
    ("iso-8859-15", Charset::Iso8859_15),
    ("iso8859-15", Charset::Iso8859_15),
    ("cp866", Charset::Cp866),
    ("ibm866", Charset::Cp866),
    ("866", Charset::Cp866),
    ("cp1251", Charset::Cp1251),
    ("windows-1251", Charset::Cp1251),
    ("win-1251", Charset::Cp1251),
    ("cp1252", Charset::Cp1252),
    ("windows-1252", Charset::Cp1252),
    ("1252", Charset::Cp1252),
    ("koi8-r", Charset::Koi8R),
    ("koi8-ru", Charset::Koi8R),
    ("koi8r", Charset::Koi8R),
    ("big5", Charset::Big5),
    ("950", Charset::Big5),
    ("gb2312", Charset::Gb2312),
    ("936", Charset::Gb2312),
    ("big5-hkscs", Charset::Big5Hkscs),
    ("shift_jis", Charset::ShiftJis),
    ("sjis", Charset::ShiftJis),
    ("sjis-win", Charset::ShiftJis),
    ("cp932", Charset::ShiftJis),
    ("932", Charset::ShiftJis),
    ("euc-jp", Charset::EucJp),
    ("eucjp", Charset::EucJp),
    ("eucjp-win", Charset::EucJp),
    ("macroman", Charset::MacRoman),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum CjkClass {
    Single,
    Lead,
    Invalid,
}

impl Charset {
    /// Looks a charset name up (without the NUL truncation and default
    /// handling of [`super::resolve_charset`]).
    pub fn from_name(name: &[u8]) -> Option<Charset> {
        NAMES
            .iter()
            .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(name))
            .map(|&(_, c)| c)
    }

    /// The CJK multi-byte charsets, for which only the basic entities exist.
    pub fn is_cjk(self) -> bool {
        matches!(
            self,
            Charset::Big5
                | Charset::Gb2312
                | Charset::Big5Hkscs
                | Charset::ShiftJis
                | Charset::EucJp
        )
    }

    fn sbcs(self) -> Option<&'static [u16; 128]> {
        Some(match self {
            Charset::Iso8859_1 => &tables::ISO_8859_1,
            Charset::Iso8859_5 => &tables::ISO_8859_5,
            Charset::Iso8859_15 => &tables::ISO_8859_15,
            Charset::Cp866 => &tables::CP866,
            Charset::Cp1251 => &tables::CP1251,
            Charset::Cp1252 => &tables::CP1252,
            Charset::Koi8R => &tables::KOI8_R,
            Charset::MacRoman => &tables::MAC_ROMAN,
            _ => return None,
        })
    }

    /// Decodes the character at `s[i]`: `Ok((code point, length))`, where
    /// the code point is `None` for a CJK multi-byte character or an
    /// unmapped single byte; `Err(n)` for an invalid sequence of `n` bytes.
    pub(crate) fn decode(self, s: &[u8], i: usize) -> Result<(Option<u32>, usize), usize> {
        let b = s[i];
        if let Some(t) = self.sbcs() {
            if b < 0x80 {
                return Ok((Some(b as u32), 1));
            }
            let u = t[(b - 0x80) as usize];
            return Ok((if u == 0xFFFF { None } else { Some(u as u32) }, 1));
        }
        let at = |k: usize| s.get(i + k).copied();
        let cont = |k: usize, lo: u8, hi: u8| at(k).is_some_and(|c| (lo..=hi).contains(&c));
        match self {
            Charset::Utf8 => {
                if b < 0x80 {
                    return Ok((Some(b as u32), 1));
                }
                // Invalid sequences (observed on PHP 8.5): the bytes after a
                // lead byte belong to its sequence unless they are ASCII or
                // another lead byte; a complete sequence with a bad byte, an
                // overlong form, a surrogate or a value above U+10FFFF is one
                // error.
                let (n, min) = match b {
                    0xC2..=0xDF => (2, 0x80),
                    0xE0..=0xEF => (3, 0x800),
                    0xF0..=0xF4 => (4, 0x10000),
                    _ => return Err(1),
                };
                let mut bad = false;
                for k in 1..n {
                    match at(k) {
                        Some(0x80..=0xBF) => {}
                        None | Some(0x00..=0x7F | 0xC2..=0xF4) => return Err(k),
                        Some(_) => bad = true,
                    }
                }
                if bad {
                    return Err(n);
                }
                let mut c = (b as u32) & (0x7F >> n);
                for k in 1..n {
                    c = (c << 6) | (s[i + k] as u32 & 0x3F);
                }
                if c < min || (0xD800..=0xDFFF).contains(&c) || c > 0x10FFFF {
                    return Err(n);
                }
                Ok((Some(c), n))
            }
            _ => {
                // CJK charsets (observed on PHP 8.5): a lead byte with a bad
                // trail byte is one error together with the following bytes
                // that cannot start a character.
                let (n, trails_ok) = match (self.cjk_class(b), self) {
                    (CjkClass::Single, _) => {
                        return Ok((if b < 0x80 { Some(b as u32) } else { None }, 1));
                    }
                    (CjkClass::Invalid, _) => return Err(1),
                    (CjkClass::Lead, Charset::Big5 | Charset::Big5Hkscs) => {
                        (2, cont(1, 0x40, 0x7E) || cont(1, 0xA1, 0xFE))
                    }
                    (CjkClass::Lead, Charset::Gb2312) => (2, cont(1, 0xA1, 0xFE)),
                    (CjkClass::Lead, Charset::ShiftJis) => {
                        (2, cont(1, 0x40, 0x7E) || cont(1, 0x80, 0xFC))
                    }
                    (CjkClass::Lead, Charset::EucJp) => match b {
                        0x8E => (2, cont(1, 0xA1, 0xDF)),
                        0x8F => (3, cont(1, 0xA1, 0xFE) && cont(2, 0xA1, 0xFE)),
                        _ => (2, cont(1, 0xA1, 0xFE)),
                    },
                    _ => unreachable!(),
                };
                if trails_ok {
                    return Ok((None, n));
                }
                let mut k = 1;
                // BIG5-HKSCS also takes 0x80 and 0xFF along, which are valid
                // on their own.
                let absorbed = |c: u8| {
                    self.cjk_class(c) == CjkClass::Invalid
                        || (self == Charset::Big5Hkscs && (c == 0x80 || c == 0xFF))
                };
                while k < n && at(k).is_some_and(absorbed) {
                    k += 1;
                }
                Err(k)
            }
        }
    }

    fn cjk_class(self, b: u8) -> CjkClass {
        use CjkClass::*;
        if b < 0x80 {
            return Single;
        }
        match self {
            Charset::Big5 => match b {
                0x81..=0xFE => Lead,
                _ => Single,
            },
            Charset::Big5Hkscs => match b {
                0x81..=0xFE => Lead,
                _ => Single,
            },
            Charset::Gb2312 => match b {
                0xA1..=0xFE => Lead,
                0x8E | 0x8F | 0xA0 | 0xFF => Invalid,
                _ => Single,
            },
            Charset::ShiftJis => match b {
                0x81..=0x9F | 0xE0..=0xFC => Lead,
                0xA1..=0xDF => Single,
                _ => Invalid,
            },
            Charset::EucJp => match b {
                0x8E | 0x8F | 0xA1..=0xFE => Lead,
                0xA0 | 0xFF => Invalid,
                _ => Single,
            },
            _ => unreachable!(),
        }
    }

    /// Appends code point `c` in this charset; false if not representable.
    pub(crate) fn encode_char(self, c: u32, out: &mut Vec<u8>) -> bool {
        if let Some(t) = self.sbcs() {
            if self == Charset::MacRoman && c == 0x7F {
                return false;
            }
            if c < 0x80 {
                out.push(c as u8);
                return true;
            }
            if c == 0xFFFF {
                return false;
            }
            if let Some(p) = t.iter().position(|&u| u as u32 == c) {
                out.push(0x80 + p as u8);
                return true;
            }
            return false;
        }
        match self {
            Charset::Utf8 => match char::from_u32(c) {
                Some(ch) => {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    true
                }
                None => false,
            },
            _ => {
                if c < 0x80 {
                    out.push(c as u8);
                    true
                } else {
                    false
                }
            }
        }
    }
}
