//! `UConverter` (PHP manual: UConverter): charset conversion through
//! Unicode with ICU's converter names, types, substitution and
//! callback reasons (ICU API documentation, ucnv.h / ucnv_err.h), on the
//! goro-charset codecs.

use goro_charset::Encoding;
use goro_charset::codec::{Subst, SubstMode};

/// ICU's UConverterType values.
pub const UNSUPPORTED_CONVERTER: i64 = -1;
pub const SBCS: i64 = 0;
pub const DBCS: i64 = 1;
pub const MBCS: i64 = 2;
pub const LATIN_1: i64 = 3;
pub const UTF8: i64 = 4;
pub const UTF16_BIG_ENDIAN: i64 = 5;
pub const UTF16_LITTLE_ENDIAN: i64 = 6;
pub const UTF32_BIG_ENDIAN: i64 = 7;
pub const UTF32_LITTLE_ENDIAN: i64 = 8;
pub const EBCDIC_STATEFUL: i64 = 9;
pub const ISO_2022: i64 = 10;
pub const HZ: i64 = 23;
pub const SCSU: i64 = 24;
pub const ISCII: i64 = 25;
pub const US_ASCII: i64 = 26;
pub const UTF7: i64 = 27;
pub const BOCU1: i64 = 28;
pub const UTF16: i64 = 29;
pub const UTF32: i64 = 30;
pub const CESU8: i64 = 31;
pub const IMAP_MAILBOX: i64 = 32;

/// A converter: its ICU name, aliases, type and codec.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conv {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub ty: i64,
    pub enc: Option<Encoding>,
    /// A single-byte charset of goro-charset's iconv tables (by its
    /// glibc name).
    pub iconv: Option<&'static str>,
}

macro_rules! conv {
    ($name:expr, $ty:expr, $enc:expr, [$($a:expr),*]) => {
        Conv { name: $name, aliases: &[$name, $($a),*], ty: $ty, enc: $enc, iconv: None }
    };
}

macro_rules! sbcs {
    ($name:expr, $glibc:expr, [$($a:expr),*]) => {
        Conv { name: $name, aliases: &[$name, $($a),*], ty: SBCS, enc: None, iconv: Some($glibc) }
    };
}

/// The converters (ICU's alias table names for the codecs goro has).
pub static CONVERTERS: &[Conv] = &[
    conv!(
        "UTF-8",
        UTF8,
        Some(Encoding::Utf8),
        [
            "ibm-1208",
            "ibm-1209",
            "ibm-5304",
            "ibm-5305",
            "ibm-13496",
            "ibm-13497",
            "ibm-17592",
            "ibm-17593",
            "windows-65001",
            "cp1208",
            "x-UTF_8J",
            "unicode-1-1-utf-8",
            "unicode-2-0-utf-8"
        ]
    ),
    conv!(
        "UTF-16",
        UTF16,
        Some(Encoding::Utf16),
        [
            "ISO-10646-UCS-2",
            "ibm-1204",
            "ibm-1205",
            "unicode",
            "csUnicode",
            "ucs-2"
        ]
    ),
    conv!(
        "UTF-16BE",
        UTF16_BIG_ENDIAN,
        Some(Encoding::Utf16Be),
        [
            "x-utf-16be",
            "UnicodeBigUnmarked",
            "ibm-1200",
            "ibm-1201",
            "ibm-13488",
            "ibm-13489",
            "ibm-17584",
            "ibm-17585",
            "ibm-21680",
            "ibm-21681",
            "ibm-25776",
            "ibm-25777",
            "ibm-29872",
            "ibm-29873",
            "ibm-61955",
            "ibm-61956",
            "windows-1201",
            "cp1200",
            "cp1201",
            "UTF16_BigEndian"
        ]
    ),
    conv!(
        "UTF-16LE",
        UTF16_LITTLE_ENDIAN,
        Some(Encoding::Utf16Le),
        [
            "x-utf-16le",
            "UnicodeLittleUnmarked",
            "ibm-1202",
            "ibm-1203",
            "ibm-13490",
            "ibm-13491",
            "ibm-17586",
            "ibm-17587",
            "ibm-21682",
            "ibm-21683",
            "ibm-25778",
            "ibm-25779",
            "ibm-29874",
            "ibm-29875",
            "UTF16_LittleEndian",
            "windows-1200"
        ]
    ),
    conv!(
        "UTF-32",
        UTF32,
        Some(Encoding::Utf32),
        ["ISO-10646-UCS-4", "ibm-1236", "ibm-1237", "csUCS4", "ucs-4"]
    ),
    conv!(
        "UTF-32BE",
        UTF32_BIG_ENDIAN,
        Some(Encoding::Utf32Be),
        ["UTF32_BigEndian", "ibm-1232", "ibm-1233", "ibm-9424"]
    ),
    conv!(
        "UTF-32LE",
        UTF32_LITTLE_ENDIAN,
        Some(Encoding::Utf32Le),
        ["UTF32_LittleEndian", "ibm-1234", "ibm-1235"]
    ),
    conv!("UTF-7", UTF7, Some(Encoding::Utf7), ["windows-65000"]),
    conv!("CESU-8", CESU8, None, ["ibm-9400"]),
    conv!(
        "IMAP-mailbox-name",
        IMAP_MAILBOX,
        Some(Encoding::Utf7Imap),
        []
    ),
    conv!(
        "US-ASCII",
        US_ASCII,
        Some(Encoding::Ascii),
        [
            "ASCII",
            "ANSI_X3.4-1968",
            "ANSI_X3.4-1986",
            "ISO_646.irv:1991",
            "iso_646.irv:1983",
            "ISO646-US",
            "us",
            "csASCII",
            "iso-ir-6",
            "cp367",
            "ascii7",
            "646",
            "windows-20127",
            "ibm-367"
        ]
    ),
    conv!(
        "ISO-8859-1",
        LATIN_1,
        Some(Encoding::Iso8859_1),
        [
            "ibm-819",
            "IBM819",
            "cp819",
            "latin1",
            "8859_1",
            "csISOLatin1",
            "iso-ir-100",
            "ISO_8859-1:1987",
            "l1",
            "819"
        ]
    ),
    conv!(
        "ibm-912_P100-1995",
        SBCS,
        Some(Encoding::Iso8859_2),
        [
            "ibm-912",
            "ISO-8859-2",
            "ISO_8859-2:1987",
            "latin2",
            "csISOLatin2",
            "iso-ir-101",
            "l2",
            "8859_2",
            "cp912",
            "912",
            "windows-28592"
        ]
    ),
    conv!(
        "ibm-913_P100-2000",
        SBCS,
        Some(Encoding::Iso8859_3),
        [
            "ibm-913",
            "ISO-8859-3",
            "ISO_8859-3:1988",
            "latin3",
            "csISOLatin3",
            "iso-ir-109",
            "l3",
            "8859_3",
            "cp913",
            "913",
            "windows-28593"
        ]
    ),
    conv!(
        "ibm-914_P100-1995",
        SBCS,
        Some(Encoding::Iso8859_4),
        [
            "ibm-914",
            "ISO-8859-4",
            "latin4",
            "csISOLatin4",
            "iso-ir-110",
            "ISO_8859-4:1988",
            "l4",
            "8859_4",
            "cp914",
            "914",
            "windows-28594"
        ]
    ),
    conv!(
        "ibm-915_P100-1995",
        SBCS,
        Some(Encoding::Iso8859_5),
        [
            "ibm-915",
            "ISO-8859-5",
            "cyrillic",
            "csISOLatinCyrillic",
            "iso-ir-144",
            "ISO_8859-5:1988",
            "8859_5",
            "cp915",
            "915",
            "windows-28595"
        ]
    ),
    conv!(
        "ibm-1089_P100-1995",
        SBCS,
        Some(Encoding::Iso8859_6),
        [
            "ibm-1089",
            "ISO-8859-6",
            "arabic",
            "csISOLatinArabic",
            "iso-ir-127",
            "ISO_8859-6:1987",
            "ECMA-114",
            "ASMO-708",
            "8859_6",
            "cp1089",
            "1089",
            "windows-28596"
        ]
    ),
    conv!(
        "ibm-9005_X110-2007",
        SBCS,
        Some(Encoding::Iso8859_7),
        [
            "ibm-9005",
            "ISO-8859-7",
            "8859_7",
            "greek",
            "greek8",
            "ELOT_928",
            "ECMA-118",
            "csISOLatinGreek",
            "iso-ir-126",
            "ISO_8859-7:1987",
            "windows-28597",
            "sun_eu_greek"
        ]
    ),
    conv!(
        "ibm-5012_P100-1999",
        SBCS,
        Some(Encoding::Iso8859_8),
        [
            "ibm-5012",
            "ISO-8859-8",
            "hebrew",
            "csISOLatinHebrew",
            "iso-ir-138",
            "ISO_8859-8:1988",
            "ISO-8859-8-I",
            "ISO-8859-8-E",
            "8859_8",
            "windows-28598",
            "hebrew8"
        ]
    ),
    conv!(
        "ibm-920_P100-1995",
        SBCS,
        Some(Encoding::Iso8859_9),
        [
            "ibm-920",
            "ISO-8859-9",
            "latin5",
            "csISOLatin5",
            "iso-ir-148",
            "ISO_8859-9:1989",
            "l5",
            "8859_9",
            "cp920",
            "920",
            "windows-28599",
            "ECMA-128",
            "turkish8",
            "turkish"
        ]
    ),
    conv!(
        "iso-8859_10-1998",
        SBCS,
        Some(Encoding::Iso8859_10),
        [
            "ISO-8859-10",
            "iso-ir-157",
            "l6",
            "ISO_8859-10:1992",
            "csISOLatin6",
            "latin6"
        ]
    ),
    conv!(
        "ibm-921_P100-1995",
        SBCS,
        Some(Encoding::Iso8859_13),
        [
            "ibm-921",
            "ISO-8859-13",
            "8859_13",
            "windows-28603",
            "cp921",
            "921"
        ]
    ),
    conv!(
        "iso-8859_14-1998",
        SBCS,
        Some(Encoding::Iso8859_14),
        [
            "ISO-8859-14",
            "iso-ir-199",
            "ISO_8859-14:1998",
            "latin8",
            "iso-celtic",
            "l8"
        ]
    ),
    conv!(
        "ibm-923_P100-1998",
        SBCS,
        Some(Encoding::Iso8859_15),
        [
            "ibm-923",
            "ISO-8859-15",
            "Latin-9",
            "l9",
            "8859_15",
            "latin0",
            "csisolatin0",
            "csisolatin9",
            "iso8859_15_fdis",
            "cp923",
            "923",
            "windows-28605"
        ]
    ),
    conv!(
        "iso-8859_16-2001",
        SBCS,
        Some(Encoding::Iso8859_16),
        [
            "ISO-8859-16",
            "iso-ir-226",
            "ISO_8859-16:2001",
            "latin10",
            "l10"
        ]
    ),
    conv!(
        "ibm-5348_P100-1997",
        SBCS,
        Some(Encoding::Cp1252),
        ["ibm-5348", "windows-1252", "cp1252"]
    ),
    conv!(
        "ibm-5350_P100-1998",
        SBCS,
        Some(Encoding::Cp1254),
        ["ibm-5350", "windows-1254", "cp1254"]
    ),
    conv!(
        "ibm-5347_P100-1998",
        SBCS,
        Some(Encoding::Cp1251),
        ["ibm-5347", "windows-1251", "cp1251", "ANSI1251"]
    ),
    conv!(
        "ibm-878_P100-1996",
        SBCS,
        Some(Encoding::Koi8R),
        [
            "ibm-878",
            "KOI8-R",
            "koi8",
            "csKOI8R",
            "windows-20866",
            "cp878"
        ]
    ),
    conv!(
        "ibm-1168_P100-2002",
        SBCS,
        Some(Encoding::Koi8U),
        ["ibm-1168", "KOI8-U", "windows-21866"]
    ),
    conv!(
        "ibm-866_P100-1995",
        SBCS,
        Some(Encoding::Cp866),
        [
            "ibm-866",
            "IBM866",
            "cp866",
            "csIBM866",
            "windows-866",
            "866"
        ]
    ),
    conv!(
        "ibm-850_P100-1995",
        SBCS,
        Some(Encoding::Cp850),
        [
            "ibm-850",
            "IBM850",
            "cp850",
            "850",
            "csPC850Multilingual",
            "windows-850"
        ]
    ),
    conv!("ARMSCII-8", SBCS, Some(Encoding::ArmScii8), []),
    conv!(
        "ibm-943_P15A-2003",
        MBCS,
        Some(Encoding::Sjis),
        [
            "ibm-943",
            "Shift_JIS",
            "MS_Kanji",
            "csShiftJIS",
            "windows-31j",
            "csWindows31J",
            "x-sjis",
            "x-ms-cp932",
            "cp932",
            "windows-932",
            "cp943c",
            "IBM-943C",
            "ms932",
            "pck",
            "sjis",
            "ibm-943_VSUB_VPUA",
            "x-MS932_0213",
            "x-JISAutoDetect"
        ]
    ),
    conv!(
        "ibm-33722_P12A_P12A-2009_U2",
        MBCS,
        Some(Encoding::EucJp),
        [
            "ibm-33722",
            "ibm-5050",
            "EUC-JP",
            "Extended_UNIX_Code_Packed_Format_for_Japanese",
            "csEUCPkdFmtJapanese",
            "X-EUC-JP",
            "eucjis",
            "ujis"
        ]
    ),
    conv!(
        "ISO_2022,locale=ja,version=0",
        ISO_2022,
        Some(Encoding::Iso2022Jp),
        [
            "ISO-2022-JP",
            "csISO2022JP",
            "x-windows-iso2022jp",
            "x-windows-50220"
        ]
    ),
    conv!(
        "ISO_2022,locale=ko,version=0",
        ISO_2022,
        Some(Encoding::Iso2022Kr),
        ["ISO-2022-KR", "csISO2022KR"]
    ),
    conv!(
        "ibm-1383_P110-1999",
        MBCS,
        Some(Encoding::EucCn),
        [
            "ibm-1383",
            "GB2312",
            "csGB2312",
            "cp1383",
            "1383",
            "EUC-CN",
            "ibm-eucCN",
            "hp15CN"
        ]
    ),
    conv!(
        "windows-936-2000",
        MBCS,
        Some(Encoding::Cp936),
        ["GBK", "CP936", "MS936", "windows-936"]
    ),
    conv!(
        "gb18030",
        MBCS,
        Some(Encoding::Gb18030),
        ["ibm-1392", "windows-54936", "GB18030"]
    ),
    conv!("HZ", HZ, Some(Encoding::Hz), ["HZ-GB-2312"]),
    conv!(
        "ibm-964_P110-1999",
        MBCS,
        Some(Encoding::EucTw),
        [
            "ibm-964",
            "ibm-eucTW",
            "cns11643",
            "EUC-TW",
            "ibm-964_VPUA",
            "cp964",
            "964"
        ]
    ),
    conv!(
        "windows-950-2000",
        MBCS,
        Some(Encoding::Cp950),
        ["Big5", "csBig5", "windows-950", "x-windows-950"]
    ),
    conv!(
        "ibm-970_P110_P110-2006_U2",
        MBCS,
        Some(Encoding::EucKr),
        [
            "ibm-970",
            "EUC-KR",
            "KS_C_5601-1987",
            "windows-51949",
            "csEUCKR",
            "ibm-eucKR",
            "KSC_5601",
            "5601",
            "cp970",
            "970"
        ]
    ),
    conv!(
        "windows-949-2000",
        MBCS,
        Some(Encoding::Uhc),
        [
            "windows-949",
            "KS_C_5601-1989",
            "csKSC56011987",
            "korean",
            "iso-ir-149",
            "ms949"
        ]
    ),
    sbcs!(
        "ibm-437_P100-1995",
        "CP437",
        [
            "ibm-437",
            "IBM437",
            "cp437",
            "437",
            "csPC8CodePage437",
            "windows-437"
        ]
    ),
    sbcs!(
        "ibm-737_P100-1997",
        "CP737",
        ["ibm-737", "IBM737", "cp737", "windows-737", "737"]
    ),
    sbcs!(
        "ibm-775_P100-1996",
        "CP775",
        [
            "ibm-775",
            "IBM775",
            "cp775",
            "csPC775Baltic",
            "windows-775",
            "775"
        ]
    ),
    sbcs!(
        "ibm-852_P100-1995",
        "CP852",
        [
            "ibm-852",
            "IBM852",
            "cp852",
            "852",
            "csPCp852",
            "windows-852"
        ]
    ),
    sbcs!(
        "ibm-855_P100-1995",
        "CP855",
        [
            "ibm-855",
            "IBM855",
            "cp855",
            "855",
            "csIBM855",
            "csPCp855",
            "windows-855"
        ]
    ),
    sbcs!(
        "ibm-857_P100-1995",
        "CP857",
        [
            "ibm-857",
            "IBM857",
            "cp857",
            "857",
            "csIBM857",
            "windows-857"
        ]
    ),
    sbcs!(
        "ibm-858_P100-1997",
        "CP858",
        [
            "ibm-858",
            "IBM00858",
            "CCSID00858",
            "CP00858",
            "PC-Multilingual-850+euro",
            "cp858",
            "windows-858"
        ]
    ),
    sbcs!(
        "ibm-860_P100-1995",
        "CP860",
        ["ibm-860", "IBM860", "cp860", "860", "csIBM860"]
    ),
    sbcs!(
        "ibm-861_P100-1995",
        "CP861",
        [
            "ibm-861",
            "IBM861",
            "cp861",
            "861",
            "cp-is",
            "csIBM861",
            "windows-861"
        ]
    ),
    sbcs!(
        "ibm-862_P100-1995",
        "CP862",
        [
            "ibm-862",
            "IBM862",
            "cp862",
            "862",
            "csPC862LatinHebrew",
            "DOS-862",
            "windows-862"
        ]
    ),
    sbcs!(
        "ibm-863_P100-1995",
        "CP863",
        ["ibm-863", "IBM863", "cp863", "863", "csIBM863"]
    ),
    sbcs!(
        "ibm-864_X110-1999",
        "CP864",
        ["ibm-864", "IBM864", "cp864", "csIBM864"]
    ),
    sbcs!(
        "ibm-865_P100-1995",
        "CP865",
        ["ibm-865", "IBM865", "cp865", "865", "csIBM865"]
    ),
    sbcs!(
        "ibm-869_P100-1995",
        "CP869",
        [
            "ibm-869",
            "IBM869",
            "cp869",
            "869",
            "cp-gr",
            "csIBM869",
            "windows-869"
        ]
    ),
    sbcs!(
        "ibm-874_P100-1995",
        "CP874",
        [
            "ibm-874",
            "ibm-9066",
            "cp874",
            "TIS-620",
            "tis620.2533",
            "eucTH"
        ]
    ),
    sbcs!(
        "windows-874-2000",
        "CP874",
        ["windows-874", "MS874", "x-windows-874"]
    ),
    sbcs!(
        "ibm-5346_P100-1998",
        "CP1250",
        ["ibm-5346", "windows-1250", "cp1250"]
    ),
    sbcs!(
        "ibm-5349_P100-1998",
        "CP1253",
        ["ibm-5349", "windows-1253", "cp1253"]
    ),
    sbcs!(
        "ibm-5351_P100-1998",
        "CP1255",
        ["ibm-5351", "windows-1255", "cp1255"]
    ),
    sbcs!(
        "ibm-9448_X100-2005",
        "CP1256",
        ["ibm-9448", "windows-1256", "cp1256"]
    ),
    sbcs!(
        "ibm-9449_P100-2002",
        "CP1257",
        ["ibm-9449", "windows-1257", "cp1257"]
    ),
    sbcs!(
        "ibm-5354_P100-1998",
        "CP1258",
        ["ibm-5354", "windows-1258", "cp1258"]
    ),
    sbcs!(
        "macos-0_2-10.2",
        "MACINTOSH",
        [
            "macintosh",
            "mac",
            "csMacintosh",
            "windows-10000",
            "macroman",
            "x-macroman"
        ]
    ),
    sbcs!(
        "macos-6_2-10.4",
        "MAC-GREEK",
        ["x-mac-greek", "windows-10006", "macgr", "x-MacGreek"]
    ),
    sbcs!(
        "macos-7_3-10.2",
        "MAC-CYRILLIC",
        [
            "x-mac-cyrillic",
            "windows-10007",
            "mac-cyrillic",
            "maccy",
            "x-MacCyrillic",
            "x-MacUkraine"
        ]
    ),
    sbcs!(
        "macos-29-10.2",
        "MAC-CENTRALEUROPE",
        [
            "x-mac-centraleurroman",
            "windows-10029",
            "x-mac-ce",
            "macce",
            "maccentraleurope",
            "x-MacCentralEurope"
        ]
    ),
    sbcs!(
        "macos-35-10.2",
        "MAC-TURKISH",
        ["x-mac-turkish", "windows-10081", "mactr", "x-MacTurkish"]
    ),
    sbcs!(
        "ibm-1051_P100-1995",
        "HP-ROMAN8",
        [
            "ibm-1051",
            "hp-roman8",
            "roman8",
            "r8",
            "csHPRoman8",
            "x-roman8"
        ]
    ),
    sbcs!(
        "ibm-1276_P100-1995",
        "ADOBE-STANDARD",
        [
            "ibm-1276",
            "Adobe-Standard-Encoding",
            "csAdobeStandardEncoding"
        ]
    ),
    sbcs!(
        "ibm-37_P100-1995",
        "IBM037",
        [
            "ibm-37",
            "IBM037",
            "ibm-037",
            "ebcdic-cp-us",
            "ebcdic-cp-ca",
            "ebcdic-cp-wt",
            "ebcdic-cp-nl",
            "csIBM037",
            "cp037",
            "037",
            "cpibm37",
            "cp37"
        ]
    ),
    sbcs!(
        "ibm-273_P100-1995",
        "IBM273",
        ["ibm-273", "IBM273", "CP273", "csIBM273", "ebcdic-de", "273"]
    ),
    sbcs!(
        "ibm-277_P100-1995",
        "IBM277",
        [
            "ibm-277",
            "IBM277",
            "cp277",
            "EBCDIC-CP-DK",
            "EBCDIC-CP-NO",
            "csIBM277",
            "ebcdic-dk",
            "277"
        ]
    ),
    sbcs!(
        "ibm-278_P100-1995",
        "IBM278",
        [
            "ibm-278",
            "IBM278",
            "cp278",
            "ebcdic-cp-fi",
            "ebcdic-cp-se",
            "csIBM278",
            "ebcdic-sv",
            "278"
        ]
    ),
    sbcs!(
        "ibm-280_P100-1995",
        "IBM280",
        [
            "ibm-280",
            "IBM280",
            "CP280",
            "ebcdic-cp-it",
            "csIBM280",
            "280"
        ]
    ),
    sbcs!(
        "ibm-284_P100-1995",
        "IBM284",
        [
            "ibm-284",
            "IBM284",
            "CP284",
            "ebcdic-cp-es",
            "csIBM284",
            "cpibm284",
            "284"
        ]
    ),
    sbcs!(
        "ibm-285_P100-1995",
        "IBM285",
        [
            "ibm-285",
            "IBM285",
            "CP285",
            "ebcdic-cp-gb",
            "csIBM285",
            "cpibm285",
            "ebcdic-gb",
            "285"
        ]
    ),
    sbcs!(
        "ibm-297_P100-1995",
        "IBM297",
        [
            "ibm-297",
            "IBM297",
            "cp297",
            "ebcdic-cp-fr",
            "csIBM297",
            "cpibm297",
            "297"
        ]
    ),
    sbcs!(
        "ibm-424_P100-1995",
        "IBM424",
        [
            "ibm-424",
            "IBM424",
            "cp424",
            "ebcdic-cp-he",
            "csIBM424",
            "424"
        ]
    ),
    sbcs!(
        "ibm-500_P100-1995",
        "IBM500",
        [
            "ibm-500",
            "IBM500",
            "CP500",
            "ebcdic-cp-be",
            "csIBM500",
            "ebcdic-cp-ch",
            "500"
        ]
    ),
    sbcs!(
        "ibm-870_P100-1995",
        "IBM870",
        [
            "ibm-870",
            "IBM870",
            "CP870",
            "ebcdic-cp-roece",
            "ebcdic-cp-yu",
            "csIBM870"
        ]
    ),
    sbcs!(
        "ibm-871_P100-1995",
        "IBM871",
        [
            "ibm-871",
            "IBM871",
            "ebcdic-cp-is",
            "csIBM871",
            "CP871",
            "ebcdic-is",
            "871"
        ]
    ),
    sbcs!(
        "ibm-875_P100-1995",
        "IBM875",
        ["ibm-875", "IBM875", "cp875", "875"]
    ),
    sbcs!(
        "ibm-1026_P100-1995",
        "IBM1026",
        ["ibm-1026", "IBM1026", "CP1026", "csIBM1026", "1026"]
    ),
    sbcs!(
        "ibm-1047_P100-1995",
        "IBM1047",
        ["ibm-1047", "IBM1047", "cpibm1047"]
    ),
    sbcs!(
        "ibm-1140_P100-1997",
        "IBM1140",
        [
            "ibm-1140",
            "IBM01140",
            "CCSID01140",
            "CP01140",
            "cp1140",
            "ebcdic-us-37+euro"
        ]
    ),
    sbcs!(
        "ibm-1141_P100-1997",
        "IBM1141",
        [
            "ibm-1141",
            "IBM01141",
            "CCSID01141",
            "CP01141",
            "cp1141",
            "ebcdic-de-273+euro"
        ]
    ),
    sbcs!(
        "ibm-1142_P100-1997",
        "IBM1142",
        [
            "ibm-1142",
            "IBM01142",
            "CCSID01142",
            "CP01142",
            "cp1142",
            "ebcdic-dk-277+euro",
            "ebcdic-no-277+euro"
        ]
    ),
    sbcs!(
        "ibm-1143_P100-1997",
        "IBM1143",
        [
            "ibm-1143",
            "IBM01143",
            "CCSID01143",
            "CP01143",
            "cp1143",
            "ebcdic-fi-278+euro",
            "ebcdic-se-278+euro"
        ]
    ),
    sbcs!(
        "ibm-1144_P100-1997",
        "IBM1144",
        [
            "ibm-1144",
            "IBM01144",
            "CCSID01144",
            "CP01144",
            "cp1144",
            "ebcdic-it-280+euro"
        ]
    ),
    sbcs!(
        "ibm-1145_P100-1997",
        "IBM1145",
        [
            "ibm-1145",
            "IBM01145",
            "CCSID01145",
            "CP01145",
            "cp1145",
            "ebcdic-es-284+euro"
        ]
    ),
    sbcs!(
        "ibm-1146_P100-1997",
        "IBM1146",
        [
            "ibm-1146",
            "IBM01146",
            "CCSID01146",
            "CP01146",
            "cp1146",
            "ebcdic-gb-285+euro"
        ]
    ),
    sbcs!(
        "ibm-1147_P100-1997",
        "IBM1147",
        [
            "ibm-1147",
            "IBM01147",
            "CCSID01147",
            "CP01147",
            "cp1147",
            "ebcdic-fr-297+euro"
        ]
    ),
    sbcs!(
        "ibm-1148_P100-1997",
        "IBM1148",
        [
            "ibm-1148",
            "IBM01148",
            "CCSID01148",
            "CP01148",
            "cp1148",
            "ebcdic-international-500+euro"
        ]
    ),
    sbcs!(
        "ibm-1149_P100-1997",
        "IBM1149",
        [
            "ibm-1149",
            "IBM01149",
            "CCSID01149",
            "CP01149",
            "cp1149",
            "ebcdic-is-871+euro"
        ]
    ),
    sbcs!(
        "ibm-1124_P100-1996",
        "IBM1124",
        ["ibm-1124", "cp1124", "1124"]
    ),
    sbcs!("ibm-1125_P100-1997", "IBM1125", ["ibm-1125", "cp1125"]),
    sbcs!("ibm-1129_P100-1997", "IBM1129", ["ibm-1129"]),
    sbcs!("ibm-1131_P100-1997", "IBM1131", ["ibm-1131", "cp1131"]),
    sbcs!("ibm-1133_P100-1997", "IBM1133", ["ibm-1133"]),
    sbcs!(
        "ibm-922_P100-1999",
        "IBM922",
        ["ibm-922", "IBM922", "cp922", "922"]
    ),
    sbcs!(
        "ibm-1006_P100-1995",
        "IBM1006",
        ["ibm-1006", "IBM1006", "cp1006", "1006"]
    ),
    sbcs!(
        "ibm-1098_P100-1995",
        "IBM1098",
        ["ibm-1098", "IBM1098", "cp1098", "1098"]
    ),
    sbcs!(
        "ibm-856_P100-1995",
        "IBM856",
        ["ibm-856", "IBM856", "cp856", "856"]
    ),
    sbcs!(
        "ibm-1097_P100-1995",
        "IBM1097",
        ["ibm-1097", "cp1097", "1097"]
    ),
    sbcs!(
        "ibm-1112_P100-1995",
        "IBM1112",
        ["ibm-1112", "cp1112", "1112"]
    ),
    sbcs!(
        "ibm-1122_P100-1999",
        "IBM1122",
        ["ibm-1122", "cp1122", "1122"]
    ),
    sbcs!(
        "ibm-1123_P100-1995",
        "IBM1123",
        ["ibm-1123", "cp1123", "1123"]
    ),
    sbcs!("ibm-1132_P100-1998", "IBM1132", ["ibm-1132"]),
    sbcs!("ibm-1160_P100-1999", "IBM1160", ["ibm-1160"]),
    sbcs!("ibm-1161_P100-1999", "IBM1161", ["ibm-1161"]),
    sbcs!("ibm-1162_P100-1999", "IBM1162", ["ibm-1162"]),
    sbcs!("ibm-1163_P100-1999", "IBM1163", ["ibm-1163"]),
    sbcs!("ibm-1164_P100-1999", "IBM1164", ["ibm-1164"]),
    sbcs!("ibm-4517_P100-2005", "IBM4517", ["ibm-4517"]),
    sbcs!("ibm-4899_P100-1998", "IBM4899", ["ibm-4899"]),
    sbcs!("ibm-4909_P100-1999", "IBM4909", ["ibm-4909"]),
    sbcs!("ibm-4971_P100-1999", "IBM4971", ["ibm-4971"]),
    sbcs!("ibm-5123_P100-1999", "IBM5123", ["ibm-5123"]),
    sbcs!("ibm-8482_P100-1999", "IBM8482", ["ibm-8482"]),
    sbcs!("ibm-9067_X100-2005", "IBM9067", ["ibm-9067"]),
    sbcs!(
        "ibm-12712_P100-1998",
        "IBM12712",
        ["ibm-12712", "ebcdic-he"]
    ),
    sbcs!(
        "ibm-16804_X110-1999",
        "IBM16804",
        ["ibm-16804", "ebcdic-ar"]
    ),
    sbcs!("ibm-1153_P100-1999", "IBM1153", ["ibm-1153"]),
    sbcs!("ibm-1154_P100-1999", "IBM1154", ["ibm-1154"]),
    sbcs!("ibm-1155_P100-1999", "IBM1155", ["ibm-1155"]),
    sbcs!("ibm-1156_P100-1999", "IBM1156", ["ibm-1156"]),
    sbcs!("ibm-1157_P100-1999", "IBM1157", ["ibm-1157"]),
    sbcs!("ibm-1158_P100-1999", "IBM1158", ["ibm-1158"]),
];

/// Whether goro-charset has the codec of a converter.
pub fn available(c: &Conv) -> bool {
    match c.iconv {
        Some(n) => goro_charset::iconv::Charset::lookup(n).is_some(),
        None => true,
    }
}

/// ICU's name comparison (ucnv_compareNames): case-insensitive, ignoring
/// everything but letters and digits, with leading zeros of numbers
/// dropped.
fn norm(s: &str) -> String {
    let mut out = String::new();
    let mut prev_digit = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if c == '0' && !prev_digit {
                continue;
            }
            prev_digit = c.is_ascii_digit();
            out.push(c.to_ascii_lowercase());
        } else {
            prev_digit = false;
        }
    }
    out
}

pub fn lookup(name: &str) -> Option<&'static Conv> {
    if name.is_empty() {
        return None;
    }
    let n = norm(name);
    CONVERTERS
        .iter()
        .find(|c| available(c) && c.aliases.iter().any(|a| norm(a) == n))
}

/// ICU's converter-name standards (ucnv_getStandard).
pub const STANDARDS: &[&str] = &["MIME", "IANA", "WINDOWS", "JAVA", ""];

/// The callback reasons (UConverterCallbackReason).
pub const REASON_UNASSIGNED: i64 = 0;
pub const REASON_ILLEGAL: i64 = 1;
pub const REASON_IRREGULAR: i64 = 2;
pub const REASON_RESET: i64 = 3;
pub const REASON_CLOSE: i64 = 4;
pub const REASON_CLONE: i64 = 5;

/// One decoded unit: a code point or the bytes of an invalid sequence.
#[derive(Debug, Clone)]
pub enum Unit {
    Cp(u32),
    Bad(Vec<u8>),
}

/// Decodes `input` (maximal subparts for UTF-8, as ICU 60+ and the
/// Unicode standard section 3.9 recommend).
pub fn decode(c: &Conv, input: &[u8]) -> Vec<Unit> {
    if c.ty == UTF8 || c.ty == CESU8 {
        return decode_utf8(input, c.ty == CESU8);
    }
    if let Some(n) = c.iconv {
        return input
            .iter()
            .map(
                |&b| match iconv_conv(n, "UTF-8", &[b]).and_then(|u| String::from_utf8(u).ok()) {
                    Some(s) if !s.is_empty() => Unit::Cp(s.chars().next().unwrap() as u32),
                    _ => Unit::Bad(vec![b]),
                },
            )
            .collect();
    }
    let Some(enc) = c.enc else {
        return Vec::new();
    };
    goro_charset::convert::decode(input, enc)
        .into_iter()
        .map(|v| {
            if v == goro_charset::codec::ERR {
                Unit::Bad(vec![])
            } else {
                Unit::Cp(v)
            }
        })
        .collect()
}

fn decode_utf8(b: &[u8], cesu: bool) -> Vec<Unit> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let x = b[i];
        let (len, lo, hi) = match x {
            0x00..=0x7F => {
                out.push(Unit::Cp(x as u32));
                i += 1;
                continue;
            }
            0xC2..=0xDF => (2, 0x80, 0xBF),
            0xE0 => (3, 0xA0, 0xBF),
            0xE1..=0xEC | 0xEE..=0xEF => (3, 0x80, 0xBF),
            0xED => (3, 0x80, if cesu { 0xBF } else { 0x9F }),
            0xF0 => (4, 0x90, 0xBF),
            0xF1..=0xF3 => (4, 0x80, 0xBF),
            0xF4 => (4, 0x80, 0x8F),
            _ => {
                out.push(Unit::Bad(vec![x]));
                i += 1;
                continue;
            }
        };
        let mut j = i + 1;
        let mut ok = true;
        while j < i + len {
            let Some(&y) = b.get(j) else {
                ok = false;
                break;
            };
            let (l, h) = if j == i + 1 { (lo, hi) } else { (0x80, 0xBF) };
            if !(l..=h).contains(&y) {
                ok = false;
                break;
            }
            j += 1;
        }
        if !ok {
            out.push(Unit::Bad(b[i..j].to_vec()));
            i = j;
            continue;
        }
        let s = &b[i..j];
        let cp = match len {
            2 => ((s[0] as u32 & 0x1F) << 6) | (s[1] as u32 & 0x3F),
            3 => ((s[0] as u32 & 0x0F) << 12) | ((s[1] as u32 & 0x3F) << 6) | (s[2] as u32 & 0x3F),
            _ => {
                ((s[0] as u32 & 0x07) << 18)
                    | ((s[1] as u32 & 0x3F) << 12)
                    | ((s[2] as u32 & 0x3F) << 6)
                    | (s[3] as u32 & 0x3F)
            }
        };
        out.push(Unit::Cp(cp));
        i = j;
    }
    if cesu {
        // surrogate pairs written as two 3-byte sequences
        let mut merged = Vec::new();
        let mut k = 0;
        while k < out.len() {
            if let (Unit::Cp(h @ 0xD800..=0xDBFF), Some(Unit::Cp(l @ 0xDC00..=0xDFFF))) =
                (&out[k], out.get(k + 1))
            {
                merged.push(Unit::Cp(0x10000 + ((h - 0xD800) << 10) + (l - 0xDC00)));
                k += 2;
                continue;
            }
            merged.push(out[k].clone());
            k += 1;
        }
        return merged;
    }
    out
}

/// Encodes one code point; None when the converter can't represent it.
pub fn encode_cp(c: &Conv, cp: u32) -> Option<Vec<u8>> {
    if c.ty == CESU8 {
        let mut out = Vec::new();
        let mut put = |u: u32| {
            if let Some(ch) = char::from_u32(u) {
                let mut b = [0; 4];
                out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
            } else {
                out.push(0xE0 | (u >> 12) as u8);
                out.push(0x80 | ((u >> 6) & 0x3F) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
        };
        if cp >= 0x10000 {
            let v = cp - 0x10000;
            put(0xD800 + (v >> 10));
            put(0xDC00 + (v & 0x3FF));
        } else {
            put(cp);
        }
        return Some(out);
    }
    if let Some(n) = c.iconv {
        let ch = char::from_u32(cp)?;
        let mut b = [0; 4];
        return iconv_conv("UTF-8", n, ch.encode_utf8(&mut b).as_bytes()).filter(|v| !v.is_empty());
    }
    let enc = c.enc?;
    let sub = Subst {
        mode: SubstMode::None,
        ch: 0,
    };
    let r = goro_charset::convert::encode(&[cp], enc, &sub);
    (r.illegal == 0).then_some(r.bytes)
}

fn iconv_conv(from: &str, to: &str, input: &[u8]) -> Option<Vec<u8>> {
    use goro_charset::iconv::{Charset, Flags, convert};
    let f = Charset::lookup(from)?;
    let t = Charset::lookup(to)?;
    convert(
        f,
        t,
        Flags {
            translit: false,
            ignore: false,
        },
        input,
    )
    .ok()
}

/// Encodes code points (for stateful encodings in one go).
pub fn encode_all(c: &Conv, cps: &[u32]) -> Vec<u8> {
    match c.enc {
        Some(enc) if c.ty != CESU8 && c.iconv.is_none() => {
            goro_charset::convert::encode(cps, enc, &Subst::default()).bytes
        }
        _ => cps
            .iter()
            .flat_map(|&cp| encode_cp(c, cp).unwrap_or_default())
            .collect(),
    }
}

/// The default substitution bytes of a converter (ICU's subchar: SUB
/// for single-byte charsets, U+FFFD in Unicode encodings).
pub fn default_subst(c: &Conv) -> Vec<u8> {
    match c.ty {
        UTF8 | UTF16 | UTF16_BIG_ENDIAN | UTF16_LITTLE_ENDIAN | UTF32 | UTF32_BIG_ENDIAN
        | UTF32_LITTLE_ENDIAN | UTF7 | CESU8 | IMAP_MAILBOX => {
            encode_cp(c, 0xFFFD).unwrap_or_else(|| vec![0x1A])
        }
        _ => vec![0x1A],
    }
}

/// Whether `s` can be a converter's substitution (ICU: 1 to 4 bytes, one
/// byte for single-byte charsets).
pub fn valid_subst(c: &Conv, s: &[u8]) -> bool {
    if s.is_empty() || s.len() > 4 {
        return false;
    }
    !matches!(c.ty, SBCS | LATIN_1 | US_ASCII) || s.len() == 1
}

pub fn reason_text(r: i64) -> &'static str {
    match r {
        REASON_UNASSIGNED => "REASON_UNASSIGNED",
        REASON_ILLEGAL => "REASON_ILLEGAL",
        REASON_IRREGULAR => "REASON_IRREGULAR",
        REASON_RESET => "REASON_RESET",
        REASON_CLOSE => "REASON_CLOSE",
        REASON_CLONE => "REASON_CLONE",
        _ => "",
    }
}
