//! The encodings mbstring knows, with their names, aliases and MIME names.
//!
//! The list, its order and the aliases are those `mb_list_encodings()` and
//! `mb_encoding_aliases()` return (observed on PHP 8.5). Names are matched
//! ASCII-case-insensitively against the name, the aliases and the MIME name.

/// An mbstring encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
#[allow(missing_docs)]
pub enum Encoding {
    Base64,
    Uuencode,
    HtmlEntities,
    QuotedPrintable,
    SevenBit,
    EightBit,
    Ucs4,
    Ucs4Be,
    Ucs4Le,
    Ucs2,
    Ucs2Be,
    Ucs2Le,
    Utf32,
    Utf32Be,
    Utf32Le,
    Utf16,
    Utf16Be,
    Utf16Le,
    Utf8,
    Utf7,
    Utf7Imap,
    Ascii,
    EucJp,
    Sjis,
    EucJpWin,
    EucJp2004,
    SjisDocomo,
    SjisKddi,
    SjisSoftbank,
    SjisMac,
    Sjis2004,
    Utf8Docomo,
    Utf8KddiA,
    Utf8KddiB,
    Utf8Softbank,
    Cp932,
    SjisWin,
    Cp51932,
    Jis,
    Iso2022Jp,
    Iso2022JpMs,
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
    Iso8859_16,
    EucCn,
    Cp936,
    Hz,
    EucTw,
    Big5,
    Cp950,
    EucKr,
    Uhc,
    Iso2022Kr,
    Cp1251,
    Cp866,
    Koi8R,
    Koi8U,
    ArmScii8,
    Cp850,
    Iso2022Jp2004,
    Iso2022JpKddi,
    Cp50220,
    Cp50221,
    Cp50222,
}

/// Static facts about one encoding.
#[derive(Debug)]
pub struct Info {
    /// The canonical name (`mb_list_encodings()`).
    pub name: &'static str,
    /// The aliases (`mb_encoding_aliases()`).
    pub aliases: &'static [&'static str],
    /// The MIME name (`mb_preferred_mime_name()`), if any.
    pub mime: Option<&'static str>,
}

macro_rules! info {
    ($name:expr, [$($a:expr),*], $mime:expr) => {
        Info { name: $name, aliases: &[$($a),*], mime: $mime }
    };
}

/// One entry per [`Encoding`] variant, in variant order.
static INFO: [Info; 79] = [
    info!("BASE64", [], Some("BASE64")),
    info!("UUENCODE", [], Some("x-uuencode")),
    info!("HTML-ENTITIES", ["HTML", "html"], Some("HTML-ENTITIES")),
    info!("Quoted-Printable", ["qprint"], Some("Quoted-Printable")),
    info!("7bit", [], Some("7bit")),
    info!("8bit", ["binary"], Some("8bit")),
    info!("UCS-4", ["ISO-10646-UCS-4", "UCS4"], Some("UCS-4")),
    info!("UCS-4BE", ["byte4be"], Some("UCS-4BE")),
    info!("UCS-4LE", ["byte4le"], Some("UCS-4LE")),
    info!("UCS-2", ["ISO-10646-UCS-2", "UCS2", "UNICODE"], Some("UCS-2")),
    info!("UCS-2BE", ["byte2be"], Some("UCS-2BE")),
    info!("UCS-2LE", ["byte2le"], Some("UCS-2LE")),
    info!("UTF-32", ["utf32"], Some("UTF-32")),
    info!("UTF-32BE", [], Some("UTF-32BE")),
    info!("UTF-32LE", [], Some("UTF-32LE")),
    info!("UTF-16", ["utf16"], Some("UTF-16")),
    info!("UTF-16BE", [], Some("UTF-16BE")),
    info!("UTF-16LE", [], Some("UTF-16LE")),
    info!("UTF-8", ["utf8"], Some("UTF-8")),
    info!("UTF-7", ["utf7"], Some("UTF-7")),
    info!("UTF7-IMAP", ["mUTF-7"], None),
    info!(
        "ASCII",
        [
            "ANSI_X3.4-1968",
            "iso-ir-6",
            "ANSI_X3.4-1986",
            "ISO_646.irv:1991",
            "US-ASCII",
            "ISO646-US",
            "us",
            "IBM367",
            "IBM-367",
            "cp367",
            "csASCII"
        ],
        Some("US-ASCII")
    ),
    info!("EUC-JP", ["EUC", "EUC_JP", "eucJP", "x-euc-jp"], Some("EUC-JP")),
    info!("SJIS", ["x-sjis", "SHIFT-JIS"], Some("Shift_JIS")),
    info!("eucJP-win", ["eucJP-open", "eucJP-ms"], Some("EUC-JP")),
    info!("EUC-JP-2004", ["EUC_JP-2004"], Some("EUC-JP")),
    info!(
        "SJIS-Mobile#DOCOMO",
        ["SJIS-DOCOMO", "shift_jis-imode", "x-sjis-emoji-docomo"],
        Some("Shift_JIS")
    ),
    info!(
        "SJIS-Mobile#KDDI",
        ["SJIS-KDDI", "shift_jis-kddi", "x-sjis-emoji-kddi"],
        Some("Shift_JIS")
    ),
    info!(
        "SJIS-Mobile#SOFTBANK",
        ["SJIS-SOFTBANK", "shift_jis-softbank", "x-sjis-emoji-softbank"],
        Some("Shift_JIS")
    ),
    info!("SJIS-mac", ["MacJapanese", "x-Mac-Japanese"], Some("Shift_JIS")),
    info!("SJIS-2004", ["SJIS2004", "Shift_JIS-2004"], Some("Shift_JIS")),
    info!(
        "UTF-8-Mobile#DOCOMO",
        ["UTF-8-DOCOMO", "UTF8-DOCOMO"],
        Some("UTF-8")
    ),
    info!("UTF-8-Mobile#KDDI-A", [], Some("UTF-8")),
    info!(
        "UTF-8-Mobile#KDDI-B",
        ["UTF-8-Mobile#KDDI", "UTF-8-KDDI", "UTF8-KDDI"],
        Some("UTF-8")
    ),
    info!(
        "UTF-8-Mobile#SOFTBANK",
        ["UTF-8-SOFTBANK", "UTF8-SOFTBANK"],
        Some("UTF-8")
    ),
    info!("CP932", ["MS932", "Windows-31J", "MS_Kanji"], Some("Shift_JIS")),
    info!("SJIS-win", ["SJIS-ms", "SJIS-open"], Some("Shift_JIS")),
    info!("CP51932", ["cp51932"], Some("CP51932")),
    info!("JIS", [], Some("ISO-2022-JP")),
    info!("ISO-2022-JP", [], Some("ISO-2022-JP")),
    info!("ISO-2022-JP-MS", ["ISO2022JPMS"], Some("ISO-2022-JP")),
    info!("GB18030", ["gb-18030", "gb-18030-2000"], Some("GB18030")),
    info!("GB18030-2022", [], Some("GB18030-2022")),
    info!("Windows-1252", ["cp1252"], Some("Windows-1252")),
    info!(
        "Windows-1254",
        ["CP1254", "CP-1254", "WINDOWS-1254"],
        Some("Windows-1254")
    ),
    info!("ISO-8859-1", ["ISO8859-1", "latin1"], Some("ISO-8859-1")),
    info!("ISO-8859-2", ["ISO8859-2", "latin2"], Some("ISO-8859-2")),
    info!("ISO-8859-3", ["ISO8859-3", "latin3"], Some("ISO-8859-3")),
    info!("ISO-8859-4", ["ISO8859-4", "latin4"], Some("ISO-8859-4")),
    info!("ISO-8859-5", ["ISO8859-5", "cyrillic"], Some("ISO-8859-5")),
    info!("ISO-8859-6", ["ISO8859-6", "arabic"], Some("ISO-8859-6")),
    info!("ISO-8859-7", ["ISO8859-7", "greek"], Some("ISO-8859-7")),
    info!("ISO-8859-8", ["ISO8859-8", "hebrew"], Some("ISO-8859-8")),
    info!("ISO-8859-9", ["ISO8859-9", "latin5"], Some("ISO-8859-9")),
    info!("ISO-8859-10", ["ISO8859-10", "latin6"], Some("ISO-8859-10")),
    info!("ISO-8859-13", ["ISO8859-13"], Some("ISO-8859-13")),
    info!("ISO-8859-14", ["ISO8859-14", "latin8"], Some("ISO-8859-14")),
    info!("ISO-8859-15", ["ISO8859-15"], Some("ISO-8859-15")),
    info!("ISO-8859-16", ["ISO8859-16"], Some("ISO-8859-16")),
    info!(
        "EUC-CN",
        ["CN-GB", "EUC_CN", "eucCN", "x-euc-cn", "gb2312"],
        Some("CN-GB")
    ),
    info!("CP936", ["CP-936", "GBK"], Some("CP936")),
    info!("HZ", [], Some("HZ-GB-2312")),
    info!("EUC-TW", ["EUC_TW", "eucTW", "x-euc-tw"], Some("EUC-TW")),
    info!("BIG-5", ["CN-BIG5", "BIG-FIVE", "BIGFIVE"], Some("BIG5")),
    info!("CP950", [], Some("BIG5")),
    info!("EUC-KR", ["EUC_KR", "eucKR", "x-euc-kr"], Some("EUC-KR")),
    info!("UHC", ["CP949"], Some("UHC")),
    info!("ISO-2022-KR", [], Some("ISO-2022-KR")),
    info!(
        "Windows-1251",
        ["CP1251", "CP-1251", "WINDOWS-1251"],
        Some("Windows-1251")
    ),
    info!("CP866", ["CP-866", "IBM866", "IBM-866"], Some("CP866")),
    info!("KOI8-R", ["KOI8R"], Some("KOI8-R")),
    info!("KOI8-U", ["KOI8U"], Some("KOI8-U")),
    info!(
        "ArmSCII-8",
        ["ArmSCII8", "ARMSCII-8", "ARMSCII8"],
        Some("ArmSCII-8")
    ),
    info!("CP850", ["CP-850", "IBM850", "IBM-850"], Some("CP850")),
    info!("ISO-2022-JP-2004", [], Some("ISO-2022-JP-2004")),
    info!(
        "ISO-2022-JP-MOBILE#KDDI",
        ["ISO-2022-JP-KDDI"],
        Some("ISO-2022-JP")
    ),
    info!(
        "CP50220",
        ["cp50220raw", "cp50220-raw", "JIS-ms"],
        Some("ISO-2022-JP")
    ),
    info!("CP50221", [], Some("ISO-2022-JP")),
    info!("CP50222", [], Some("ISO-2022-JP")),
];

/// Every encoding, in `mb_list_encodings()` order.
pub static ALL: [Encoding; 79] = {
    let mut all = [Encoding::Base64; 79];
    let mut i = 0;
    while i < 79 {
        all[i] = Encoding::from_index(i);
        i += 1;
    }
    all
};

impl Encoding {
    const fn from_index(i: usize) -> Encoding {
        assert!(i < 79);
        // SAFETY: `Encoding` is `repr(u8)` with 79 dataless variants numbered
        // 0..79 in declaration order, and `i` is in range.
        unsafe { core::mem::transmute::<u8, Encoding>(i as u8) }
    }

    /// The static facts about this encoding.
    #[inline]
    pub fn info(self) -> &'static Info {
        &INFO[self as usize]
    }

    /// The canonical name, as `mb_list_encodings()` prints it.
    #[inline]
    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// The aliases, as `mb_encoding_aliases()` returns them.
    #[inline]
    pub fn aliases(self) -> &'static [&'static str] {
        self.info().aliases
    }

    /// The MIME name `mb_preferred_mime_name()` returns, if any.
    #[inline]
    pub fn mime_name(self) -> Option<&'static str> {
        self.info().mime
    }

    /// Looks an encoding up by name, alias or MIME name, ignoring ASCII case.
    ///
    /// Leading and trailing whitespace is not trimmed here (encoding lists
    /// trim it, see [`parse_list`]).
    pub fn from_name(name: &[u8]) -> Option<Encoding> {
        // Fast path for the common spellings.
        if name.eq_ignore_ascii_case(b"UTF-8") {
            return Some(Encoding::Utf8);
        }
        ALL.iter().copied().find(|e| {
            let i = e.info();
            i.name.as_bytes().eq_ignore_ascii_case(name)
                || i.aliases
                    .iter()
                    .any(|a| a.as_bytes().eq_ignore_ascii_case(name))
                || i.mime
                    .is_some_and(|m| m.as_bytes().eq_ignore_ascii_case(name))
        })
    }

    /// Whether the encoding handles deprecated byte-level transforms
    /// (Base64, UUENCODE, Quoted-Printable, HTML entities).
    ///
    /// PHP 8.2 deprecated handling these via mbstring (PHP manual).
    pub fn is_deprecated_transform(self) -> bool {
        matches!(
            self,
            Encoding::Base64
                | Encoding::Uuencode
                | Encoding::HtmlEntities
                | Encoding::QuotedPrintable
        )
    }

    /// The deprecation message PHP 8.5 emits when the encoding is used.
    pub fn deprecation_message(self) -> Option<&'static str> {
        Some(match self {
            Encoding::Base64 => {
                "Handling Base64 via mbstring is deprecated; use base64_encode/base64_decode instead"
            }
            Encoding::Uuencode => {
                "Handling Uuencode via mbstring is deprecated; use convert_uuencode/convert_uudecode instead"
            }
            Encoding::HtmlEntities => {
                "Handling HTML entities via mbstring is deprecated; use htmlspecialchars, htmlentities, or mb_encode_numericentity/mb_decode_numericentity instead"
            }
            Encoding::QuotedPrintable => {
                "Handling QPrint via mbstring is deprecated; use quoted_printable_encode/quoted_printable_decode instead"
            }
            _ => return None,
        })
    }

    /// Whether every ASCII byte stands for itself in this encoding, and bytes
    /// below 0x80 never occur inside multi-byte characters.
    pub fn is_ascii_compatible(self) -> bool {
        use Encoding::*;
        !matches!(
            self,
            Base64
                | Uuencode
                | HtmlEntities
                | QuotedPrintable
                | Ucs4
                | Ucs4Be
                | Ucs4Le
                | Ucs2
                | Ucs2Be
                | Ucs2Le
                | Utf32
                | Utf32Be
                | Utf32Le
                | Utf16
                | Utf16Be
                | Utf16Le
                | Utf7
                | Utf7Imap
                | Jis
                | Iso2022Jp
                | Iso2022JpMs
                | Iso2022Jp2004
                | Iso2022JpKddi
                | Cp50220
                | Cp50221
                | Cp50222
                | Hz
                | Iso2022Kr
                | Sjis
                | SjisDocomo
                | SjisKddi
                | SjisSoftbank
                | SjisMac
                | Sjis2004
                | Cp932
                | SjisWin
                | Big5
                | Cp950
                | Cp936
                | Gb18030
                | Gb18030_2022
                | Uhc
        )
    }

    /// Whether the encoding is one of the Unicode encodings whose code units
    /// are fixed-size (UCS-2/UCS-4 and their byte-order variants).
    pub fn fixed_unit_width(self) -> Option<usize> {
        use Encoding::*;
        match self {
            Ucs4 | Ucs4Be | Ucs4Le | Utf32 | Utf32Be | Utf32Le => Some(4),
            Ucs2 | Ucs2Be | Ucs2Le => Some(2),
            _ => None,
        }
    }
}

/// Splits a comma-separated encoding list the way mbstring does: each item is
/// trimmed of spaces and tabs, and `auto` expands to `auto` (the caller
/// substitutes the language's detection order).
///
/// Returns the items, with `None` for an item that names no encoding.
pub fn split_list(list: &[u8]) -> Vec<&[u8]> {
    list.split(|&b| b == b',')
        .map(|item| item.trim_ascii())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trip() {
        for (i, e) in ALL.iter().enumerate() {
            assert_eq!(*e as usize, i);
        }
        assert_eq!(ALL[78], Encoding::Cp50222);
    }

    #[test]
    fn lookup() {
        // observed on PHP 8.5: mb_convert_encoding accepts these
        let ok: &[(&str, Encoding)] = &[
            ("Shift_JIS", Encoding::Sjis),
            ("ms_kanji", Encoding::Cp932),
            ("utf8", Encoding::Utf8),
            ("binary", Encoding::EightBit),
            ("byte2be", Encoding::Ucs2Be),
            ("UNICODE", Encoding::Ucs2),
            ("gb2312", Encoding::EucCn),
            ("GBK", Encoding::Cp936),
            ("HZ-GB-2312", Encoding::Hz),
            ("BIG5", Encoding::Big5),
            ("cp950", Encoding::Cp950),
            ("MUTF-7", Encoding::Utf7Imap),
            ("sjis-mobile#docomo", Encoding::SjisDocomo),
            ("UTF-8-Mobile#KDDI", Encoding::Utf8KddiB),
            ("x-uuencode", Encoding::Uuencode),
            ("CP949", Encoding::Uhc),
            ("EUC", Encoding::EucJp),
            ("cp1252", Encoding::Cp1252),
            ("latin8", Encoding::Iso8859_14),
        ];
        for (n, e) in ok {
            assert_eq!(Encoding::from_name(n.as_bytes()), Some(*e), "{n}");
        }
        for n in [
            "UTF_8",
            "pass",
            "wchar",
            "ISO2022JP",
            "ISO_8859-1",
            "iso88591",
            "ISO-8859-11",
            "Windows-1250",
            " UTF-8",
            "",
            "auto",
        ] {
            assert_eq!(Encoding::from_name(n.as_bytes()), None, "{n}");
        }
    }
}
