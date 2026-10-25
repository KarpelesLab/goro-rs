//! The character sets of the iconv emulation: glibc's charset names and
//! one-character decoders / encoders with glibc's error semantics
//! (EILSEQ for an illegal input sequence or an unconvertible character,
//! EINVAL for an incomplete sequence at the end of the input). The CJK and
//! legacy single-byte code tables come from the mbstring extension's libmbfl
//! port.

use crate::sbcs_tables;
use goro_ext_mbstring::mbfl::{self, EncNo, Encoding, MBFL_BAD_INPUT};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Endian {
    Be,
    Le,
}

/// How a multi-byte character set's characters are shaped (for callers
/// classifying an undecodable sequence).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 0x8E: 2 bytes, 0x8F: 3 bytes, 0xA1-0xFE: 2 bytes.
    EucJp,
    /// 0x81-0x9F, 0xE0-0xFC: 2 bytes.
    Sjis,
    /// 0x81-0xFE: 2 bytes.
    Dbcs,
}

/// The family of an mbfl-backed charset.
pub fn family(no: EncNo) -> Family {
    match no {
        EncNo::EucJp | EncNo::EucJpWin => Family::EucJp,
        EncNo::Sjis | EncNo::Cp932 => Family::Sjis,
        _ => Family::Dbcs,
    }
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
    /// glibc's UNICODE: UCS-2 with a BOM (decode: detected, default
    /// little endian; encode: BOM + little endian).
    Unicode,
    /// UTF-7 (true: the IMAP variant, RFC 3501).
    Utf7(bool),
    Iso2022Jp,
    Cp1258,
    Mbfl(EncNo),
    /// A glibc single-byte charset (an index into sbcs_tables::TABLES).
    Sbcs(u16),
}

/// glibc charset names (and common aliases) to charsets. Names are
/// compared case-insensitively.
fn table() -> &'static [(&'static str, Kind)] {
    use Endian::*;
    use Kind::*;
    &[
        ("UTF-8", Utf8),
        ("UTF8", Utf8),
        ("ISO-10646/UTF8", Utf8),
        ("ISO-10646/UTF-8", Utf8),
        ("ISO-IR-193", Utf8),
        ("OSF05010001", Utf8),
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
        ("ISO-IR-6", Ascii),
        ("ANSI_X3.4", Ascii),
        ("OSF00010020", Ascii),
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
        ("UCS2", Ucs2(Le)),
        ("ISO-10646/UCS2", Ucs2(Le)),
        ("OSF00010100", Ucs2(Le)),
        ("OSF00010101", Ucs2(Le)),
        ("OSF00010102", Ucs2(Le)),
        ("UCS-2BE", Ucs2(Be)),
        ("UNICODEBIG", Ucs2(Be)),
        ("UCS-2LE", Ucs2(Le)),
        ("UNICODELITTLE", Ucs2(Le)),
        ("UCS-4", Ucs4(Be, false)),
        ("UCS4", Ucs4(Be, false)),
        ("ISO-10646", Ucs4(Be, false)),
        ("ISO-10646-UCS-4", Ucs4(Be, false)),
        ("CSUCS4", Ucs4(Be, false)),
        ("ISO-10646/UCS4", Ucs4(Be, false)),
        ("10646-1:1993", Ucs4(Be, false)),
        ("10646-1:1993/UCS4", Ucs4(Be, false)),
        ("OSF00010104", Ucs4(Be, false)),
        ("OSF00010105", Ucs4(Be, false)),
        ("OSF00010106", Ucs4(Be, false)),
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
        ("UTF16BE", Utf16(Some(Be))),
        ("UTF16LE", Utf16(Some(Le))),
        ("UTF32LE", Ucs4(Le, true)),
        ("UTF32BE", Ucs4(Be, true)),
        ("UNICODE", Unicode),
        ("UTF-7", Utf7(false)),
        ("UTF7", Utf7(false)),
        ("UTF-7-IMAP", Utf7(true)),
        ("UTF-16LE", Utf16(Some(Le))),
        ("ISO-2022-JP", Iso2022Jp),
        ("CSISO2022JP", Iso2022Jp),
        ("ISO2022JP", Iso2022Jp),
        ("CP1258", Cp1258),
        ("WINDOWS-1258", Cp1258),
        ("MS-VIET", Cp1258),
        ("EUC-JP", Mbfl(EncNo::EucJp)),
        ("EUCJP", Mbfl(EncNo::EucJp)),
        ("UJIS", Mbfl(EncNo::EucJp)),
        ("CSEUCPKDFMTJAPANESE", Mbfl(EncNo::EucJp)),
        ("EUC-JP-MS", Mbfl(EncNo::EucJpWin)),
        ("EUCJP-MS", Mbfl(EncNo::EucJpWin)),
        ("EUCJP-WIN", Mbfl(EncNo::EucJpWin)),
        ("EUCJP-OPEN", Mbfl(EncNo::EucJpWin)),
        ("SJIS", Mbfl(EncNo::Sjis)),
        ("SHIFT_JIS", Mbfl(EncNo::Sjis)),
        ("SHIFT-JIS", Mbfl(EncNo::Sjis)),
        ("MS_KANJI", Mbfl(EncNo::Sjis)),
        ("CSSHIFTJIS", Mbfl(EncNo::Sjis)),
        ("CP932", Mbfl(EncNo::Cp932)),
        ("WINDOWS-31J", Mbfl(EncNo::Cp932)),
        ("MS932", Mbfl(EncNo::Cp932)),
        ("SJIS-OPEN", Mbfl(EncNo::Cp932)),
        ("SJIS-WIN", Mbfl(EncNo::Cp932)),
        ("CSWINDOWS31J", Mbfl(EncNo::Cp932)),
        ("IBM-943", Mbfl(EncNo::Cp932)),
        ("EUC-KR", Mbfl(EncNo::EucKr)),
        ("EUCKR", Mbfl(EncNo::EucKr)),
        ("CSEUCKR", Mbfl(EncNo::EucKr)),
        ("UHC", Mbfl(EncNo::Uhc)),
        ("CP949", Mbfl(EncNo::Uhc)),
        ("MSCP949", Mbfl(EncNo::Uhc)),
        ("BIG5", Mbfl(EncNo::Big5)),
        ("BIG-5", Mbfl(EncNo::Big5)),
        ("BIG-FIVE", Mbfl(EncNo::Big5)),
        ("CN-BIG5", Mbfl(EncNo::Big5)),
        ("CSBIG5", Mbfl(EncNo::Big5)),
        ("CP950", Mbfl(EncNo::Cp950)),
        ("GB2312", Mbfl(EncNo::EucCn)),
        ("EUC-CN", Mbfl(EncNo::EucCn)),
        ("EUCCN", Mbfl(EncNo::EucCn)),
        ("CSGB2312", Mbfl(EncNo::EucCn)),
        ("CN-GB", Mbfl(EncNo::EucCn)),
        ("GBK", Mbfl(EncNo::Cp936)),
        ("GB13000", Mbfl(EncNo::Cp936)),
        ("CP936", Mbfl(EncNo::Cp936)),
        ("MS936", Mbfl(EncNo::Cp936)),
        ("WINDOWS-936", Mbfl(EncNo::Cp936)),
    ]
}

/// The charset named `name` (glibc's normalization: case-insensitive;
/// ISO_8859-N, ISO8859-N, ISO88591... spellings).
pub fn lookup(name: &[u8]) -> Option<Kind> {
    // glibc's strip(): only alphanumerics and "_-.,:" are kept; an empty
    // name is the locale's charset (UTF-8 for php's C.UTF-8 LC_CTYPE).
    let up: Vec<u8> = name
        .iter()
        .filter(|c| c.is_ascii_alphanumeric() || b"_-.,:/".contains(c))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if up.is_empty() {
        return Some(Kind::Utf8);
    }
    if let Ok(i) = sbcs_tables::NAMES.binary_search_by(|(n, _)| n.as_bytes().cmp(up.as_slice())) {
        return Some(Kind::Sbcs(sbcs_tables::NAMES[i].1));
    }
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
        (EncNo::Cp932, 0x2016) => return Some(vec![0x81, 0x61]),
        (EncNo::Cp932, 0x2212) => return Some(vec![0x81, 0x7C]),
        (EncNo::Cp932, 0x301C) => return Some(vec![0x81, 0x60]),
        (EncNo::EucKr, 0x20A9) => return Some(vec![0xA3, 0xDC]),
        (EncNo::EucKr, 0x327E) => return Some(vec![0xA2, 0xE8]),
        (EncNo::EucJpWin, 0xA2) => return Some(vec![0xA1, 0xF1]),
        (EncNo::EucJpWin, 0xA3) => return Some(vec![0xA1, 0xF2]),
        (EncNo::EucJpWin, 0xA5) => return Some(vec![0x5C]),
        (EncNo::EucJpWin, 0xA6) => return Some(vec![0x8F, 0xA2, 0xC3]),
        (EncNo::EucJpWin, 0xAC) => return Some(vec![0xA2, 0xCC]),
        (EncNo::EucJpWin, 0x2014) => return Some(vec![0xA1, 0xBD]),
        (EncNo::EucJpWin, 0x2016) => return Some(vec![0xA1, 0xC2]),
        (EncNo::EucJpWin, 0x203E) => return Some(vec![0x7E]),
        (EncNo::EucJpWin, 0x2212) => return Some(vec![0xA1, 0xDD]),
        (EncNo::EucJpWin, 0x301C) => return Some(vec![0xA1, 0xC1]),
        (EncNo::Sjis, 0xFFE0) | (EncNo::Cp932, 0xA2) => return Some(vec![0x81, 0x91]),
        (EncNo::Sjis, 0xFFE1) | (EncNo::Cp932, 0xA3) => return Some(vec![0x81, 0x92]),
        (EncNo::Sjis, 0xFFE2) | (EncNo::Cp932, 0xAC) => return Some(vec![0x81, 0xCA]),
        (EncNo::Big5 | EncNo::Cp950, 0x20AC) => return Some(vec![0xA3, 0xE1]),
        (EncNo::EucJp | EncNo::EucJpWin, 0x80..=0x8D | 0x90..=0x9F)
        | (EncNo::EucKr, 0x80..=0x9F)
        | (EncNo::Big5 | EncNo::Cp950, 0x80) => {
            return Some(vec![cp as u8]);
        }
        _ => {}
    }
    if no == EncNo::Big5 {
        use crate::big5_glibc::BIG5_ENC;
        if let Ok(i) = BIG5_ENC.binary_search_by_key(&cp, |&(c, _)| c) {
            let b = BIG5_ENC[i].1;
            return (b != 0).then(|| vec![(b >> 8) as u8, b as u8]);
        }
    }
    if no_pua(no, cp) {
        return None;
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

/// A private-use character glibc's table of the charset lacks (it has
/// none, or only BIG5's U+F6B1-U+F848).
fn no_pua(no: EncNo, cp: u32) -> bool {
    (0xE000..0xF900).contains(&cp)
        && match no {
            EncNo::EucJp
            | EncNo::Sjis
            | EncNo::EucKr
            | EncNo::Uhc
            | EncNo::EucCn
            | EncNo::Cp936 => true,
            EncNo::Big5 | EncNo::Cp950 => !(0xF6B1..=0xF848).contains(&cp),
            _ => false,
        }
}

/// glibc's BIG5 where it differs from libmbfl's: Some(0) is undefined.
fn big5_override(bytes: &[u8]) -> Option<u32> {
    use crate::big5_glibc::BIG5_DEC;
    let &[b1, b2] = bytes else { return None };
    let k = (b1 as u16) << 8 | b2 as u16;
    BIG5_DEC
        .binary_search_by_key(&k, |&(b, _)| b)
        .ok()
        .map(|i| BIG5_DEC[i].1)
}

/// mbfl_decode_char() with glibc's mapping differences.
fn mbfl_decode_char_glibc(no: EncNo, bytes: &[u8]) -> Option<u32> {
    match (no, bytes) {
        (EncNo::Sjis, [0x5C]) => return Some(0xA5),
        (EncNo::Sjis, [0x7E]) => return Some(0x203E),
        (EncNo::EucJp, [0x8F, 0xA2, 0xB7]) => return Some(0xFF5E),
        (EncNo::Big5 | EncNo::Cp950, [0xA3, 0xE1]) => return Some(0x20AC),
        (EncNo::EucKr, [0xA2, 0xE8]) => return Some(0x327E),
        (EncNo::Big5, _) => {
            if let Some(c) = big5_override(bytes) {
                return Some(c).filter(|&c| c != 0);
            }
        }
        _ => {}
    }
    mbfl_decode_char(no, bytes)
}

/// The byte of `cp` in a single-byte charset.
fn sbcs_encode(t: &sbcs_tables::Sbcs, cp: u32) -> Option<u8> {
    if cp >= 0xFFFF {
        return None;
    }
    if let Ok(i) = t.extra.binary_search_by_key(&(cp as u16), |&(c, _)| c) {
        return Some(t.extra[i].1);
    }
    let base = if t.dec.len() == 128 {
        if cp < 0x80 {
            return Some(cp as u8);
        }
        0x80
    } else {
        0
    };
    t.dec
        .iter()
        .position(|&c| c as u32 == cp)
        .map(|i| (base + i) as u8)
}

/// UTF-7 characters written as themselves.
fn utf7_direct(c: u32, imap: bool) -> bool {
    if imap {
        return (0x20..=0x7E).contains(&c) && c != b'&' as u32;
    }
    c < 0x80 && (c as u8).is_ascii_alphanumeric()
        || c < 0x80 && b"'(),-./:? \t\n\r".contains(&(c as u8))
}

/// UTF-7 characters accepted as themselves when decoding.
fn utf7_xdirect(c: u32, imap: bool) -> bool {
    if imap {
        return (0x20..=0x7E).contains(&c) && c != b'&' as u32;
    }
    c == 9
        || c == 10
        || c == 13
        || ((0x20..=0x7D).contains(&c) && c != b'+' as u32 && c != b'\\' as u32)
}

fn utf7_b64_value(c: u8, imap: bool) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' if !imap => Some(63),
        b',' if imap => Some(63),
        _ => None,
    }
}

fn utf7_b64_char(v: u32, imap: bool) -> u8 {
    let v = (v & 0x3F) as u8;
    match v {
        0..=25 => b'A' + v,
        26..=51 => b'a' + v - 26,
        52..=61 => b'0' + v - 52,
        62 => b'+',
        _ => {
            if imap {
                b','
            } else {
                b'/'
            }
        }
    }
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
    /// 2 JIS X 0208); UTF-16 / UTF-32: 0 = BOM not seen, 1 = BE, 2 = LE;
    /// UTF-7: 1 in base64.
    state: u8,
    /// UTF-7: the base64 bits not yet decoded, their count and a high
    /// surrogate waiting for its low one.
    acc: u32,
    nbits: u8,
    hi: u16,
}

impl Decoder {
    pub fn new(kind: Kind) -> Decoder {
        Decoder {
            kind,
            state: 0,
            acc: 0,
            nbits: 0,
            hi: 0,
        }
    }

    pub fn reset(&mut self) {
        self.state = 0;
        self.acc = 0;
        self.nbits = 0;
        self.hi = 0;
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
            Kind::Unicode => {
                if s.len() < 2 {
                    return Dec::Incomplete;
                }
                if self.state == 0 {
                    self.state = 2;
                    if s[0] == 0xFE && s[1] == 0xFF {
                        self.state = 1;
                        return Dec::Shift(2);
                    }
                    if s[0] == 0xFF && s[1] == 0xFE {
                        return Dec::Shift(2);
                    }
                }
                let e = if self.state == 1 {
                    Endian::Be
                } else {
                    Endian::Le
                };
                let c = u16_at(s, e) as u32;
                if (0xD800..0xE000).contains(&c) {
                    return Dec::Illegal(2);
                }
                Dec::Char(c, 2)
            }
            Kind::Utf7(imap) => self.decode_utf7(s, imap),
            Kind::Sbcs(t) => {
                let dec = sbcs_tables::TABLES[t as usize].dec;
                let b = s[0] as usize;
                let c = if dec.len() == 128 {
                    if b < 0x80 {
                        return Dec::Char(b as u32, 1);
                    }
                    dec[b - 0x80]
                } else {
                    dec[b]
                };
                if c == 0xFFFF {
                    Dec::Illegal(1)
                } else {
                    Dec::Char(c as u32, 1)
                }
            }
            Kind::Mbfl(no) => {
                let c = s[0];
                // The length of a character by its lead byte in glibc's
                // decoders (a lone lead byte is an incomplete character).
                let n = match (no, c) {
                    (EncNo::EucJp | EncNo::EucJpWin, 0x8F) => 3,
                    (EncNo::EucJp | EncNo::EucJpWin | EncNo::EucCn, 0x8E | 0x8F)
                    | (EncNo::EucJp | EncNo::EucJpWin, 0xA0..=0xFE)
                    | (EncNo::Sjis, 0x81..=0x9F | 0xE0..=0xEA)
                    | (EncNo::Cp932, 0x81..=0x9F | 0xE0..=0xFC)
                    | (EncNo::EucKr, 0xA1..=0xFF)
                    | (EncNo::Uhc, 0x81..=0xC8 | 0xCA..=0xFD)
                    | (EncNo::Big5 | EncNo::Cp950, 0xA1..=0xF9)
                    | (EncNo::EucCn, 0xA1..=0xFE)
                    | (EncNo::Cp936, 0x81..=0xFE) => 2,
                    _ => 1,
                };
                // glibc's EUC-JP and EUC-KR pass the C1 controls through,
                // BIG5 0x80.
                if (0x80..=0x9F).contains(&c)
                    && (((no == EncNo::EucJp || no == EncNo::EucJpWin) && c != 0x8E && c != 0x8F)
                        || no == EncNo::EucKr)
                    || (c == 0x80 && matches!(no, EncNo::Big5 | EncNo::Cp950))
                {
                    return Dec::Char(c as u32, 1);
                }
                if n == 1 && c >= 0x80 && mbfl_decode_char_glibc(no, &s[..1]).is_none() {
                    return Dec::Illegal(1);
                }
                if s.len() < n {
                    // (EUC-JP's 0x8F checks the second byte first)
                    let rows = if no == EncNo::EucJp {
                        0xA2..=0xED
                    } else {
                        0xA1..=0xFE
                    };
                    if n == 3 && s.len() == 2 && !rows.contains(&s[1]) {
                        return Dec::Illegal(1);
                    }
                    return Dec::Incomplete;
                }
                match mbfl_decode_char_glibc(no, &s[..n]) {
                    Some(cp) if no_pua(no, cp) => Dec::Illegal(1),
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

    fn decode_utf7(&mut self, s: &[u8], imap: bool) -> Dec {
        let c = s[0];
        if self.state == 0 {
            if c == if imap { b'&' } else { b'+' } {
                if s.len() < 2 {
                    return Dec::Incomplete;
                }
                if s[1] == b'-' {
                    return Dec::Char(c as u32, 2);
                }
                self.state = 1;
                self.acc = 0;
                self.nbits = 0;
                self.hi = 0;
                return Dec::Shift(1);
            }
            return if utf7_xdirect(c as u32, imap) {
                Dec::Char(c as u32, 1)
            } else {
                Dec::Illegal(1)
            };
        }
        let Some(v) = utf7_b64_value(c, imap) else {
            // The end of the base64 run: leftover bits must be padding.
            // (IMAP ends it with "-" only)
            let bad = self.nbits > 6 || self.acc != 0 || self.hi != 0 || (imap && c != b'-');
            self.reset();
            if bad {
                return Dec::Illegal(1);
            }
            if c == b'-' {
                return Dec::Shift(1);
            }
            return self.decode_utf7(s, imap);
        };
        self.acc = (self.acc << 6) | v as u32;
        self.nbits += 6;
        if self.nbits < 16 {
            return Dec::Shift(1);
        }
        self.nbits -= 16;
        let u = (self.acc >> self.nbits) as u16;
        self.acc &= (1 << self.nbits) - 1;
        match u {
            0xD800..=0xDBFF if self.hi == 0 => {
                self.hi = u;
                Dec::Shift(1)
            }
            0xDC00..=0xDFFF if self.hi != 0 => {
                let cp = 0x10000 + (((self.hi as u32) - 0xD800) << 10) + (u as u32 - 0xDC00);
                self.hi = 0;
                Dec::Char(cp, 1)
            }
            _ if self.hi != 0 || (0xD800..0xE000).contains(&u) => {
                self.hi = 0;
                Dec::Illegal(1)
            }
            _ => Dec::Char(u as u32, 1),
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
    /// ISO-2022-JP: designated set; UTF-16 / UTF-32: BOM written;
    /// UTF-7: 1 in base64.
    state: u8,
    /// UTF-7: the base64 bits not yet written and their count.
    acc: u32,
    nbits: u8,
}

impl Encoder {
    pub fn new(kind: Kind) -> Encoder {
        Encoder {
            kind,
            state: 0,
            acc: 0,
            nbits: 0,
        }
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
            Kind::Unicode => {
                if cp >= 0x10000 || (0xD800..0xE000).contains(&cp) {
                    return false;
                }
                if self.state == 0 {
                    self.state = 1;
                    push_u16(out, 0xFEFF, Endian::Le);
                }
                push_u16(out, cp as u16, Endian::Le);
                true
            }
            Kind::Utf7(imap) => self.encode_utf7(cp, out, imap),
            Kind::Sbcs(t) => match sbcs_encode(&sbcs_tables::TABLES[t as usize], cp) {
                Some(b) => {
                    out.push(b);
                    true
                }
                None => false,
            },
            Kind::Mbfl(no) => match mbfl_encode_char(no, cp) {
                Some(b) => {
                    out.extend_from_slice(&b);
                    true
                }
                None => false,
            },
        }
    }

    fn encode_utf7(&mut self, cp: u32, out: &mut Vec<u8>, imap: bool) -> bool {
        if cp > 0x10FFFF || (0xD800..0xE000).contains(&cp) {
            return false;
        }
        let shift = if imap { b'&' } else { b'+' };
        if utf7_direct(cp, imap) || (imap && cp == b'&' as u32 && self.state == 1) {
            if self.state == 1 {
                self.flush_utf7(out, imap);
                // (UTF-7 only needs the '-' before a base64 character)
                if imap || (cp as u8).is_ascii_alphanumeric() || b"+/-".contains(&(cp as u8)) {
                    out.push(b'-');
                }
            }
            out.push(cp as u8);
            if imap && cp == b'&' as u32 {
                out.push(b'-');
            }
            return true;
        }
        if self.state == 0 {
            if cp == shift as u32 {
                out.push(shift);
                out.push(b'-');
                return true;
            }
            out.push(shift);
            self.state = 1;
        }
        let mut units = [cp as u16, 0];
        let mut n = 1;
        if cp >= 0x10000 {
            let v = cp - 0x10000;
            units = [0xD800 + (v >> 10) as u16, 0xDC00 + (v & 0x3FF) as u16];
            n = 2;
        }
        for &u in &units[..n] {
            self.acc = (self.acc << 16) | u as u32;
            self.nbits += 16;
            while self.nbits >= 6 {
                self.nbits -= 6;
                out.push(utf7_b64_char(self.acc >> self.nbits, imap));
            }
            self.acc &= (1 << self.nbits) - 1;
        }
        true
    }

    /// Writes the pending base64 bits of UTF-7 (zero padded) and leaves
    /// base64.
    fn flush_utf7(&mut self, out: &mut Vec<u8>, imap: bool) {
        if self.nbits > 0 {
            out.push(utf7_b64_char(self.acc << (6 - self.nbits), imap));
        }
        self.state = 0;
        self.acc = 0;
        self.nbits = 0;
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
    pub fn reset_bytes(&self) -> Vec<u8> {
        match self.kind {
            Kind::Iso2022Jp if self.state != 0 => b"\x1b(B".to_vec(),
            Kind::Utf7(imap) if self.state != 0 => {
                let mut e = *self;
                let mut out = Vec::new();
                e.flush_utf7(&mut out, imap);
                out.push(b'-');
                out
            }
            _ => Vec::new(),
        }
    }

    /// Back to the initial state (after the reset bytes were written, or
    /// iconv(cd, NULL, NULL, NULL, NULL)).
    pub fn reset(&mut self) {
        // (the BOM of UTF-16 / UTF-32 is written once per conversion
        // descriptor)
        if matches!(self.kind, Kind::Iso2022Jp | Kind::Utf7(_)) {
            self.state = 0;
            self.acc = 0;
            self.nbits = 0;
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
