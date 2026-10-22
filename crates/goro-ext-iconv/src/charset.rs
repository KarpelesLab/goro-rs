//! The character sets of the iconv emulation: glibc's charset names and
//! one-character decoders / encoders with glibc's error semantics
//! (EILSEQ for an illegal input sequence or an unconvertible character,
//! EINVAL for an incomplete sequence at the end of the input). The CJK and
//! legacy single-byte code tables come from the mbstring extension's libmbfl
//! port.

use goro_ext_mbstring::mbfl::{self, EncNo, Encoding, MBFL_BAD_INPUT};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Endian {
    Be,
    Le,
}

/// How a multi-byte character set delimits its characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// One byte per character.
    Sbcs,
    /// 0x8E: 2 bytes, 0x8F: 3 bytes, 0xA1-0xFE: 2 bytes.
    EucJp,
    /// 0x81-0x9F, 0xE0-0xFC: 2 bytes.
    Sjis,
    /// 0x81-0xFE: 2 bytes.
    Dbcs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Utf8,
    Ascii,
    Latin1,
    /// UCS-2 (no BOM).
    Ucs2(Endian),
    /// UCS-4 / UTF-32BE / UTF-32LE (no BOM); `utf` rejects non-Unicode
    /// values.
    Ucs4(Endian, bool),
    /// UTF-16 with a BOM (decode: detected, default little endian; encode:
    /// BOM + little endian), or UTF-16BE / UTF-16LE.
    Utf16(Option<Endian>),
    /// UTF-32 with a BOM.
    Utf32Bom,
    Iso2022Jp,
    Cp1258,
    Mbfl(EncNo, Family),
}

/// glibc charset names (and common aliases) to charsets. Names are
/// compared case-insensitively.
fn table() -> &'static [(&'static str, Kind)] {
    use Endian::*;
    use Family::*;
    use Kind::*;
    &[
        ("UTF-8", Utf8),
        ("UTF8", Utf8),
        ("ISO-10646/UTF8", Utf8),
        ("ISO-10646/UTF-8", Utf8),
        ("ASCII", Ascii),
        ("US-ASCII", Ascii),
        ("ANSI_X3.4-1968", Ascii),
        ("ANSI_X3.4-1986", Ascii),
        ("ISO646-US", Ascii),
        ("ISO_646.IRV:1991", Ascii),
        ("US", Ascii),
        ("IBM367", Ascii),
        ("CP367", Ascii),
        ("CSASCII", Ascii),
        ("646", Ascii),
        ("ISO-8859-1", Latin1),
        ("ISO8859-1", Latin1),
        ("ISO_8859-1", Latin1),
        ("ISO_8859-1:1987", Latin1),
        ("ISO88591", Latin1),
        ("LATIN1", Latin1),
        ("L1", Latin1),
        ("IBM819", Latin1),
        ("CP819", Latin1),
        ("CSISOLATIN1", Latin1),
        ("UCS-2", Ucs2(Le)),
        ("ISO-10646-UCS-2", Ucs2(Le)),
        ("CSUNICODE", Ucs2(Le)),
        ("UCS-2BE", Ucs2(Be)),
        ("UNICODEBIG", Ucs2(Be)),
        ("UCS-2LE", Ucs2(Le)),
        ("UNICODELITTLE", Ucs2(Le)),
        ("UCS-4", Ucs4(Be, false)),
        ("UCS4", Ucs4(Be, false)),
        ("ISO-10646", Ucs4(Be, false)),
        ("ISO-10646-UCS-4", Ucs4(Be, false)),
        ("CSUCS4", Ucs4(Be, false)),
        ("UCS-4BE", Ucs4(Be, false)),
        ("UCS-4LE", Ucs4(Le, false)),
        ("WCHAR_T", Ucs4(Le, false)),
        ("INTERNAL", Ucs4(Le, false)),
        ("UTF-32BE", Ucs4(Be, true)),
        ("UTF-32LE", Ucs4(Le, true)),
        ("UTF-32", Utf32Bom),
        ("UTF32", Utf32Bom),
        ("UTF-16", Utf16(None)),
        ("UTF16", Utf16(None)),
        ("UTF-16BE", Utf16(Some(Be))),
        ("UTF-16LE", Utf16(Some(Le))),
        ("ISO-2022-JP", Iso2022Jp),
        ("CSISO2022JP", Iso2022Jp),
        ("ISO2022JP", Iso2022Jp),
        ("CP1258", Cp1258),
        ("WINDOWS-1258", Cp1258),
        ("MS-VIET", Cp1258),
        ("EUC-JP", Mbfl(EncNo::EucJp, EucJp)),
        ("EUCJP", Mbfl(EncNo::EucJp, EucJp)),
        ("UJIS", Mbfl(EncNo::EucJp, EucJp)),
        ("CSEUCPKDFMTJAPANESE", Mbfl(EncNo::EucJp, EucJp)),
        ("EUC-JP-MS", Mbfl(EncNo::EucJpWin, EucJp)),
        ("EUCJP-MS", Mbfl(EncNo::EucJpWin, EucJp)),
        ("EUCJP-WIN", Mbfl(EncNo::EucJpWin, EucJp)),
        ("SJIS", Mbfl(EncNo::Sjis, Sjis)),
        ("SHIFT_JIS", Mbfl(EncNo::Sjis, Sjis)),
        ("SHIFT-JIS", Mbfl(EncNo::Sjis, Sjis)),
        ("MS_KANJI", Mbfl(EncNo::Sjis, Sjis)),
        ("CSSHIFTJIS", Mbfl(EncNo::Sjis, Sjis)),
        ("CP932", Mbfl(EncNo::Cp932, Sjis)),
        ("WINDOWS-31J", Mbfl(EncNo::Cp932, Sjis)),
        ("MS932", Mbfl(EncNo::Cp932, Sjis)),
        ("IBM-943", Mbfl(EncNo::Cp932, Sjis)),
        ("EUC-KR", Mbfl(EncNo::EucKr, Dbcs)),
        ("EUCKR", Mbfl(EncNo::EucKr, Dbcs)),
        ("CSEUCKR", Mbfl(EncNo::EucKr, Dbcs)),
        ("UHC", Mbfl(EncNo::Uhc, Dbcs)),
        ("CP949", Mbfl(EncNo::Uhc, Dbcs)),
        ("BIG5", Mbfl(EncNo::Big5, Dbcs)),
        ("BIG-5", Mbfl(EncNo::Big5, Dbcs)),
        ("BIG-FIVE", Mbfl(EncNo::Big5, Dbcs)),
        ("CN-BIG5", Mbfl(EncNo::Big5, Dbcs)),
        ("CSBIG5", Mbfl(EncNo::Big5, Dbcs)),
        ("CP950", Mbfl(EncNo::Cp950, Dbcs)),
        ("GB2312", Mbfl(EncNo::EucCn, Dbcs)),
        ("EUC-CN", Mbfl(EncNo::EucCn, Dbcs)),
        ("EUCCN", Mbfl(EncNo::EucCn, Dbcs)),
        ("CSGB2312", Mbfl(EncNo::EucCn, Dbcs)),
        ("GBK", Mbfl(EncNo::Cp936, Dbcs)),
        ("CP936", Mbfl(EncNo::Cp936, Dbcs)),
        ("MS936", Mbfl(EncNo::Cp936, Dbcs)),
        ("WINDOWS-936", Mbfl(EncNo::Cp936, Dbcs)),
        ("CP1252", Mbfl(EncNo::Cp1252, Sbcs)),
        ("WINDOWS-1252", Mbfl(EncNo::Cp1252, Sbcs)),
        ("MS-ANSI", Mbfl(EncNo::Cp1252, Sbcs)),
        ("CP1254", Mbfl(EncNo::Cp1254, Sbcs)),
        ("WINDOWS-1254", Mbfl(EncNo::Cp1254, Sbcs)),
        ("MS-TURK", Mbfl(EncNo::Cp1254, Sbcs)),
        ("CP1251", Mbfl(EncNo::Cp1251, Sbcs)),
        ("WINDOWS-1251", Mbfl(EncNo::Cp1251, Sbcs)),
        ("MS-CYRL", Mbfl(EncNo::Cp1251, Sbcs)),
        ("CP866", Mbfl(EncNo::Cp866, Sbcs)),
        ("IBM866", Mbfl(EncNo::Cp866, Sbcs)),
        ("866", Mbfl(EncNo::Cp866, Sbcs)),
        ("CSIBM866", Mbfl(EncNo::Cp866, Sbcs)),
        ("CP850", Mbfl(EncNo::Cp850, Sbcs)),
        ("IBM850", Mbfl(EncNo::Cp850, Sbcs)),
        ("850", Mbfl(EncNo::Cp850, Sbcs)),
        ("CSPC850MULTILINGUAL", Mbfl(EncNo::Cp850, Sbcs)),
        ("KOI8-R", Mbfl(EncNo::Koi8r, Sbcs)),
        ("CSKOI8R", Mbfl(EncNo::Koi8r, Sbcs)),
        ("KOI8-U", Mbfl(EncNo::Koi8u, Sbcs)),
        ("ARMSCII-8", Mbfl(EncNo::Armscii8, Sbcs)),
        ("ISO-8859-2", Mbfl(EncNo::Iso8859_2, Sbcs)),
        ("ISO-8859-3", Mbfl(EncNo::Iso8859_3, Sbcs)),
        ("ISO-8859-4", Mbfl(EncNo::Iso8859_4, Sbcs)),
        ("ISO-8859-5", Mbfl(EncNo::Iso8859_5, Sbcs)),
        ("ISO-8859-6", Mbfl(EncNo::Iso8859_6, Sbcs)),
        ("ISO-8859-7", Mbfl(EncNo::Iso8859_7, Sbcs)),
        ("ISO-8859-8", Mbfl(EncNo::Iso8859_8, Sbcs)),
        ("ISO-8859-9", Mbfl(EncNo::Iso8859_9, Sbcs)),
        ("ISO-8859-10", Mbfl(EncNo::Iso8859_10, Sbcs)),
        ("ISO-8859-13", Mbfl(EncNo::Iso8859_13, Sbcs)),
        ("ISO-8859-14", Mbfl(EncNo::Iso8859_14, Sbcs)),
        ("ISO-8859-15", Mbfl(EncNo::Iso8859_15, Sbcs)),
        ("ISO-8859-16", Mbfl(EncNo::Iso8859_16, Sbcs)),
        ("LATIN2", Mbfl(EncNo::Iso8859_2, Sbcs)),
        ("LATIN3", Mbfl(EncNo::Iso8859_3, Sbcs)),
        ("LATIN4", Mbfl(EncNo::Iso8859_4, Sbcs)),
        ("CYRILLIC", Mbfl(EncNo::Iso8859_5, Sbcs)),
        ("ARABIC", Mbfl(EncNo::Iso8859_6, Sbcs)),
        ("GREEK", Mbfl(EncNo::Iso8859_7, Sbcs)),
        ("HEBREW", Mbfl(EncNo::Iso8859_8, Sbcs)),
        ("LATIN5", Mbfl(EncNo::Iso8859_9, Sbcs)),
        ("LATIN6", Mbfl(EncNo::Iso8859_10, Sbcs)),
        ("LATIN7", Mbfl(EncNo::Iso8859_13, Sbcs)),
        ("LATIN8", Mbfl(EncNo::Iso8859_14, Sbcs)),
        ("LATIN-9", Mbfl(EncNo::Iso8859_15, Sbcs)),
        ("LATIN9", Mbfl(EncNo::Iso8859_15, Sbcs)),
        ("LATIN10", Mbfl(EncNo::Iso8859_16, Sbcs)),
    ]
}

/// The charset named `name` (glibc's normalization: case-insensitive;
/// ISO_8859-N, ISO8859-N, ISO88591... spellings).
pub fn lookup(name: &[u8]) -> Option<Kind> {
    let up: Vec<u8> = name.to_ascii_uppercase();
    for &(n, k) in table() {
        if n.as_bytes() == up.as_slice() {
            return Some(k);
        }
    }
    // ISO_8859-N / ISO8859-N / ISO88592 ...
    let compact: Vec<u8> = up
        .iter()
        .copied()
        .filter(|c| *c != b'-' && *c != b'_')
        .collect();
    for &(n, k) in table() {
        let nc: Vec<u8> = n.bytes().filter(|c| *c != b'-' && *c != b'_').collect();
        if nc == compact {
            return Some(k);
        }
    }
    None
}

/// The result of decoding from the start of a (non-empty) input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dec {
    /// A character and the bytes it took.
    Char(u32, usize),
    /// Bytes consumed without a character (escape sequences, a BOM).
    Shift(usize),
    /// An illegal sequence; //IGNORE skips this many bytes.
    Illegal(usize),
    /// An incomplete sequence at the end of the input.
    Incomplete,
}

fn mbfl_enc(no: EncNo) -> Option<&'static Encoding> {
    mbfl::no2encoding(no)
}

/// One character through an mbfl decoder.
fn mbfl_decode_char(no: EncNo, bytes: &[u8]) -> Option<u32> {
    let enc = mbfl_enc(no)?;
    let out = enc.decode(bytes);
    match out.as_slice() {
        [cp] if *cp != MBFL_BAD_INPUT => Some(*cp),
        _ => None,
    }
}

/// One character through an mbfl encoder.
fn mbfl_encode_char(no: EncNo, cp: u32) -> Option<Vec<u8>> {
    // glibc's tables where they differ from libmbfl's.
    match (no, cp) {
        (EncNo::EucJp | EncNo::Sjis | EncNo::Cp932, 0xA5) => return Some(vec![0x5C]),
        (EncNo::EucJp | EncNo::Sjis | EncNo::Cp932, 0x203E) => return Some(vec![0x7E]),
        (EncNo::EucJp, 0xFF5E) => return Some(vec![0x8F, 0xA2, 0xB7]),
        (EncNo::Cp932, 0x2014) => return Some(vec![0x81, 0x5C]),
        (EncNo::Big5 | EncNo::Cp950, 0x20AC) => return Some(vec![0xA3, 0xE1]),
        (EncNo::EucJp, 0x80..=0x8D | 0x90..=0x9F) | (EncNo::EucKr, 0x80..=0x9F) => {
            return Some(vec![cp as u8]);
        }
        _ => {}
    }
    let enc = mbfl_enc(no)?;
    let (bytes, errors) = enc.encode(&[cp], '?' as u32, mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE);
    if errors > 0 || bytes.is_empty() {
        return None;
    }
    // libmbfl also accepts compatibility characters (e.g. U+FF5E for
    // 0xA1C1 in EUC-JP); glibc only converts what decodes back to the same
    // character.
    if cp >= 0x80 && mbfl_decode_char_glibc(no, &bytes) != Some(cp) {
        return None;
    }
    Some(bytes)
}

/// mbfl_decode_char() with glibc's mapping differences.
fn mbfl_decode_char_glibc(no: EncNo, bytes: &[u8]) -> Option<u32> {
    match (no, bytes) {
        (EncNo::Sjis, [0x5C]) => return Some(0xA5),
        (EncNo::Sjis, [0x7E]) => return Some(0x203E),
        (EncNo::EucJp, [0x8F, 0xA2, 0xB7]) => return Some(0xFF5E),
        (EncNo::Big5 | EncNo::Cp950, [0xA3, 0xE1]) => return Some(0x20AC),
        _ => {}
    }
    mbfl_decode_char(no, bytes)
}

/// CP1258 0x80-0xFF (0 = undefined).
const CP1258_HIGH: [u16; 128] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0, 0x2039, 0x0152,
    0, 0, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0, 0x203A,
    0x0153, 0, 0, 0x0178, 0x00A0, 0x00A1, 0x00A2, 0x00A3, 0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8,
    0x00A9, 0x00AA, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x00AF, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4,
    0x00B5, 0x00B6, 0x00B7, 0x00B8, 0x00B9, 0x00BA, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x00BF, 0x00C0,
    0x00C1, 0x00C2, 0x0102, 0x00C4, 0x00C5, 0x00C6, 0x00C7, 0x00C8, 0x00C9, 0x00CA, 0x00CB, 0x0300,
    0x00CD, 0x00CE, 0x00CF, 0x0110, 0x00D1, 0x0309, 0x00D3, 0x00D4, 0x01A0, 0x00D6, 0x00D7, 0x00D8,
    0x00D9, 0x00DA, 0x00DB, 0x00DC, 0x01AF, 0x0303, 0x00DF, 0x00E0, 0x00E1, 0x00E2, 0x0103, 0x00E4,
    0x00E5, 0x00E6, 0x00E7, 0x00E8, 0x00E9, 0x00EA, 0x00EB, 0x0301, 0x00ED, 0x00EE, 0x00EF, 0x0111,
    0x00F1, 0x0323, 0x00F3, 0x00F4, 0x01A1, 0x00F6, 0x00F7, 0x00F8, 0x00F9, 0x00FA, 0x00FB, 0x00FC,
    0x01B0, 0x20AB, 0x00FF,
];

/// A decoder with its shift state.
#[derive(Clone, Copy, Debug)]
pub struct Decoder {
    pub kind: Kind,
    /// ISO-2022-JP: the designated set (0 ASCII, 1 JIS X 0201 Roman,
    /// 2 JIS X 0208); UTF-16 / UTF-32: 0 = BOM not seen, 1 = BE, 2 = LE.
    state: u8,
}

impl Decoder {
    pub fn new(kind: Kind) -> Decoder {
        Decoder { kind, state: 0 }
    }

    pub fn reset(&mut self) {
        self.state = 0;
    }

    pub fn decode(&mut self, s: &[u8]) -> Dec {
        match self.kind {
            Kind::Utf8 => decode_utf8(s),
            Kind::Ascii => {
                if s[0] < 0x80 {
                    Dec::Char(s[0] as u32, 1)
                } else {
                    Dec::Illegal(1)
                }
            }
            Kind::Latin1 => Dec::Char(s[0] as u32, 1),
            Kind::Ucs2(e) => {
                if s.len() < 2 {
                    return Dec::Incomplete;
                }
                let c = u16_at(s, e) as u32;
                if (0xD800..0xE000).contains(&c) {
                    return Dec::Illegal(2);
                }
                Dec::Char(c, 2)
            }
            Kind::Ucs4(e, utf) => {
                if s.len() < 4 {
                    return Dec::Incomplete;
                }
                let c = u32_at(s, e);
                if c > 0x7fff_ffff || (utf && (c > 0x10ffff || (0xD800..0xE000).contains(&c))) {
                    return Dec::Illegal(4);
                }
                Dec::Char(c, 4)
            }
            Kind::Utf16(fixed) => {
                if s.len() < 2 {
                    return Dec::Incomplete;
                }
                let e = match fixed {
                    Some(e) => e,
                    None => {
                        if self.state == 0 {
                            // Without a BOM: little endian (host order).
                            self.state = 2;
                            if s[0] == 0xFE && s[1] == 0xFF {
                                self.state = 1;
                                return Dec::Shift(2);
                            }
                            if s[0] == 0xFF && s[1] == 0xFE {
                                return Dec::Shift(2);
                            }
                        }
                        if self.state == 1 {
                            Endian::Be
                        } else {
                            Endian::Le
                        }
                    }
                };
                let c = u16_at(s, e) as u32;
                if (0xD800..0xDC00).contains(&c) {
                    if s.len() < 4 {
                        return Dec::Incomplete;
                    }
                    let c2 = u16_at(&s[2..], e) as u32;
                    if !(0xDC00..0xE000).contains(&c2) {
                        return Dec::Illegal(2);
                    }
                    return Dec::Char(0x10000 + ((c - 0xD800) << 10) + (c2 - 0xDC00), 4);
                }
                if (0xDC00..0xE000).contains(&c) {
                    return Dec::Illegal(2);
                }
                Dec::Char(c, 2)
            }
            Kind::Utf32Bom => {
                if s.len() < 4 {
                    return Dec::Incomplete;
                }
                if self.state == 0 {
                    self.state = 2;
                    if s[..4] == [0, 0, 0xFE, 0xFF] {
                        self.state = 1;
                        return Dec::Shift(4);
                    }
                    if s[..4] == [0xFF, 0xFE, 0, 0] {
                        return Dec::Shift(4);
                    }
                }
                let e = if self.state == 1 {
                    Endian::Be
                } else {
                    Endian::Le
                };
                let c = u32_at(s, e);
                if c > 0x10ffff || (0xD800..0xE000).contains(&c) {
                    return Dec::Illegal(4);
                }
                Dec::Char(c, 4)
            }
            Kind::Cp1258 => {
                if s[0] < 0x80 {
                    return Dec::Char(s[0] as u32, 1);
                }
                match CP1258_HIGH[(s[0] - 0x80) as usize] {
                    0 => Dec::Illegal(1),
                    c => Dec::Char(c as u32, 1),
                }
            }
            Kind::Iso2022Jp => self.decode_2022jp(s),
            Kind::Mbfl(no, fam) => {
                let c = s[0];
                let n = match fam {
                    Family::Sbcs => 1,
                    Family::EucJp => match c {
                        0x8E => 2,
                        0x8F => 3,
                        0xA1..=0xFE => 2,
                        _ => 1,
                    },
                    Family::Sjis => match c {
                        0x81..=0x9F | 0xE0..=0xFC => 2,
                        _ => 1,
                    },
                    Family::Dbcs => match c {
                        0x81..=0xFE => 2,
                        _ => 1,
                    },
                };
                // glibc's EUC-JP and EUC-KR pass the C1 controls through.
                if (0x80..=0x9F).contains(&c)
                    && ((fam == Family::EucJp && c != 0x8E && c != 0x8F) || no == EncNo::EucKr)
                {
                    return Dec::Char(c as u32, 1);
                }
                if s.len() < n {
                    return Dec::Incomplete;
                }
                let pua = |cp: u32| (0xE000..0xF900).contains(&cp);
                match mbfl_decode_char_glibc(no, &s[..n]) {
                    // glibc's tables of these charsets have no
                    // private-use mappings.
                    Some(cp)
                        if pua(cp)
                            && matches!(
                                no,
                                EncNo::EucJp
                                    | EncNo::Sjis
                                    | EncNo::EucKr
                                    | EncNo::Uhc
                                    | EncNo::EucCn
                                    | EncNo::Cp936
                                    | EncNo::Big5
                            ) =>
                    {
                        Dec::Illegal(1)
                    }
                    Some(cp) => Dec::Char(cp, n),
                    // //IGNORE skips the lead byte, or the pair for some
                    // charsets when the trail byte is in range.
                    None => {
                        let t = s.get(1).copied().unwrap_or(0);
                        let pair = n == 2
                            && match no {
                                EncNo::EucKr | EncNo::Uhc => true,
                                EncNo::Cp936 => (0x40..=0xFE).contains(&t) && t != 0x7F,
                                EncNo::Cp932 => {
                                    matches!(c, 0x81..=0x9F | 0xE0..=0xEF)
                                        && (0x40..=0xFC).contains(&t)
                                        && t != 0x7F
                                }
                                _ => false,
                            };
                        Dec::Illegal(if pair { 2 } else { 1 })
                    }
                }
            }
        }
    }

    fn decode_2022jp(&mut self, s: &[u8]) -> Dec {
        if s[0] == 0x1B {
            if s.len() < 3 {
                return Dec::Incomplete;
            }
            match (s[1], s[2]) {
                (b'(', b'B') => self.state = 0,
                (b'(', b'J') => self.state = 1,
                (b'$', b'@') | (b'$', b'B') => self.state = 2,
                _ => return Dec::Illegal(1),
            }
            return Dec::Shift(3);
        }
        if s[0] >= 0x80 {
            return Dec::Illegal(1);
        }
        match self.state {
            0 => Dec::Char(s[0] as u32, 1),
            1 => Dec::Char(
                match s[0] {
                    0x5C => 0xA5,
                    0x7E => 0x203E,
                    c => c as u32,
                },
                1,
            ),
            _ => {
                if s[0] < 0x21 || s[0] == 0x7F {
                    // Control characters pass through.
                    return Dec::Char(s[0] as u32, 1);
                }
                if s.len() < 2 {
                    return Dec::Incomplete;
                }
                if !(0x21..=0x7E).contains(&s[1]) {
                    return Dec::Illegal(1);
                }
                match mbfl_decode_char(EncNo::EucJp, &[s[0] | 0x80, s[1] | 0x80]) {
                    Some(cp) => Dec::Char(cp, 2),
                    None => Dec::Illegal(2),
                }
            }
        }
    }
}

fn u16_at(s: &[u8], e: Endian) -> u16 {
    match e {
        Endian::Be => u16::from_be_bytes([s[0], s[1]]),
        Endian::Le => u16::from_le_bytes([s[0], s[1]]),
    }
}

fn u32_at(s: &[u8], e: Endian) -> u32 {
    let b = [s[0], s[1], s[2], s[3]];
    match e {
        Endian::Be => u32::from_be_bytes(b),
        Endian::Le => u32::from_le_bytes(b),
    }
}

/// glibc's UTF-8 decoder (gconv_simple.c, from utf8 loop).
fn decode_utf8(s: &[u8]) -> Dec {
    let c = s[0];
    if c < 0x80 {
        return Dec::Char(c as u32, 1);
    }
    let (cnt, mask) = match c {
        0xC2..=0xDF => (2, 0x1F),
        0xE0..=0xEF => (3, 0x0F),
        0xF0..=0xF7 => (4, 0x07),
        // glibc still decodes the 5- and 6-byte forms (up to 0x7FFFFFFF).
        0xF8..=0xFB => (5, 0x03),
        0xFC..=0xFD => (6, 0x01),
        _ => {
            // An illegal lead byte: skip it and the continuation bytes
            // after it.
            let mut i = 1;
            while i < s.len() && s[i] & 0xC0 == 0x80 && i < 5 {
                i += 1;
            }
            return Dec::Illegal(i);
        }
    };
    if s.len() < cnt {
        let mut i = 1;
        while i < s.len() && s[i] & 0xC0 == 0x80 {
            i += 1;
        }
        if i == s.len() {
            return Dec::Incomplete;
        }
        return Dec::Illegal(i);
    }
    let mut ch = (c & mask) as u32;
    let mut i = 1;
    while i < cnt {
        let b = s[i];
        if b & 0xC0 != 0x80 {
            break;
        }
        ch = (ch << 6) | (b & 0x3F) as u32;
        i += 1;
    }
    if i < cnt || (cnt > 2 && (ch >> (5 * cnt - 4)) == 0) || (0xD800..=0xDFFF).contains(&ch) {
        return Dec::Illegal(i);
    }
    Dec::Char(ch, cnt)
}

/// An encoder with its shift state.
#[derive(Clone, Copy, Debug)]
pub struct Encoder {
    pub kind: Kind,
    /// ISO-2022-JP: designated set; UTF-16 / UTF-32: BOM written.
    state: u8,
}

impl Encoder {
    pub fn new(kind: Kind) -> Encoder {
        Encoder { kind, state: 0 }
    }

    /// Bytes of `cp` appended to `out`; false when it cannot be
    /// represented.
    pub fn encode(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        match self.kind {
            Kind::Utf8 => {
                if (0xD800..0xE000).contains(&cp) || cp > 0x7fff_ffff {
                    return false;
                }
                push_utf8(out, cp);
                true
            }
            Kind::Ascii => {
                if cp < 0x80 {
                    out.push(cp as u8);
                    true
                } else {
                    false
                }
            }
            Kind::Latin1 => {
                if cp < 0x100 {
                    out.push(cp as u8);
                    true
                } else {
                    false
                }
            }
            Kind::Ucs2(e) => {
                if cp >= 0x10000 || (0xD800..0xE000).contains(&cp) {
                    return false;
                }
                push_u16(out, cp as u16, e);
                true
            }
            Kind::Ucs4(e, utf) => {
                if utf && (cp > 0x10ffff || (0xD800..0xE000).contains(&cp)) {
                    return false;
                }
                push_u32(out, cp, e);
                true
            }
            Kind::Utf16(fixed) => {
                if cp > 0x10ffff || (0xD800..0xE000).contains(&cp) {
                    return false;
                }
                let e = match fixed {
                    Some(e) => e,
                    None => {
                        if self.state == 0 {
                            self.state = 1;
                            push_u16(out, 0xFEFF, Endian::Le);
                        }
                        Endian::Le
                    }
                };
                if cp >= 0x10000 {
                    let v = cp - 0x10000;
                    push_u16(out, 0xD800 + (v >> 10) as u16, e);
                    push_u16(out, 0xDC00 + (v & 0x3FF) as u16, e);
                } else {
                    push_u16(out, cp as u16, e);
                }
                true
            }
            Kind::Utf32Bom => {
                if cp > 0x10ffff || (0xD800..0xE000).contains(&cp) {
                    return false;
                }
                if self.state == 0 {
                    self.state = 1;
                    push_u32(out, 0xFEFF, Endian::Le);
                }
                push_u32(out, cp, Endian::Le);
                true
            }
            Kind::Cp1258 => {
                if cp < 0x80 {
                    out.push(cp as u8);
                    return true;
                }
                match CP1258_HIGH.iter().position(|&c| c != 0 && c as u32 == cp) {
                    Some(i) => {
                        out.push(0x80 + i as u8);
                        true
                    }
                    None => false,
                }
            }
            Kind::Iso2022Jp => self.encode_2022jp(cp, out),
            Kind::Mbfl(no, _) => match mbfl_encode_char(no, cp) {
                Some(b) => {
                    out.extend_from_slice(&b);
                    true
                }
                None => false,
            },
        }
    }

    fn encode_2022jp(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        let set = |st: &mut u8, want: u8, out: &mut Vec<u8>| {
            if *st != want {
                out.extend_from_slice(match want {
                    0 => b"\x1b(B",
                    1 => b"\x1b(J",
                    _ => b"\x1b$B",
                });
                *st = want;
            }
        };
        if cp < 0x80 {
            // JIS X 0201 Roman has the same characters except \ and ~.
            if self.state == 1 && cp != 0x5C && cp != 0x7E {
                out.push(cp as u8);
            } else {
                set(&mut self.state, 0, out);
                out.push(cp as u8);
            }
            return true;
        }
        if cp == 0xA5 || cp == 0x203E {
            set(&mut self.state, 1, out);
            out.push(if cp == 0xA5 { 0x5C } else { 0x7E });
            return true;
        }
        match mbfl_encode_char(EncNo::EucJp, cp) {
            Some(b) if b.len() == 2 && b[0] >= 0xA1 && b[1] >= 0xA1 => {
                set(&mut self.state, 2, out);
                out.push(b[0] & 0x7F);
                out.push(b[1] & 0x7F);
                true
            }
            _ => false,
        }
    }

    /// The bytes returning to the initial shift state (iconv(cd, NULL,
    /// NULL, &out, &outleft)).
    pub fn reset_bytes(&self) -> &'static [u8] {
        match self.kind {
            Kind::Iso2022Jp if self.state != 0 => b"\x1b(B",
            _ => b"",
        }
    }

    /// Back to the initial state (after the reset bytes were written, or
    /// iconv(cd, NULL, NULL, NULL, NULL)).
    pub fn reset(&mut self) {
        // (the BOM of UTF-16 / UTF-32 is written once per conversion
        // descriptor)
        if self.kind == Kind::Iso2022Jp {
            self.state = 0;
        }
    }
}

pub fn push_utf8(out: &mut Vec<u8>, cp: u32) {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.push(0xC0 | (cp >> 6) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else if cp < 0x10000 {
        out.push(0xE0 | (cp >> 12) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else if cp < 0x200000 {
        out.push(0xF0 | (cp >> 18) as u8);
        out.push(0x80 | ((cp >> 12) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else if cp < 0x4000000 {
        out.push(0xF8 | (cp >> 24) as u8);
        out.push(0x80 | ((cp >> 18) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 12) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else {
        out.push(0xFC | (cp >> 30) as u8);
        out.push(0x80 | ((cp >> 24) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 18) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 12) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    }
}

fn push_u16(out: &mut Vec<u8>, v: u16, e: Endian) {
    out.extend_from_slice(&match e {
        Endian::Be => v.to_be_bytes(),
        Endian::Le => v.to_le_bytes(),
    });
}

fn push_u32(out: &mut Vec<u8>, v: u32, e: Endian) {
    out.extend_from_slice(&match e {
        Endian::Be => v.to_be_bytes(),
        Endian::Le => v.to_le_bytes(),
    });
}
