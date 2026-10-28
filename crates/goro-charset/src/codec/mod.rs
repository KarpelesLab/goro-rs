//! Decoders and encoders between bytes and Unicode code points.
//!
//! A decoder turns bytes into a stream of `u32` values: Unicode scalar values,
//! or [`ERR`] for each invalid byte sequence. (UCS-4 passes any 32-bit value
//! through, as mbstring does.) An encoder turns that stream back into bytes;
//! when a value has no representation, the substitution policy ([`Subst`])
//! decides what is written instead, mirroring `mb_substitute_character()`.

use crate::Encoding;

mod cjk_cn;
mod cjk_kr;
mod cjk_tw;
mod html;
mod iso2022jp;
mod japanese;
mod macjapanese;
mod mobile;
mod single;
pub(crate) mod transform;
mod unicode;
mod utf7;

pub(crate) use unicode::utf8_valid_up_to;

/// The value a decoder emits for an invalid byte sequence.
pub const ERR: u32 = 0xFFFF_FFFF;

/// What `mb_substitute_character()` is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubstMode {
    /// Write the substitute character ([`Subst::ch`]).
    Char,
    /// Write nothing (`"none"`).
    None,
    /// Write `U+XXXX` for characters the target lacks (`"long"`); invalid
    /// input still gets the substitute character.
    Long,
    /// Write `&#xXXXX;` for characters the target lacks (`"entity"`); invalid
    /// input still gets the substitute character.
    Entity,
}

/// The substitution policy for unconvertible characters.
///
/// `ch` keeps the last code point set with `mb_substitute_character()` even
/// while the mode is `long`, `entity` or `none`: in `long` and `entity` mode
/// mbstring still writes it for invalid input (observed on PHP 8.5, and test
/// ext/mbstring/tests/sjis_encoding.phpt expects `%` in `long` mode after
/// setting 0x25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subst {
    /// The mode.
    pub mode: SubstMode,
    /// The substitute code point (default `?`).
    pub ch: u32,
}

impl Default for Subst {
    fn default() -> Self {
        Subst {
            mode: SubstMode::Char,
            ch: b'?' as u32,
        }
    }
}

impl Subst {
    /// A policy writing `ch`.
    pub const fn char(ch: u32) -> Subst {
        Subst {
            mode: SubstMode::Char,
            ch,
        }
    }
}

/// A streaming decoder.
pub trait Decode {
    /// Decodes from `input[*pos..]`, appending to `out`, until the input is
    /// exhausted or `out.len() >= limit`. `input` is always the whole string,
    /// so a decoder sees a truncated character as truncated.
    ///
    /// The caller leaves at least 16 values of headroom past `limit`.
    fn decode(&mut self, input: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize);

    /// Called once after the input is exhausted.
    fn finish(&mut self, _out: &mut Vec<u32>) {}

    /// Whether `mb_check_encoding` rejects the input even without invalid
    /// sequences (ISO-2022-JP ending outside ASCII, say). Asked after
    /// [`Decode::finish`].
    fn check_failed(&self) -> bool {
        false
    }
}

/// A streaming encoder.
pub trait Encode {
    /// Encodes one value. Returns `false`, writing nothing, when it has no
    /// representation (always for [`ERR`]).
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool;

    /// Called once at the end, to flush state (pending characters, a final
    /// shift sequence).
    fn finish(&mut self, _out: &mut Vec<u8>, _sub: &Subst, _errors: &mut usize) {}

    /// Encodes a run of values, substituting the unrepresentable ones.
    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        for &cp in cps {
            if !self.put(cp, out) {
                fail(self, cp, out, sub, errors);
            }
        }
    }
}

/// Handles a value the encoder could not write.
#[cold]
pub(crate) fn fail<E: Encode + ?Sized>(
    e: &mut E,
    cp: u32,
    out: &mut Vec<u8>,
    sub: &Subst,
    errors: &mut usize,
) {
    *errors += 1;
    match sub.mode {
        SubstMode::None => {}
        SubstMode::Char => put_subst(e, sub.ch, out),
        SubstMode::Long | SubstMode::Entity if cp == ERR => put_subst(e, sub.ch, out),
        SubstMode::Long => {
            put_ascii(e, b"U+", out);
            put_hex(e, cp, out);
        }
        SubstMode::Entity => {
            put_ascii(e, b"&#x", out);
            put_hex(e, cp, out);
            put_ascii(e, b";", out);
        }
    }
}

fn put_subst<E: Encode + ?Sized>(e: &mut E, ch: u32, out: &mut Vec<u8>) {
    if !e.put(ch, out) {
        e.put(b'?' as u32, out);
    }
}

fn put_ascii<E: Encode + ?Sized>(e: &mut E, s: &[u8], out: &mut Vec<u8>) {
    for &b in s {
        e.put(b as u32, out);
    }
}

fn put_hex<E: Encode + ?Sized>(e: &mut E, v: u32, out: &mut Vec<u8>) {
    let mut buf = [0u8; 8];
    let mut n = 0;
    let mut v = v;
    loop {
        buf[n] = b"0123456789ABCDEF"[(v & 15) as usize];
        n += 1;
        v >>= 4;
        if v == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        e.put(buf[i] as u32, out);
    }
}

/// Creates the decoder for `enc`.
pub fn decoder(enc: Encoding) -> Box<dyn Decode> {
    use Encoding::*;
    match enc {
        Utf8 => Box::new(unicode::Utf8Dec),
        Utf16 => Box::new(unicode::Utf16Dec::new(unicode::Bom::Detect, true)),
        Utf16Be => Box::new(unicode::Utf16Dec::new(unicode::Bom::Be, true)),
        Utf16Le => Box::new(unicode::Utf16Dec::new(unicode::Bom::Le, true)),
        Ucs2 => Box::new(unicode::Utf16Dec::new(unicode::Bom::Detect, false)),
        Ucs2Be => Box::new(unicode::Utf16Dec::new(unicode::Bom::Be, false)),
        Ucs2Le => Box::new(unicode::Utf16Dec::new(unicode::Bom::Le, false)),
        Utf32 => Box::new(unicode::Utf32Dec::new(unicode::Bom::Detect, true)),
        Utf32Be => Box::new(unicode::Utf32Dec::new(unicode::Bom::Be, true)),
        Utf32Le => Box::new(unicode::Utf32Dec::new(unicode::Bom::Le, true)),
        Ucs4 => Box::new(unicode::Utf32Dec::new(unicode::Bom::Detect, false)),
        Ucs4Be => Box::new(unicode::Utf32Dec::new(unicode::Bom::Be, false)),
        Ucs4Le => Box::new(unicode::Utf32Dec::new(unicode::Bom::Le, false)),
        Ascii | SevenBit => Box::new(single::AsciiDec),
        EightBit => Box::new(single::Latin1Dec),
        Iso8859_1 => Box::new(single::Latin1Dec),
        HtmlEntities => Box::new(html::HtmlDec),
        Base64 | Uuencode | QuotedPrintable => Box::new(single::Latin1Dec),
        Utf7 => Box::new(utf7::Utf7Dec::new(false)),
        Utf7Imap => Box::new(utf7::Utf7Dec::new(true)),
        Gb18030 | Gb18030_2022 | EucCn | Cp936 => Box::new(cjk_cn::GbDec::new(enc)),
        Hz => Box::new(cjk_cn::HzDec::default()),
        Jis | Iso2022Jp | Iso2022JpMs | Iso2022Jp2004 | Iso2022JpKddi | Cp50220 | Cp50221
        | Cp50222 => Box::new(iso2022jp::Iso2022JpDec::new(enc)),
        Utf8Docomo | Utf8KddiA | Utf8KddiB | Utf8Softbank => {
            Box::new(mobile::Utf8MobileDec::new(enc))
        }
        SjisDocomo | SjisKddi | SjisSoftbank => Box::new(mobile::SjisMobileDec::new(enc)),
        EucTw => Box::new(cjk_tw::EucTwDec),
        Big5 | Cp950 => Box::new(cjk_tw::Big5Dec::new(enc)),
        EucKr | Uhc => Box::new(cjk_kr::KrDec::new(enc)),
        Iso2022Kr => Box::new(cjk_kr::Iso2022KrDec::default()),
        EucJp | EucJpWin | Cp51932 | EucJp2004 => Box::new(japanese::EucJpDec::new(enc)),
        Sjis | SjisMac | Sjis2004 | Cp932 | SjisWin => Box::new(japanese::SjisDec::new(enc)),
        _ => match single::table(enc) {
            Some(t) => Box::new(single::SingleDec::new(t)),
            None => Box::new(single::AsciiDec),
        },
    }
}

/// Creates the encoder for `enc`.
pub fn encoder(enc: Encoding) -> Box<dyn Encode> {
    use Encoding::*;
    match enc {
        Utf8 => Box::new(unicode::Utf8Enc),
        Utf16 | Utf16Be => Box::new(unicode::Utf16Enc::new(false, true)),
        Utf16Le => Box::new(unicode::Utf16Enc::new(true, true)),
        Ucs2 | Ucs2Be => Box::new(unicode::Utf16Enc::new(false, false)),
        Ucs2Le => Box::new(unicode::Utf16Enc::new(true, false)),
        Utf32 | Utf32Be => Box::new(unicode::Utf32Enc::new(false, true)),
        Utf32Le => Box::new(unicode::Utf32Enc::new(true, true)),
        Ucs4 | Ucs4Be => Box::new(unicode::Utf32Enc::new(false, false)),
        Ucs4Le => Box::new(unicode::Utf32Enc::new(true, false)),
        Ascii | SevenBit => Box::new(single::AsciiEnc),
        HtmlEntities => Box::new(html::HtmlEnc),
        EightBit | Iso8859_1 | Base64 | Uuencode | QuotedPrintable => Box::new(single::Latin1Enc),
        Utf7 => Box::new(utf7::Utf7Enc::new(false)),
        Utf7Imap => Box::new(utf7::Utf7Enc::new(true)),
        Gb18030 | Gb18030_2022 | EucCn | Cp936 => Box::new(cjk_cn::GbEnc::new(enc)),
        Hz => Box::new(cjk_cn::HzEnc::default()),
        Jis | Iso2022Jp | Iso2022JpMs | Iso2022Jp2004 | Iso2022JpKddi | Cp50220 | Cp50221
        | Cp50222 => Box::new(iso2022jp::Iso2022JpEnc::new(enc)),
        Utf8Docomo | Utf8KddiA | Utf8KddiB | Utf8Softbank => {
            Box::new(mobile::Utf8MobileEnc::new(enc))
        }
        SjisDocomo | SjisKddi | SjisSoftbank => Box::new(mobile::SjisMobileEnc::new(enc)),
        EucTw => Box::new(cjk_tw::EucTwEnc),
        Big5 | Cp950 => Box::new(cjk_tw::Big5Enc::new(enc)),
        EucKr | Uhc => Box::new(cjk_kr::KrEnc::new(enc)),
        Iso2022Kr => Box::new(cjk_kr::Iso2022KrEnc::default()),
        EucJp | EucJpWin | Cp51932 | EucJp2004 => Box::new(japanese::EucJpEnc::new(enc)),
        SjisMac => Box::new(macjapanese::MacEnc::new()),
        Sjis | Sjis2004 | Cp932 | SjisWin => Box::new(japanese::SjisEnc::new(enc)),
        _ => match single::table(enc) {
            Some(t) => Box::new(single::SingleEnc::new(t)),
            None => Box::new(single::AsciiEnc),
        },
    }
}

/// The chunk size of the decode → encode pipeline.
pub(crate) const CHUNK: usize = 2048;

/// Decodes all of `input` into code points.
pub fn decode_all(input: &[u8], enc: Encoding, out: &mut Vec<u32>) {
    let mut d = decoder(enc);
    let mut pos = 0;
    out.reserve(input.len().min(1 << 20) + 32);
    while pos < input.len() {
        if out.capacity() - out.len() < CHUNK + 32 {
            out.reserve(CHUNK + 32);
        }
        let limit = out.len() + CHUNK;
        d.decode(input, &mut pos, out, limit);
    }
    d.finish(out);
}

/// Encodes code points into `enc`, returning the number of unrepresentable
/// values.
pub fn encode_all(cps: &[u32], enc: Encoding, sub: &Subst, out: &mut Vec<u8>) -> usize {
    let mut e = encoder(enc);
    let mut errors = 0;
    out.reserve(cps.len());
    e.encode(cps, out, sub, &mut errors);
    e.finish(out, sub, &mut errors);
    errors
}
