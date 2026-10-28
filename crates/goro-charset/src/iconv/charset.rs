//! The charsets iconv knows and their codecs.

use super::convert::DecodeStatus;
use super::iso2022::{Iso2022JpDecoder, Iso2022JpEncoder, Iso2022KrDecoder, Iso2022KrEncoder};
use super::table::{Table, TableDecoder, TableEncoder};
use super::tables::{cp, names};
use super::unicode::{self, Order, WideDecoder, WideEncoder};
use super::utf7::{Utf7Decoder, Utf7Encoder};

/// Hand-written codecs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Builtin {
    Ascii,
    Utf8,
    Utf16,
    Utf16Be,
    Utf16Le,
    Utf32,
    Utf32Be,
    Utf32Le,
    Ucs2,
    Ucs2Be,
    Ucs4,
    Ucs4Le,
    Unicode,
    Utf7,
    Utf7Imap,
    Iso2022Jp,
    Iso2022Kr,
    Gb18030,
}

pub(crate) enum Kind {
    Builtin(Builtin),
    Table(&'static Table),
}

pub(crate) struct CharsetInfo {
    pub name: &'static str,
    pub kind: Kind,
}

/// A charset known to iconv.
#[derive(Clone, Copy)]
pub struct Charset(pub(crate) &'static CharsetInfo);

impl std::fmt::Debug for Charset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Charset({})", self.0.name)
    }
}

impl PartialEq for Charset {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}

impl Eq for Charset {}

impl Charset {
    /// The canonical name (glibc's names are not visible from PHP; this is the
    /// most common alias).
    pub fn name(&self) -> &'static str {
        self.0.name
    }

    /// Looks up a charset name the way glibc's `iconv_open` does, without the
    /// `//` suffixes (see [`super::parse_charset`] for full names).
    pub fn lookup(name: &str) -> Option<Charset> {
        let n = super::names::normalise(name);
        let n = n
            .strip_suffix('/')
            .filter(|s| !s.ends_with('/'))
            .unwrap_or(&n);
        if n.is_empty() {
            return Some(Charset::utf8());
        }
        names::ALIASES
            .binary_search_by(|e| e.0.cmp(n))
            .ok()
            .map(|i| Charset(&names::CHARSETS[usize::from(names::ALIASES[i].1)]))
    }

    pub(crate) fn utf8() -> Charset {
        Charset::builtin(Builtin::Utf8)
    }

    fn builtin(b: Builtin) -> Charset {
        let i = names::CHARSETS
            .iter()
            .position(|c| matches!(c.kind, Kind::Builtin(x) if x == b))
            .expect("builtin charset");
        Charset(&names::CHARSETS[i])
    }

    pub(crate) fn decoder(&self) -> Decoder {
        match self.0.kind {
            Kind::Table(t) => Decoder::Table(TableDecoder::new(t)),
            Kind::Builtin(b) => match b {
                Builtin::Ascii => Decoder::Ascii,
                Builtin::Utf8 => Decoder::Utf8,
                Builtin::Utf16 => Decoder::Wide(WideDecoder::new(Order::Le, true, 2, true)),
                Builtin::Utf16Be => Decoder::Wide(WideDecoder::new(Order::Be, false, 2, true)),
                Builtin::Utf16Le => Decoder::Wide(WideDecoder::new(Order::Le, false, 2, true)),
                Builtin::Utf32 => Decoder::Wide(WideDecoder::new(Order::Le, true, 4, true)),
                Builtin::Utf32Be => Decoder::Wide(WideDecoder::new(Order::Be, false, 4, true)),
                Builtin::Utf32Le => Decoder::Wide(WideDecoder::new(Order::Le, false, 4, true)),
                Builtin::Ucs2 => Decoder::Wide(WideDecoder::new(Order::Le, false, 2, false)),
                Builtin::Ucs2Be => Decoder::Wide(WideDecoder::new(Order::Be, false, 2, false)),
                Builtin::Ucs4 => Decoder::Wide(WideDecoder::new(Order::Be, false, 4, false)),
                Builtin::Ucs4Le => Decoder::Wide(WideDecoder::new(Order::Le, false, 4, false)),
                Builtin::Unicode => Decoder::Wide(WideDecoder::new(Order::Le, true, 2, false)),
                Builtin::Utf7 => Decoder::Utf7(Utf7Decoder::new(false)),
                Builtin::Utf7Imap => Decoder::Utf7(Utf7Decoder::new(true)),
                Builtin::Iso2022Jp => Decoder::Iso2022Jp(Iso2022JpDecoder::new(&cp::EUC_JP)),
                Builtin::Iso2022Kr => Decoder::Iso2022Kr(Iso2022KrDecoder::new(&cp::EUC_KR)),
                Builtin::Gb18030 => {
                    Decoder::Table(TableDecoder::gb18030(&cp::GB18030, &cp::GB18030_FOUR))
                }
            },
        }
    }

    pub(crate) fn encoder(&self) -> Encoder {
        match self.0.kind {
            Kind::Table(t) => Encoder::Table(TableEncoder::new(t)),
            Kind::Builtin(b) => {
                let wide = |order, bom, width, utf| {
                    Encoder::Wide(WideEncoder {
                        order,
                        bom,
                        width,
                        utf,
                    })
                };
                match b {
                    Builtin::Ascii => Encoder::Ascii,
                    Builtin::Utf8 => Encoder::Utf8,
                    Builtin::Utf16 => wide(Order::Le, true, 2, true),
                    Builtin::Utf16Be => wide(Order::Be, false, 2, true),
                    Builtin::Utf16Le => wide(Order::Le, false, 2, true),
                    Builtin::Utf32 => wide(Order::Le, true, 4, true),
                    Builtin::Utf32Be => wide(Order::Be, false, 4, true),
                    Builtin::Utf32Le => wide(Order::Le, false, 4, true),
                    Builtin::Ucs2 => wide(Order::Le, false, 2, false),
                    Builtin::Ucs2Be => wide(Order::Be, false, 2, false),
                    Builtin::Ucs4 => wide(Order::Be, false, 4, false),
                    Builtin::Ucs4Le => wide(Order::Le, false, 4, false),
                    Builtin::Unicode => wide(Order::Le, true, 2, false),
                    Builtin::Utf7 => Encoder::Utf7(Utf7Encoder::new(false)),
                    Builtin::Utf7Imap => Encoder::Utf7(Utf7Encoder::new(true)),
                    Builtin::Iso2022Jp => Encoder::Iso2022Jp(Iso2022JpEncoder::new(&cp::EUC_JP)),
                    Builtin::Iso2022Kr => Encoder::Iso2022Kr(Iso2022KrEncoder::new(&cp::EUC_KR)),
                    Builtin::Gb18030 => {
                        Encoder::Table(TableEncoder::gb18030(&cp::GB18030, &cp::GB18030_FOUR))
                    }
                }
            }
        }
    }

    /// Single-byte table with every byte decoding to one character
    /// (fast paths).
    pub(crate) fn sbcs_table(&self) -> Option<&'static Table> {
        match self.0.kind {
            Kind::Table(t) if t.nodes.len() == 1 && t.compose.is_empty() && t.multi.is_empty() => {
                Some(t)
            }
            _ => None,
        }
    }

    pub(crate) fn is_utf8(&self) -> bool {
        matches!(self.0.kind, Kind::Builtin(Builtin::Utf8))
    }
}

/// A decoder in progress.
pub(crate) enum Decoder {
    Ascii,
    Utf8,
    Wide(WideDecoder),
    Utf7(Utf7Decoder),
    Table(TableDecoder),
    Iso2022Jp(Iso2022JpDecoder),
    Iso2022Kr(Iso2022KrDecoder),
}

impl Decoder {
    /// Decodes from `input[*pos..]` into `out` until the input ends, `out`
    /// holds `limit` characters, or an error (with `*pos` at the faulty
    /// sequence).
    pub(crate) fn decode(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u32>,
        limit: usize,
    ) -> DecodeStatus {
        match self {
            Decoder::Ascii => {
                while *pos < input.len() && out.len() < limit {
                    let b = input[*pos];
                    if b >= 0x80 {
                        return DecodeStatus::Illegal(1);
                    }
                    out.push(u32::from(b));
                    *pos += 1;
                }
                DecodeStatus::Ok
            }
            Decoder::Utf8 => unicode::decode_utf8(input, pos, out, limit),
            Decoder::Wide(d) => d.decode(input, pos, out, limit),
            Decoder::Utf7(d) => d.decode(input, pos, out, limit),
            Decoder::Table(d) => d.decode(input, pos, out, limit),
            Decoder::Iso2022Jp(d) => d.decode(input, pos, out, limit),
            Decoder::Iso2022Kr(d) => d.decode(input, pos, out, limit),
        }
    }

    /// End of input: flush pending characters; UTF-7 may be incomplete.
    pub(crate) fn finish(&mut self, out: &mut Vec<u32>) -> DecodeStatus {
        match self {
            Decoder::Table(d) => {
                d.flush(out);
                DecodeStatus::Ok
            }
            Decoder::Utf7(d) => d.finish(),
            _ => DecodeStatus::Ok,
        }
    }

    /// Whether ASCII bytes decode to themselves with no state involved.
    pub(crate) fn ascii_transparent(&self) -> bool {
        match self {
            Decoder::Ascii | Decoder::Utf8 => true,
            Decoder::Table(d) => d.transparent(),
            _ => false,
        }
    }
}

/// An encoder in progress.
pub(crate) enum Encoder {
    Ascii,
    Utf8,
    Wide(WideEncoder),
    Utf7(Utf7Encoder),
    Table(TableEncoder),
    Iso2022Jp(Iso2022JpEncoder),
    Iso2022Kr(Iso2022KrEncoder),
}

impl Encoder {
    /// Encodes `cps`; on a character the charset can't encode, returns its
    /// index (everything before it is written).
    pub(crate) fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>) -> Result<(), usize> {
        match self {
            Encoder::Ascii => {
                for (i, &c) in cps.iter().enumerate() {
                    if c >= 0x80 {
                        return Err(i);
                    }
                    out.push(c as u8);
                }
                Ok(())
            }
            Encoder::Utf8 => unicode::encode_utf8(cps, out),
            Encoder::Wide(e) => e.encode(cps, out),
            Encoder::Utf7(e) => e.encode(cps, out),
            Encoder::Table(e) => e.encode(cps, out),
            Encoder::Iso2022Jp(e) => e.encode(cps, out),
            Encoder::Iso2022Kr(e) => e.encode(cps, out),
        }
    }

    pub(crate) fn can_encode(&self, cp: u32) -> bool {
        match self {
            Encoder::Ascii => cp < 0x80,
            Encoder::Utf8 => cp < 0x8000_0000 && !(0xD800..0xE000).contains(&cp),
            Encoder::Wide(e) => e.can_encode(cp),
            Encoder::Utf7(_) => Utf7Encoder::can_encode(cp),
            Encoder::Table(e) => e.can_encode(cp),
            Encoder::Iso2022Jp(e) => e.can_encode(cp),
            Encoder::Iso2022Kr(e) => e.can_encode(cp),
        }
    }

    /// Output written before any character (the ISO-2022-KR header).
    pub(crate) fn start(&mut self, out: &mut Vec<u8>) {
        if let Encoder::Iso2022Kr(e) = self {
            e.start(out);
        }
    }

    /// End of output: close shift states.
    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) {
        match self {
            Encoder::Utf7(e) => e.finish(out),
            Encoder::Table(e) => e.finish(out),
            Encoder::Iso2022Jp(e) => e.finish(out),
            Encoder::Iso2022Kr(e) => e.finish(out),
            _ => {}
        }
    }

    pub(crate) fn ascii_transparent(&self) -> bool {
        match self {
            Encoder::Ascii | Encoder::Utf8 => true,
            Encoder::Table(e) => e.transparent(),
            _ => false,
        }
    }

    /// Whether unencodable tag characters (U+E0000-U+E007F) are dropped.
    pub(crate) fn drops_tags(&self) -> bool {
        !matches!(self, Encoder::Iso2022Jp(_) | Encoder::Iso2022Kr(_))
    }
}
