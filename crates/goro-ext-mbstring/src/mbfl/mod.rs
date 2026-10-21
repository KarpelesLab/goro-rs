//! A Rust port of PHP's libmbfl (ext/mbstring/libmbfl), using the "fast"
//! conversion API from PHP 8.3+: each encoding has a `to_wchar` decoder which
//! turns bytes into Unicode codepoints (with `MBFL_BAD_INPUT` marking invalid
//! byte sequences) and a `from_wchar` encoder which writes codepoints into a
//! `ConvBuf`, calling `mb_illegal_output` for codepoints it cannot represent.
//!
//! Differences from the C API: decoders always consume their whole input and
//! push into a growable Vec (there is no output buffer limit), and the state
//! word is passed separately. The results are identical to running the C
//! decoder in a loop until the input is exhausted.
#![allow(non_upper_case_globals, clippy::all)]

pub mod basic;
pub mod chinese;
pub mod common;
pub mod eucjp;
pub mod iso2022;
pub mod mobile;
pub mod singlebyte;
pub mod sjis;
pub mod tables;
pub mod transfer;
pub mod unicode;
pub mod utf7;
pub mod utf8_mobile;

/// Codepoint used to mark an invalid byte sequence in decoded text.
pub const MBFL_BAD_INPUT: u32 = 0xFFFF_FFFF;

pub const MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE: u8 = 0;
pub const MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR: u8 = 1;
pub const MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG: u8 = 2;
pub const MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY: u8 = 3;
/// For internal use only; uses 0xFF (invalid in UTF-8) as the error marker.
pub const MBFL_OUTPUTFILTER_ILLEGAL_MODE_BADUTF8: u8 = 4;

pub const MBFL_ENCTYPE_SBCS: u32 = 0x0000_0001;
pub const MBFL_ENCTYPE_WCS2: u32 = 0x0000_0002;
pub const MBFL_ENCTYPE_WCS4: u32 = 0x0000_0004;
pub const MBFL_ENCTYPE_GL_UNSAFE: u32 = 0x0000_4000;

pub const MBFL_WCSPLANE_UCS2MAX: u32 = 0x0001_0000;
pub const MBFL_WCSPLANE_UTF32MAX: u32 = 0x0011_0000;
pub const MBFL_WCSPLANE_SUPMIN: u32 = 0x0001_0000;
pub const MBFL_WCSPLANE_SUPMAX: u32 = 0x0020_0000;

/// Mirrors `enum mbfl_no_encoding` from libmbfl.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum EncNo {
    Pass,
    Wchar,
    Base64,
    Uuencode,
    HtmlEnt,
    Qprint,
    SevenBit,
    EightBit,
    Ucs4,
    Ucs4be,
    Ucs4le,
    Ucs2,
    Ucs2be,
    Ucs2le,
    Utf32,
    Utf32be,
    Utf32le,
    Utf16,
    Utf16be,
    Utf16le,
    Utf8,
    Utf8Docomo,
    Utf8KddiA,
    Utf8KddiB,
    Utf8Sb,
    Utf7,
    Utf7imap,
    Ascii,
    EucJp,
    EucJp2004,
    Sjis,
    EucJpWin,
    SjisDocomo,
    SjisKddi,
    SjisSb,
    SjisMac,
    Sjis2004,
    Cp932,
    SjisWin,
    Cp51932,
    Jis,
    Iso2022jp,
    Iso2022jp2004,
    Iso2022jpKddi,
    Iso2022jpms,
    Gb18030,
    Gb18030_2022,
    Cp1252,
    Cp1254,
    Iso8859_1,
    Iso8859_2,
    Iso8859_3,
    Iso8859_4,
    Iso8859_5,
    Iso8859_6,
    Iso8859_7,
    Iso8859_8,
    Iso8859_9,
    Iso8859_10,
    Iso8859_13,
    Iso8859_14,
    Iso8859_15,
    EucCn,
    Cp936,
    EucTw,
    Big5,
    Cp950,
    EucKr,
    Iso2022kr,
    Uhc,
    Hz,
    Cp1251,
    Cp866,
    Koi8r,
    Koi8u,
    Iso8859_16,
    Armscii8,
    Cp850,
    Cp50220,
    Cp50221,
    Cp50222,
}

/// Output buffer for `from_wchar` encoders (C: `mb_convert_buf`).
pub struct ConvBuf {
    pub out: Vec<u8>,
    pub state: u32,
    pub errors: usize,
    pub replacement_char: u32,
    pub error_mode: u8,
}

impl ConvBuf {
    pub fn new(initsize: usize, replacement_char: u32, error_mode: u8) -> ConvBuf {
        ConvBuf {
            out: Vec::with_capacity(initsize),
            state: 0,
            errors: 0,
            replacement_char,
            error_mode,
        }
    }
    #[inline]
    pub fn add(&mut self, c: u8) {
        self.out.push(c);
    }
    #[inline]
    pub fn add2(&mut self, c1: u8, c2: u8) {
        self.out.push(c1);
        self.out.push(c2);
    }
    #[inline]
    pub fn add3(&mut self, c1: u8, c2: u8, c3: u8) {
        self.out.extend_from_slice(&[c1, c2, c3]);
    }
    #[inline]
    pub fn add4(&mut self, c1: u8, c2: u8, c3: u8, c4: u8) {
        self.out.extend_from_slice(&[c1, c2, c3, c4]);
    }
    #[inline]
    pub fn appends(&mut self, s: &[u8]) {
        self.out.extend_from_slice(s);
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.out.len()
    }
}

/// Decoder: consumes all of `input`, appending codepoints to `out`.
/// `state` persists between calls for stateful encodings (it starts at 0).
pub type ToWcharFn = fn(input: &[u8], out: &mut Vec<u32>, state: &mut u32);
/// Encoder: `end` is true on the final call, so stateful encoders can emit
/// any trailing bytes (e.g. an escape sequence back to ASCII).
pub type FromWcharFn = fn(input: &[u32], buf: &mut ConvBuf, end: bool);
/// Fast validity check for an entire string.
pub type CheckFn = fn(input: &[u8]) -> bool;
/// `mb_strcut` implementation: cut `len` bytes starting at byte `from`.
pub type CutFn = fn(s: &[u8], from: usize, len: usize) -> Vec<u8>;

pub struct Encoding {
    pub no: EncNo,
    pub name: &'static str,
    pub mime_name: Option<&'static str>,
    pub aliases: &'static [&'static str],
    pub mblen_table: Option<&'static [u8; 256]>,
    pub flag: u32,
    pub to_wchar: Option<ToWcharFn>,
    pub from_wchar: Option<FromWcharFn>,
    pub check: Option<CheckFn>,
    pub cut: Option<CutFn>,
}

impl PartialEq for Encoding {
    fn eq(&self, other: &Encoding) -> bool {
        self.no == other.no
    }
}

impl std::fmt::Debug for Encoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Encoding({})", self.name)
    }
}

impl Encoding {
    /// Decode a whole string to codepoints.
    pub fn decode(&self, input: &[u8]) -> Vec<u32> {
        let mut out = Vec::with_capacity(input.len());
        let mut state = 0u32;
        if let Some(f) = self.to_wchar {
            f(input, &mut out, &mut state);
        }
        out
    }

    /// Encode a whole sequence of codepoints.
    pub fn encode(&self, input: &[u32], replacement_char: u32, error_mode: u8) -> (Vec<u8>, usize) {
        let mut buf = ConvBuf::new(input.len(), replacement_char, error_mode);
        if let Some(f) = self.from_wchar {
            f(input, &mut buf, true);
        }
        (buf.out, buf.errors)
    }
}

const HEXCHARS: &[u8; 16] = b"0123456789ABCDEF";

fn convert_cp_to_hex(cp: u32, out: &mut Vec<u32>) {
    let mut nonzero = false;
    let mut shift: i32 = 28;
    while shift >= 0 {
        let n = (cp >> shift) & 0xF;
        if n != 0 || nonzero {
            nonzero = true;
            out.push(HEXCHARS[n as usize] as u32);
        }
        shift -= 4;
    }
    if !nonzero {
        out.push('0' as u32);
    }
}

fn mb_illegal_marker(bad_cp: u32, out: &mut Vec<u32>, err_mode: u8, replacement_char: u32) {
    if bad_cp == MBFL_BAD_INPUT {
        if err_mode != MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE {
            out.push(replacement_char);
        }
    } else {
        match err_mode {
            MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR => out.push(replacement_char),
            MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG => {
                out.push('U' as u32);
                out.push('+' as u32);
                convert_cp_to_hex(bad_cp, out);
            }
            MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY => {
                out.push('&' as u32);
                out.push('#' as u32);
                out.push('x' as u32);
                convert_cp_to_hex(bad_cp, out);
                out.push(';' as u32);
            }
            _ => {}
        }
    }
}

/// C: `mb_illegal_output` (what `MB_CONVERT_ERROR` expands to).
/// `f` is the encoder which hit the error; it is re-invoked to emit the
/// error marker (replacement char, U+XXXX or &#xXXXX;).
pub fn mb_illegal_output(bad_cp: u32, f: FromWcharFn, buf: &mut ConvBuf) {
    buf.errors += 1;
    let repl_char = buf.replacement_char;
    let err_mode = buf.error_mode;

    if err_mode == MBFL_OUTPUTFILTER_ILLEGAL_MODE_BADUTF8 {
        buf.out.push(0xFF);
        return;
    }

    let mut temp: Vec<u32> = Vec::with_capacity(12);
    mb_illegal_marker(bad_cp, &mut temp, err_mode, repl_char);

    // Avoid infinite loop if `f` is not able to handle `repl_char`
    if err_mode == MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR && repl_char != '?' as u32 {
        buf.replacement_char = '?' as u32;
    } else {
        buf.error_mode = MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE;
    }

    f(&temp, buf, false);

    buf.replacement_char = repl_char;
    buf.error_mode = err_mode;
}

/// Order of `mbfl_encoding_ptr_list` in libmbfl (used for name lookup and
/// mb_list_encodings()).
const ENCODING_ORDER: &[EncNo] = &[
    EncNo::Base64,
    EncNo::Uuencode,
    EncNo::HtmlEnt,
    EncNo::Qprint,
    EncNo::SevenBit,
    EncNo::EightBit,
    EncNo::Ucs4,
    EncNo::Ucs4be,
    EncNo::Ucs4le,
    EncNo::Ucs2,
    EncNo::Ucs2be,
    EncNo::Ucs2le,
    EncNo::Utf32,
    EncNo::Utf32be,
    EncNo::Utf32le,
    EncNo::Utf16,
    EncNo::Utf16be,
    EncNo::Utf16le,
    EncNo::Utf8,
    EncNo::Utf7,
    EncNo::Utf7imap,
    EncNo::Ascii,
    EncNo::EucJp,
    EncNo::Sjis,
    EncNo::EucJpWin,
    EncNo::EucJp2004,
    EncNo::SjisDocomo,
    EncNo::SjisKddi,
    EncNo::SjisSb,
    EncNo::SjisMac,
    EncNo::Sjis2004,
    EncNo::Utf8Docomo,
    EncNo::Utf8KddiA,
    EncNo::Utf8KddiB,
    EncNo::Utf8Sb,
    EncNo::Cp932,
    EncNo::SjisWin,
    EncNo::Cp51932,
    EncNo::Jis,
    EncNo::Iso2022jp,
    EncNo::Iso2022jpms,
    EncNo::Gb18030,
    EncNo::Gb18030_2022,
    EncNo::Cp1252,
    EncNo::Cp1254,
    EncNo::Iso8859_1,
    EncNo::Iso8859_2,
    EncNo::Iso8859_3,
    EncNo::Iso8859_4,
    EncNo::Iso8859_5,
    EncNo::Iso8859_6,
    EncNo::Iso8859_7,
    EncNo::Iso8859_8,
    EncNo::Iso8859_9,
    EncNo::Iso8859_10,
    EncNo::Iso8859_13,
    EncNo::Iso8859_14,
    EncNo::Iso8859_15,
    EncNo::Iso8859_16,
    EncNo::EucCn,
    EncNo::Cp936,
    EncNo::Hz,
    EncNo::EucTw,
    EncNo::Big5,
    EncNo::Cp950,
    EncNo::EucKr,
    EncNo::Uhc,
    EncNo::Iso2022kr,
    EncNo::Cp1251,
    EncNo::Cp866,
    EncNo::Koi8r,
    EncNo::Koi8u,
    EncNo::Armscii8,
    EncNo::Cp850,
    EncNo::Iso2022jp2004,
    EncNo::Iso2022jpKddi,
    EncNo::Cp50220,
    EncNo::Cp50221,
    EncNo::Cp50222,
];

fn all_family_encodings() -> Vec<&'static Encoding> {
    let mut v: Vec<&'static Encoding> = Vec::new();
    v.extend_from_slice(basic::ENCODINGS);
    v.extend_from_slice(singlebyte::ENCODINGS);
    v.extend_from_slice(iso2022::ENCODINGS);
    v.extend_from_slice(unicode::ENCODINGS);
    v.extend_from_slice(utf7::ENCODINGS);
    v.extend_from_slice(transfer::ENCODINGS);
    v.extend_from_slice(chinese::ENCODINGS);
    v.extend_from_slice(mobile::ENCODINGS);
    v.extend_from_slice(sjis::ENCODINGS);
    v.extend_from_slice(eucjp::ENCODINGS);
    v.extend_from_slice(utf8_mobile::ENCODINGS);
    v
}

thread_local! {
    static REGISTRY: Vec<&'static Encoding> = {
        let fams = all_family_encodings();
        let mut v = Vec::new();
        for no in ENCODING_ORDER {
            if let Some(e) = fams.iter().find(|e| e.no == *no) {
                v.push(*e);
            }
        }
        v
    };
}

/// C: `mbfl_get_supported_encodings`
pub fn supported_encodings() -> Vec<&'static Encoding> {
    REGISTRY.with(|r| r.clone())
}

/// C: `mbfl_name2encoding`
pub fn name2encoding(name: &[u8]) -> Option<&'static Encoding> {
    REGISTRY.with(|r| {
        for e in r.iter() {
            if e.name.as_bytes().eq_ignore_ascii_case(name) {
                return Some(*e);
            }
        }
        for e in r.iter() {
            if let Some(m) = e.mime_name {
                if m.as_bytes().eq_ignore_ascii_case(name) {
                    return Some(*e);
                }
            }
        }
        for e in r.iter() {
            for a in e.aliases {
                if a.as_bytes().eq_ignore_ascii_case(name) {
                    return Some(*e);
                }
            }
        }
        None
    })
}

/// C: `mbfl_no2encoding` (also finds `pass` and `wchar`)
pub fn no2encoding(no: EncNo) -> Option<&'static Encoding> {
    match no {
        EncNo::Pass => return Some(&basic::ENC_PASS),
        EncNo::Wchar => return Some(&basic::ENC_WCHAR),
        _ => {}
    }
    REGISTRY.with(|r| r.iter().find(|e| e.no == no).copied())
}

/// C: `mbfl_encoding_preferred_mime_name`
pub fn preferred_mime_name(enc: &Encoding) -> Option<&'static str> {
    match enc.mime_name {
        Some(m) if !m.is_empty() => Some(m),
        _ => None,
    }
}

/// C: `mb_fast_convert`. Returns the converted bytes and the error count.
pub fn fast_convert(
    input: &[u8],
    from: &Encoding,
    to: &Encoding,
    repl_char: u32,
    error_mode: u8,
) -> (Vec<u8>, usize) {
    // Transfer encodings operate on raw bytes (C: mb_fast_convert)
    let (from, to): (&Encoding, &Encoding) = if matches!(to.no, EncNo::Base64 | EncNo::Qprint) {
        (&basic::ENC_8BIT, to)
    } else if matches!(from.no, EncNo::Base64 | EncNo::Qprint | EncNo::Uuencode) {
        (from, &basic::ENC_8BIT)
    } else {
        (from, to)
    };
    if input.is_empty() {
        // C never calls the encoder when there is no input
        return (Vec::new(), 0);
    }
    let to_wchar = match from.to_wchar {
        Some(f) => f,
        None => return (input.to_vec(), 0),
    };
    let from_wchar = match to.from_wchar {
        Some(f) => f,
        None => return (input.to_vec(), 0),
    };
    let mut buf = ConvBuf::new(input.len(), repl_char, error_mode);
    let mut state = 0u32;
    let mut wchars: Vec<u32> = Vec::with_capacity(input.len());
    to_wchar(input, &mut wchars, &mut state);
    from_wchar(&wchars, &mut buf, true);
    (buf.out, buf.errors)
}
