// HTML entity functions: htmlspecialchars, htmlentities, html_entity_decode,
// htmlspecialchars_decode, get_html_translation_table.
//
// PHP's html.c semantics:
//   - flags lower 2 bits = quote style (NONE=0, SINGLE=1, DOUBLE=2, BOTH=3)
//   - bit 2 (4)   = ENT_IGNORE: drop invalid sequences
//   - bit 3 (8)   = ENT_SUBSTITUTE: replace invalid with U+FFFD
//   - bits 4|5    = document type (HTML401=0, XML1=16, XHTML=32, HTML5=48)
//   - bit 7 (128) = ENT_HTML_SUBSTITUTE_DISALLOWED_CHARS
//
// Charset detection:
//   - explicit `encoding` arg used if non-null/empty
//   - empty string falls back to ini default_charset (we default to UTF-8)
//   - unknown charset emits a warning and assumes UTF-8

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::html_tables::*;

// PHP constants from html.h
const ENT_HTML_QUOTE_SINGLE: i64 = 1;
const ENT_HTML_QUOTE_DOUBLE: i64 = 2;
const ENT_HTML_IGNORE_ERRORS: i64 = 4;
const ENT_HTML_SUBSTITUTE_ERRORS: i64 = 8;
const ENT_HTML_DOC_TYPE_MASK: i64 = 16 | 32;
const ENT_HTML_DOC_HTML401: i64 = 0;
const ENT_HTML_DOC_XML1: i64 = 16;
const ENT_HTML_DOC_XHTML: i64 = 32;
const ENT_HTML_DOC_HTML5: i64 = 16 | 32;
const ENT_HTML_SUBSTITUTE_DISALLOWED_CHARS: i64 = 128;

/// Per-doctype rule: is this Unicode codepoint allowed verbatim in output?
/// Maps the C `unicode_cp_is_allowed` from PHP's html.c.
fn unicode_cp_is_allowed(cp: u32, doctype: i64) -> bool {
    match doctype {
        ENT_HTML_DOC_HTML401 => {
            (0x20..=0x7E).contains(&cp)
                || cp == 0x0A
                || cp == 0x09
                || cp == 0x0D
                || (0xA0..=0xD7FF).contains(&cp)
                || (0xE000..=0x10FFFF).contains(&cp)
        }
        ENT_HTML_DOC_HTML5 => {
            (0x20..=0x7E).contains(&cp)
                || ((0x09..=0x0D).contains(&cp) && cp != 0x0B)
                || (0xA0..=0xD7FF).contains(&cp)
                || ((0xE000..=0x10FFFF).contains(&cp)
                    && (cp & 0xFFFF) < 0xFFFE
                    && !(0xFDD0..=0xFDEF).contains(&cp))
        }
        ENT_HTML_DOC_XHTML | ENT_HTML_DOC_XML1 => {
            (0x20..=0xD7FF).contains(&cp)
                || cp == 0x0A
                || cp == 0x09
                || cp == 0x0D
                || ((0xE000..=0x10FFFF).contains(&cp) && cp != 0xFFFE && cp != 0xFFFF)
        }
        _ => true,
    }
}

/// Per-doctype rule: is this Unicode codepoint allowed as a numeric character
/// reference (`&#xxx;`)? More permissive than `unicode_cp_is_allowed`.
fn numeric_entity_is_allowed(cp: u32, doctype: i64) -> bool {
    match doctype {
        ENT_HTML_DOC_HTML401 => cp <= 0x10FFFF,
        ENT_HTML_DOC_HTML5 => {
            (0x20..=0x7E).contains(&cp)
                || ((0x09..=0x0C).contains(&cp) && cp != 0x0B)
                || ((0xA0..=0x10FFFF).contains(&cp)
                    && (cp & 0xFFFF) < 0xFFFE
                    && !(0xFDD0..=0xFDEF).contains(&cp))
        }
        ENT_HTML_DOC_XHTML | ENT_HTML_DOC_XML1 => unicode_cp_is_allowed(cp, doctype),
        _ => true,
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Charset {
    Utf8,
    Iso88591,
    Iso885915,
    Iso88595,
    Cp1252,
    Cp1251,
    Cp866,
    Koi8r,
    MacRoman,
    Big5,
    Big5HkScs,
    Gb2312,
    Sjis,
    EucJp,
}

impl Charset {
    /// CHARSET_PARTIAL_SUPPORT(): the multi-byte charsets without Unicode
    /// mappings (only the ASCII range is known).
    fn is_partial(self) -> bool {
        matches!(
            self,
            Charset::Big5 | Charset::Big5HkScs | Charset::Gb2312 | Charset::Sjis | Charset::EucJp
        )
    }

    /// CHARSET_UNICODE_COMPAT(): code units are Unicode code points.
    fn is_unicode_compat(self) -> bool {
        matches!(self, Charset::Utf8 | Charset::Iso88591)
    }

    /// enc_to_uni_index[]: the single-byte charset's byte -> Unicode table
    /// (0 marks an unmapped byte, see to_unicode()).
    fn to_uni_table(self) -> Option<&'static [u32; 256]> {
        Some(match self {
            Charset::Iso88591 => &CHARSET_ISO_8859_1,
            Charset::Iso885915 => &CHARSET_ISO_8859_15,
            Charset::Iso88595 => &CHARSET_ISO_8859_5,
            Charset::Cp1252 => &CHARSET_CP1252,
            Charset::Cp1251 => &CHARSET_CP1251,
            Charset::Cp866 => &CHARSET_CP866,
            Charset::Koi8r => &CHARSET_KOI8R,
            Charset::MacRoman => &CHARSET_MACROMAN,
            _ => return None,
        })
    }

    /// map_to_unicode(): 0xFFFF when the byte has no mapping.
    fn to_unicode(self, table: &[u32; 256], byte: u32) -> u32 {
        let v = table[(byte & 0xFF) as usize];
        // (MacRoman leaves 0x00 unmapped as well)
        if v == 0 && (byte != 0 || self == Charset::MacRoman) {
            0xFFFF
        } else {
            v
        }
    }
}

/// html_tables.h charset_map[] (matched case-insensitively, whole name).
static CHARSET_MAP: &[(&str, Charset)] = &[
    ("ISO-8859-1", Charset::Iso88591),
    ("ISO8859-1", Charset::Iso88591),
    ("ISO-8859-15", Charset::Iso885915),
    ("ISO8859-15", Charset::Iso885915),
    ("utf-8", Charset::Utf8),
    ("cp1252", Charset::Cp1252),
    ("Windows-1252", Charset::Cp1252),
    ("1252", Charset::Cp1252),
    ("BIG5", Charset::Big5),
    ("950", Charset::Big5),
    ("GB2312", Charset::Gb2312),
    ("936", Charset::Gb2312),
    ("BIG5-HKSCS", Charset::Big5HkScs),
    ("Shift_JIS", Charset::Sjis),
    ("SJIS", Charset::Sjis),
    ("932", Charset::Sjis),
    ("SJIS-win", Charset::Sjis),
    ("CP932", Charset::Sjis),
    ("EUCJP", Charset::EucJp),
    ("EUC-JP", Charset::EucJp),
    ("eucJP-win", Charset::EucJp),
    ("KOI8-R", Charset::Koi8r),
    ("koi8-ru", Charset::Koi8r),
    ("koi8r", Charset::Koi8r),
    ("cp1251", Charset::Cp1251),
    ("Windows-1251", Charset::Cp1251),
    ("win-1251", Charset::Cp1251),
    ("iso8859-5", Charset::Iso88595),
    ("iso-8859-5", Charset::Iso88595),
    ("cp866", Charset::Cp866),
    ("866", Charset::Cp866),
    ("ibm866", Charset::Cp866),
    ("MacRoman", Charset::MacRoman),
];

pub fn register(vm: &mut Vm) {
    vm.register_function(b"htmlspecialchars", htmlspecialchars_fn);
    vm.register_function(b"htmlentities", htmlentities_fn);
    vm.register_function(b"htmlspecialchars_decode", htmlspecialchars_decode_fn);
    vm.register_function(b"html_entity_decode", html_entity_decode_fn);
    vm.register_function(b"get_html_translation_table", get_html_translation_table_fn);
}

/// The charset_map[] entry named `encoding`, if any.
fn determine_charset(encoding: &[u8]) -> Option<Charset> {
    CHARSET_MAP
        .iter()
        .find(|(name, _)| name.as_bytes().eq_ignore_ascii_case(encoding))
        .map(|&(_, cs)| cs)
}

/// Look up the effective default charset from INI settings, mirroring PHP's
/// get_default_charset(): `internal_encoding` first, then `default_charset`.
/// Returns the raw string when set and non-empty.
fn ini_default_charset(vm: &Vm) -> Option<Vec<u8>> {
    for key in [b"internal_encoding".as_ref(), b"default_charset".as_ref()] {
        if let Some(val) = vm.constants.get(key) {
            let s = val.to_php_string();
            if !s.as_bytes().is_empty() {
                return Some(s.as_bytes().to_vec());
            }
        }
    }
    None
}

/// determine_charset(): the explicit charset (empty / null fall back to
/// get_default_charset()), warning about and replacing an unknown one with
/// UTF-8.
fn resolve_charset(vm: &mut Vm, args: &[Value], fn_name: &str, arg_idx: usize) -> Charset {
    let raw: Option<Vec<u8>> = match args.get(arg_idx).map(|v| v.deref()) {
        None | Some(Value::Null) => ini_default_charset(vm),
        Some(v) => {
            let s = v.to_php_string();
            if s.as_bytes().is_empty() {
                ini_default_charset(vm)
            } else {
                Some(s.as_bytes().to_vec())
            }
        }
    };
    let Some(raw) = raw else {
        return Charset::Utf8;
    };
    match determine_charset(&raw) {
        Some(cs) => cs,
        None => {
            vm.emit_warning(&format!(
                "{}(): Charset \"{}\" is not supported, assuming UTF-8",
                fn_name,
                String::from_utf8_lossy(&raw)
            ));
            Charset::Utf8
        }
    }
}

/// valid as single byte character or leading byte
fn utf8_lead(c: u8) -> bool {
    c < 0x80 || (0xC2..=0xF4).contains(&c)
}

fn utf8_trail(c: u8) -> bool {
    (0x80..=0xBF).contains(&c)
}

/// get_next_char(): the code unit sequence at `*pos` (a Unicode code point
/// for UTF-8, the raw byte for single-byte charsets, the bytes packed
/// big-endian for the other multi-byte ones). On an invalid sequence, None
/// with `*pos` advanced past the bytes to report as illegal (UTR #36, 3.6.1,
/// strategy 2: no non-initial byte that is a valid character or lead).
fn get_next_char(cs: Charset, s: &[u8], pos: &mut usize) -> Option<u32> {
    let p = *pos;
    let avail = s.len().saturating_sub(p);
    macro_rules! fail {
        ($adv:expr) => {{
            *pos = p + $adv;
            return None;
        }};
    }
    if avail < 1 {
        fail!(1);
    }
    let c = s[p];
    let this_char: u32;
    match cs {
        Charset::Utf8 => {
            if c < 0x80 {
                this_char = c as u32;
                *pos = p + 1;
            } else if c < 0xC2 {
                fail!(1);
            } else if c < 0xE0 {
                if avail < 2 {
                    fail!(1);
                }
                if !utf8_trail(s[p + 1]) {
                    fail!(if utf8_lead(s[p + 1]) { 1 } else { 2 });
                }
                this_char = ((c as u32 & 0x1F) << 6) | (s[p + 1] as u32 & 0x3F);
                if this_char < 0x80 {
                    fail!(2);
                }
                *pos = p + 2;
            } else if c < 0xF0 {
                if avail < 3 || !utf8_trail(s[p + 1]) || !utf8_trail(s[p + 2]) {
                    if avail < 2 || utf8_lead(s[p + 1]) {
                        fail!(1);
                    } else if avail < 3 || utf8_lead(s[p + 2]) {
                        fail!(2);
                    } else {
                        fail!(3);
                    }
                }
                this_char = ((c as u32 & 0x0F) << 12)
                    | ((s[p + 1] as u32 & 0x3F) << 6)
                    | (s[p + 2] as u32 & 0x3F);
                if this_char < 0x800 || (0xD800..=0xDFFF).contains(&this_char) {
                    fail!(3);
                }
                *pos = p + 3;
            } else if c < 0xF5 {
                if avail < 4
                    || !utf8_trail(s[p + 1])
                    || !utf8_trail(s[p + 2])
                    || !utf8_trail(s[p + 3])
                {
                    if avail < 2 || utf8_lead(s[p + 1]) {
                        fail!(1);
                    } else if avail < 3 || utf8_lead(s[p + 2]) {
                        fail!(2);
                    } else if avail < 4 || utf8_lead(s[p + 3]) {
                        fail!(3);
                    } else {
                        fail!(4);
                    }
                }
                this_char = ((c as u32 & 0x07) << 18)
                    | ((s[p + 1] as u32 & 0x3F) << 12)
                    | ((s[p + 2] as u32 & 0x3F) << 6)
                    | (s[p + 3] as u32 & 0x3F);
                if !(0x10000..=0x10FFFF).contains(&this_char) {
                    fail!(4);
                }
                *pos = p + 4;
            } else {
                fail!(1);
            }
        }
        Charset::Big5 | Charset::Big5HkScs => {
            if (0x81..=0xFE).contains(&c) {
                if avail < 2 {
                    fail!(1);
                }
                let next = s[p + 1];
                if (0x40..=0x7E).contains(&next) || (0xA1..=0xFE).contains(&next) {
                    this_char = ((c as u32) << 8) | next as u32;
                } else if cs == Charset::Big5 || (next != 0x80 && next != 0xFF) {
                    fail!(1);
                } else {
                    fail!(2);
                }
                *pos = p + 2;
            } else {
                this_char = c as u32;
                *pos = p + 1;
            }
        }
        Charset::Gb2312 => {
            let lead = |c: u8| c != 0x8E && c != 0x8F && c != 0xA0 && c != 0xFF;
            let trail = |c: u8| (0xA1..=0xFE).contains(&c);
            if (0xA1..=0xFE).contains(&c) {
                if avail < 2 {
                    fail!(1);
                }
                let next = s[p + 1];
                if trail(next) {
                    this_char = ((c as u32) << 8) | next as u32;
                } else if lead(next) {
                    fail!(1);
                } else {
                    fail!(2);
                }
                *pos = p + 2;
            } else if lead(c) {
                this_char = c as u32;
                *pos = p + 1;
            } else {
                fail!(1);
            }
        }
        Charset::Sjis => {
            let lead = |c: u8| c != 0x80 && c != 0xA0 && c < 0xFD;
            let trail = |c: u8| c >= 0x40 && c != 0x7F && c < 0xFD;
            if (0x81..=0x9F).contains(&c) || (0xE0..=0xFC).contains(&c) {
                if avail < 2 {
                    fail!(1);
                }
                let next = s[p + 1];
                if trail(next) {
                    this_char = ((c as u32) << 8) | next as u32;
                } else if lead(next) {
                    fail!(1);
                } else {
                    fail!(2);
                }
                *pos = p + 2;
            } else if c < 0x80 || (0xA1..=0xDF).contains(&c) {
                this_char = c as u32;
                *pos = p + 1;
            } else {
                fail!(1);
            }
        }
        Charset::EucJp => {
            if (0xA1..=0xFE).contains(&c) || c == 0x8E {
                if avail < 2 {
                    fail!(1);
                }
                let next = s[p + 1];
                let ok = if c == 0x8E {
                    (0xA1..=0xDF).contains(&next)
                } else {
                    (0xA1..=0xFE).contains(&next)
                };
                if ok {
                    this_char = ((c as u32) << 8) | next as u32;
                } else {
                    fail!(if next != 0xA0 && next != 0xFF { 1 } else { 2 });
                }
                *pos = p + 2;
            } else if c == 0x8F {
                let hi = |b: u8| (0xA1..=0xFE).contains(&b);
                if avail < 3 || !hi(s[p + 1]) || !hi(s[p + 2]) {
                    if avail < 2 || (s[p + 1] != 0xA0 && s[p + 1] != 0xFF) {
                        fail!(1);
                    } else if avail < 3 || (s[p + 2] != 0xA0 && s[p + 2] != 0xFF) {
                        fail!(2);
                    } else {
                        fail!(3);
                    }
                }
                this_char = ((c as u32) << 16) | ((s[p + 1] as u32) << 8) | s[p + 2] as u32;
                *pos = p + 3;
            } else if c != 0xA0 && c != 0xFF {
                this_char = c as u32;
                *pos = p + 1;
            } else {
                fail!(1);
            }
        }
        // single-byte charsets
        _ => {
            this_char = c as u32;
            *pos = p + 1;
        }
    }
    Some(this_char)
}

/// map_from_unicode(): the code unit for Unicode `code` in `cs`.
fn map_from_unicode(code: u32, cs: Charset) -> Option<u32> {
    // unimap_bsearch() over the charset's table (byte >= `from`)
    let search = |table: &[u32; 256], from: usize| -> Option<u32> {
        if code > 0xFFFF {
            return None;
        }
        (from..256)
            .find(|&b| table[b] == code && code != 0)
            .map(|b| b as u32)
    };
    match cs {
        Charset::Utf8 => Some(code),
        Charset::Iso88591 => (code <= 0xFF).then_some(code),
        Charset::Iso88595 => {
            if code <= 0xA0 || code == 0xAD {
                Some(code)
            } else if code == 0x2116 {
                Some(0xF0)
            } else if code == 0xA7 {
                Some(0xFD)
            } else if (0x0401..=0x045F).contains(&code) {
                if code == 0x040D || code == 0x0450 || code == 0x045D {
                    None
                } else {
                    Some(code - 0x360)
                }
            } else {
                None
            }
        }
        Charset::Iso885915 => {
            if code < 0xA4 || (code > 0xBE && code <= 0xFF) {
                Some(code)
            } else {
                search(&CHARSET_ISO_8859_15, 0xA4)
            }
        }
        Charset::Cp1252 => {
            if code <= 0x7F || (0xA0..=0xFF).contains(&code) {
                Some(code)
            } else {
                search(&CHARSET_CP1252, 0x80)
            }
        }
        Charset::MacRoman | Charset::Cp1251 | Charset::Koi8r | Charset::Cp866 => {
            if cs == Charset::MacRoman && code == 0x7F {
                None
            } else if code <= 0x7F {
                Some(code)
            } else {
                search(cs.to_uni_table()?, 0x80)
            }
        }
        // Only the ASCII range is known; 0x5C is the Yen sign in SJIS/EUC-JP.
        Charset::Sjis | Charset::EucJp => {
            ((0x20..=0x7D).contains(&code) && code != 0x5C).then_some(code)
        }
        Charset::Big5 | Charset::Big5HkScs | Charset::Gb2312 => {
            (0x20..=0x7D).contains(&code).then_some(code)
        }
    }
}

/// write_octet_sequence()
fn write_octet_sequence(out: &mut Vec<u8>, cs: Charset, code: u32) {
    if cs == Charset::Utf8 {
        append_utf8(out, code);
    } else {
        out.push(code as u8);
    }
}

/// Append a Unicode codepoint to `out` encoded as UTF-8.
fn append_utf8(out: &mut Vec<u8>, cp: u32) {
    if let Some(ch) = char::from_u32(cp) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        out.extend_from_slice(s.as_bytes());
    }
}

/// Look up the entity name for a codepoint in the encode table.
fn lookup_entity_encode(cp: u32, table: &'static [(u32, &'static str)]) -> Option<&'static str> {
    match table.binary_search_by_key(&cp, |&(c, _)| c) {
        Ok(idx) => Some(table[idx].1),
        Err(_) => None,
    }
}

/// Look up an entity name in the decode table. Returns (cp1, cp2) — cp2=0 if single.
fn lookup_entity_decode(name: &[u8], table: &[(&str, u32, u32)]) -> Option<(u32, u32)> {
    let s = std::str::from_utf8(name).ok()?;
    match table.binary_search_by_key(&s, |&(n, _, _)| n) {
        Ok(idx) => Some((table[idx].1, table[idx].2)),
        Err(_) => None,
    }
}

/// Return the encode table appropriate for the given doctype + all-entities flag
/// (determine_entity_table(); XML 1.0 never reaches here with `all`).
fn pick_encode_table(doctype: i64, all_entities: bool) -> &'static [(u32, &'static str)] {
    if !all_entities {
        // htmlspecialchars: only the basic 5 (with or without apos depending on doctype)
        match doctype {
            ENT_HTML_DOC_HTML401 => BASIC_ENTS_ENCODE,
            _ => BASIC_ENTS_APOS_ENCODE,
        }
    } else {
        match doctype {
            ENT_HTML_DOC_HTML5 => ENT_HTML5_ENCODE,
            ENT_HTML_DOC_XML1 => BASIC_ENTS_APOS_ENCODE,
            _ => ENT_HTML4_ENCODE,
        }
    }
}

/// Return the multi-codepoint encoding table for a doctype, when applicable.
/// Only HTML5 has these; returns an empty slice for other doctypes.
fn pick_multi_cp_table(doctype: i64, all_entities: bool) -> &'static [(u32, u32, &'static str)] {
    if !all_entities {
        return &[];
    }
    match doctype {
        ENT_HTML_DOC_HTML5 => ENT_HTML5_MULTI_CP,
        _ => &[],
    }
}

/// unescape_inverse_map(): the named entities recognized when decoding
/// (XHTML uses the HTML 4.01 map plus the `&apos;` special case).
fn unescape_inverse_map(all: bool, doctype: i64) -> &'static [(&'static str, u32, u32)] {
    if all {
        match doctype {
            ENT_HTML_DOC_HTML401 | ENT_HTML_DOC_XHTML => DEC_HTML4,
            ENT_HTML_DOC_HTML5 => DEC_HTML5,
            _ => DEC_BASIC_APOS,
        }
    } else {
        match doctype {
            ENT_HTML_DOC_HTML401 => DEC_BASIC_NOAPOS,
            _ => DEC_BASIC_APOS,
        }
    }
}

/// resolve_named_entity_html() with the XHTML `&apos;` hack.
fn resolve_named_entity(
    name: &[u8],
    inv_map: &[(&str, u32, u32)],
    doctype: i64,
) -> Option<(u32, u32)> {
    lookup_entity_decode(name, inv_map).or_else(|| {
        (doctype == ENT_HTML_DOC_XHTML && name == b"apos").then_some((u32::from(b'\''), 0))
    })
}

static DEC_BASIC_APOS: &[(&str, u32, u32)] = &[
    ("amp", 0x26, 0),
    ("apos", 0x27, 0),
    ("gt", 0x3E, 0),
    ("lt", 0x3C, 0),
    ("quot", 0x22, 0),
];

static DEC_BASIC_NOAPOS: &[(&str, u32, u32)] = &[
    ("amp", 0x26, 0),
    ("gt", 0x3E, 0),
    ("lt", 0x3C, 0),
    ("quot", 0x22, 0),
];

// Basic entities only — for htmlspecialchars.
static BASIC_ENTS_ENCODE: &[(u32, &str)] = &[
    (0x22, "quot"),
    (0x26, "amp"),
    (0x27, "#039"),
    (0x3C, "lt"),
    (0x3E, "gt"),
];

static BASIC_ENTS_APOS_ENCODE: &[(u32, &str)] = &[
    (0x22, "quot"),
    (0x26, "amp"),
    (0x27, "apos"),
    (0x3C, "lt"),
    (0x3E, "gt"),
];

/// process_numeric_entity(): `*p` is just after the `#`; on success the
/// code point, with `*p` on the terminating `;`.
fn process_numeric_entity(s: &[u8], p: &mut usize) -> Option<u32> {
    let hex = matches!(s.get(*p), Some(b'x' | b'X'));
    if hex {
        *p += 1;
    }
    let is_digit = |b: Option<&u8>| match b {
        Some(b) if hex => b.is_ascii_hexdigit(),
        Some(b) => b.is_ascii_digit(),
        None => false,
    };
    if !is_digit(s.get(*p)) {
        return None;
    }
    // ZEND_STRTOL(): base 16 also takes a "0x" prefix; saturates on overflow
    if hex
        && s[*p] == b'0'
        && matches!(s.get(*p + 1), Some(b'x' | b'X'))
        && s.get(*p + 2).is_some_and(|b| b.is_ascii_hexdigit())
    {
        *p += 2;
    }
    let radix = if hex { 16 } else { 10 };
    let mut code: i64 = 0;
    while let Some(d) = s.get(*p).and_then(|&b| (b as char).to_digit(radix)) {
        code = code.saturating_mul(radix as i64).saturating_add(d as i64);
        *p += 1;
    }
    if s.get(*p) != Some(&b';') || code > 0x10FFFF {
        return None;
    }
    Some(code as u32)
}

/// php_escape_html_entities_ex()
fn encode_html(input: &[u8], cs: Charset, flags: i64, double_encode: bool, all: bool) -> Vec<u8> {
    let doctype = flags & ENT_HTML_DOC_TYPE_MASK;
    // LIMIT_ALL
    let all = all && !cs.is_partial() && doctype != ENT_HTML_DOC_XML1;
    let entity_table = pick_encode_table(doctype, all);
    let multi_cp_table = pick_multi_cp_table(doctype, all);
    let to_uni_table = if all && !cs.is_unicode_compat() {
        cs.to_uni_table()
    } else {
        None
    };
    // (only consulted for !double_encode)
    let inv_map = unescape_inverse_map(true, doctype);
    let replacement: &[u8] = if cs == Charset::Utf8 {
        b"\xEF\xBF\xBD"
    } else {
        b"&#xFFFD;"
    };

    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let cursor_before = cursor;
        let Some(mut this_char) = get_next_char(cs, input, &mut cursor) else {
            // invalid MB sequence
            if flags & ENT_HTML_IGNORE_ERRORS != 0 {
                continue;
            } else if flags & ENT_HTML_SUBSTITUTE_ERRORS != 0 {
                out.extend_from_slice(replacement);
                continue;
            }
            return Vec::new();
        };
        let mut mbsequence: &[u8] = &input[cursor_before..cursor];

        if this_char != u32::from(b'&') {
            if (this_char == u32::from(b'\'') && flags & ENT_HTML_QUOTE_SINGLE == 0)
                || (this_char == u32::from(b'"') && flags & ENT_HTML_QUOTE_DOUBLE == 0)
            {
                out.extend_from_slice(mbsequence);
                continue;
            }
            let rep = if all {
                if let Some(table) = to_uni_table {
                    this_char = cs.to_unicode(table, this_char);
                    if this_char == 0xFFFF {
                        // no mapping; pass through
                        out.extend_from_slice(mbsequence);
                        continue;
                    }
                }
                find_entity_for_char(
                    this_char,
                    cs,
                    entity_table,
                    multi_cp_table,
                    input,
                    &mut cursor,
                )
            } else if this_char < 64 {
                lookup_entity_encode(this_char, entity_table)
            } else {
                None
            };
            if let Some(name) = rep {
                out.push(b'&');
                out.extend_from_slice(name.as_bytes());
                out.push(b';');
                continue;
            }
            // No entity: check its validity, else pass it unchanged.
            if flags & ENT_HTML_SUBSTITUTE_DISALLOWED_CHARS != 0 {
                let disallowed = if cs.is_unicode_compat() || to_uni_table.is_some() {
                    !unicode_cp_is_allowed(this_char, doctype)
                } else {
                    // Not a Unicode code point unless in 0x20..0x7D; assume
                    // C0 controls map to themselves.
                    this_char <= 0x7D && !unicode_cp_is_allowed(this_char, doctype)
                };
                if disallowed {
                    mbsequence = replacement;
                }
            }
            out.extend_from_slice(mbsequence);
        } else if double_encode {
            out.extend_from_slice(b"&amp;");
        } else {
            match existing_entity_len(input, cursor, doctype, inv_map, flags) {
                Some(ent_len) => {
                    out.push(b'&');
                    out.extend_from_slice(&input[cursor..cursor + ent_len]);
                    out.push(b';');
                    cursor += ent_len + 1;
                }
                None => out.extend_from_slice(b"&amp;"),
            }
        }
    }
    out
}

/// find_entity_for_char(): the entity for `k`; for the leading code point
/// of HTML5 two-code-point entities, peeks at the next character (advancing
/// `cursor` past it when it completes one).
fn find_entity_for_char(
    k: u32,
    cs: Charset,
    table: &'static [(u32, &'static str)],
    multi: &'static [(u32, u32, &'static str)],
    input: &[u8],
    cursor: &mut usize,
) -> Option<&'static str> {
    if multi.iter().any(|&(c1, _, _)| c1 == k) && *cursor < input.len() {
        let mut next_cursor = *cursor;
        if let Some(next_char) = get_next_char(cs, input, &mut next_cursor) {
            if let Some(&(_, _, name)) = multi
                .iter()
                .find(|&&(c1, c2, _)| c1 == k && c2 == next_char)
            {
                *cursor = next_cursor;
                return Some(name);
            }
        }
    }
    lookup_entity_encode(k, table)
}

/// The !double_encode check of php_escape_html_entities_ex(): `cursor` is
/// just after an `&`; the length of the valid entity there (without the `&`
/// and `;`), if any.
fn existing_entity_len(
    input: &[u8],
    cursor: usize,
    doctype: i64,
    inv_map: &[(&str, u32, u32)],
    flags: i64,
) -> Option<usize> {
    if input.get(cursor) == Some(&b'#') {
        let mut pos = cursor + 1;
        let code_point = process_numeric_entity(input, &mut pos)?;
        if flags & ENT_HTML_SUBSTITUTE_DISALLOWED_CHARS != 0
            && !numeric_entity_is_allowed(code_point, doctype)
        {
            return None;
        }
        return Some(pos - cursor);
    }
    // process_named_entity_html()
    let mut end = cursor;
    while end < input.len() && input[end].is_ascii_alphanumeric() {
        end += 1;
    }
    if input.get(end) != Some(&b';') || end == cursor {
        return None;
    }
    resolve_named_entity(&input[cursor..end], inv_map, doctype)?;
    Some(end - cursor)
}

/// The arguments of htmlspecialchars() / htmlentities(): the string, the
/// flags and double_encode (the charset is determined later, and only for
/// a non-empty string).
fn parse_html_args(
    vm: &mut Vm,
    args: &[Value],
    fn_name: &str,
) -> Result<(Vec<u8>, i64, bool), VmError> {
    // Z_PARAM_STR: arrays and objects without __toString() are rejected.
    let first = args.first().map_or(Value::Null, |v| v.deref());
    let bytes = match &first {
        Value::Array(_) => return Err(html_string_type_error(vm, fn_name, "array")),
        Value::Object(o) => {
            let cls = o.borrow().class_name.to_ascii_lowercase();
            if !vm.class_implements_interface(&cls, b"stringable") {
                let name = goro_core::value::display_class_name(&o.borrow().class_name);
                return Err(html_string_type_error(vm, fn_name, &name));
            }
            let s = vm.value_to_string(&first);
            if vm.current_exception.is_some() {
                return Err(VmError {
                    message: "exception".into(),
                    line: vm.current_line,
                });
            }
            s.as_bytes().to_vec()
        }
        v => v.to_php_string().as_bytes().to_vec(),
    };
    // PHP 8.1+ default: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401
    let flags = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(ENT_HTML_QUOTE_SINGLE | ENT_HTML_QUOTE_DOUBLE | ENT_HTML_SUBSTITUTE_ERRORS);
    let double_encode = match args.get(3) {
        None | Some(Value::Null) => true,
        Some(v) => v.is_truthy(),
    };
    Ok((bytes, flags, double_encode))
}

fn html_string_type_error(vm: &mut Vm, fn_name: &str, given: &str) -> VmError {
    let msg = format!(
        "{}(): Argument #1 ($string) must be of type string, {} given",
        fn_name, given
    );
    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

/// php_html_entities()
fn php_html_entities(
    vm: &mut Vm,
    args: &[Value],
    fn_name: &str,
    all: bool,
) -> Result<Value, VmError> {
    let (bytes, flags, double_encode) = parse_html_args(vm, args, fn_name)?;
    if bytes.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }
    let cs = resolve_charset(vm, args, fn_name, 2);
    if all && cs.is_partial() {
        let line = vm.current_line;
        vm.emit_notice_at(
            "htmlentities(): Only basic entities substitution is supported for \
             multi-byte encodings other than UTF-8; functionality is equivalent \
             to htmlspecialchars",
            line,
        );
    }
    let out = encode_html(&bytes, cs, flags, double_encode, all);
    Ok(Value::String(PhpString::from_vec(out)))
}

fn htmlspecialchars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_html_entities(vm, args, "htmlspecialchars", false)
}

fn htmlentities_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_html_entities(vm, args, "htmlentities", true)
}

/// traverse_for_entities(): replaces the `&...;` references with their
/// expansion in `cs`; references that are not representable there are left
/// as they are.
fn decode_html(input: &[u8], cs: Charset, flags: i64, all: bool) -> Vec<u8> {
    let doctype = flags & ENT_HTML_DOC_TYPE_MASK;
    let inv_map = unescape_inverse_map(all, doctype);
    let end = input.len();

    let mut out = Vec::with_capacity(input.len());
    let mut cur = 0;
    while cur < end {
        let Some(off) = input[cur..].iter().position(|&b| b == b'&') else {
            out.extend_from_slice(&input[cur..]);
            break;
        };
        out.extend_from_slice(&input[cur..cur + off]);
        cur += off;
        // Fewer than 4 bytes left: not enough for an entity.
        if end - cur < 4 {
            out.extend_from_slice(&input[cur..]);
            break;
        }

        let mut code2 = 0;
        // Ok((code, index of the ';')), or Err(()) for an incomplete entity
        let parsed: Result<(u32, usize), ()> = if input[cur + 1] == b'#' {
            let mut p = cur + 2;
            match process_numeric_entity(input, &mut p) {
                None => Err(()),
                Some(code) => {
                    if !all && !matches!(code, 0x22 | 0x26 | 0x27 | 0x3C | 0x3E) {
                        // htmlspecialchars_decode(): only &, <, >, " and '
                        Err(())
                    } else if !unicode_cp_is_allowed(code, doctype)
                        || (doctype == ENT_HTML_DOC_HTML5 && code == 0x0D)
                    {
                        Err(())
                    } else {
                        Ok((code, p))
                    }
                }
            }
        } else {
            let name_start = cur + 1;
            let max_search = (LONGEST_ENTITY_LENGTH + 1).min(end - name_start);
            match input[name_start..name_start + max_search]
                .iter()
                .position(|&b| b == b';')
            {
                None | Some(0) => Err(()),
                Some(name_len) => {
                    let name = &input[name_start..name_start + name_len];
                    match resolve_named_entity(name, inv_map, doctype) {
                        Some((c1, c2)) => {
                            code2 = c2;
                            Ok((c1, name_start + name_len))
                        }
                        None => Err(()),
                    }
                }
            }
        };
        let Ok((mut code, semi)) = parsed else {
            // invalid_incomplete_entity: copy the '&' as a normal character
            out.push(b'&');
            cur += 1;
            continue;
        };

        let quote_blocked = (code == u32::from(b'\'') && flags & ENT_HTML_QUOTE_SINGLE == 0)
            || (code == u32::from(b'"') && flags & ENT_HTML_QUOTE_DOUBLE == 0);
        let mapped = if quote_blocked {
            None
        } else if cs != Charset::Utf8 {
            map_from_unicode(code, cs).filter(|_| code2 == 0)
        } else {
            Some(code)
        };
        match mapped {
            Some(c) => {
                code = c;
                write_octet_sequence(&mut out, cs, code);
                if code2 != 0 {
                    write_octet_sequence(&mut out, cs, code2);
                }
                cur = semi + 1;
            }
            None => {
                // invalid_complete_entity: copy up to (not including) the ';'
                out.extend_from_slice(&input[cur..semi]);
                cur = semi;
            }
        }
    }
    out
}

/// LONGEST_ENTITY_LENGTH: longest entity name, excluding & and ;
const LONGEST_ENTITY_LENGTH: usize = 31;

fn htmlspecialchars_decode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // PHP 8.1+ default: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401
    let flags = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(ENT_HTML_QUOTE_SINGLE | ENT_HTML_QUOTE_DOUBLE | ENT_HTML_SUBSTITUTE_ERRORS);
    let _ = vm;
    if !s.as_bytes().contains(&b'&') {
        return Ok(Value::String(s));
    }
    // (the charset does not matter: ISO-8859-1)
    let out = decode_html(s.as_bytes(), Charset::Iso88591, flags, false);
    Ok(Value::String(PhpString::from_vec(out)))
}

fn html_entity_decode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // PHP 8.1+ default: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401
    let flags = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(ENT_HTML_QUOTE_SINGLE | ENT_HTML_QUOTE_DOUBLE | ENT_HTML_SUBSTITUTE_ERRORS);
    // php_unescape_html_entities(): nothing to decode without an '&' (the
    // charset is not even looked up).
    if !s.as_bytes().contains(&b'&') {
        return Ok(Value::String(s));
    }
    let cs = resolve_charset(vm, args, "html_entity_decode", 2);
    let out = decode_html(s.as_bytes(), cs, flags, true);
    Ok(Value::String(PhpString::from_vec(out)))
}

fn get_html_translation_table_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // PHP_HTML_SPECIALCHARS = 0, PHP_HTML_ENTITIES = 1
    let table_kind = args.first().map(|v| v.to_long()).unwrap_or(0);
    // PHP 8.1+ default: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401
    let flags = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(ENT_HTML_QUOTE_SINGLE | ENT_HTML_QUOTE_DOUBLE | ENT_HTML_SUBSTITUTE_ERRORS);
    let cs = resolve_charset(vm, args, "get_html_translation_table", 2);

    let doctype = flags & ENT_HTML_DOC_TYPE_MASK;
    let quote_double = flags & ENT_HTML_QUOTE_DOUBLE != 0;
    let quote_single = flags & ENT_HTML_QUOTE_SINGLE != 0;
    // LIMIT_ALL downgrade: partial-support multi-byte charsets (sjis/eucjp/
    // big5/gb2312) and XML 1.0 force basic entities only, mirroring PHP's
    // behaviour in php_escape_html_entities_ex.
    let all = table_kind != 0 && !cs.is_partial() && doctype != ENT_HTML_DOC_XML1;
    let table = pick_encode_table(doctype, all);

    // write_s3row_data(): an entry for the code point's own entity, then one
    // per HTML5 multi-code-point entity it leads (the second code point is
    // mapped to the charset, or written truncated for UTF-8/ISO-8859-1).
    let multi = pick_multi_cp_table(doctype, all);
    let mut result = PhpArray::new();
    let row = |result: &mut PhpArray, key1: Vec<u8>, uni: u32| {
        let put = |result: &mut PhpArray, key: Vec<u8>, name: &str| {
            result.set(
                ArrayKey::String(PhpString::from_vec(key)),
                Value::String(PhpString::from_string(format!("&{};", name))),
            );
        };
        if let Some(name) = lookup_entity_encode(uni, table) {
            put(result, key1.clone(), name);
        }
        let mut seconds: Vec<(u32, &str)> = multi
            .iter()
            .filter(|(c1, _, _)| *c1 == uni)
            .map(|&(_, c2, n)| (c2, n))
            .collect();
        seconds.sort_by_key(|&(c2, _)| c2);
        for (c2, name) in seconds {
            let mut key = key1.clone();
            match cs {
                Charset::Utf8 => append_utf8(&mut key, c2),
                Charset::Iso88591 => key.push(c2 as u8),
                _ => match map_from_unicode(c2, cs) {
                    Some(b) => key.push(b as u8),
                    None => continue,
                },
            }
            put(result, key, name);
        }
    };
    let quote_ok = |c: u32| !(c == 0x22 && !quote_double || c == 0x27 && !quote_single);
    if cs == Charset::Utf8 || cs == Charset::Iso88591 || !all {
        // Code point order (ISO-8859-1 and the basic table: up to U+00FF).
        let mut cps: Vec<u32> = table
            .iter()
            .map(|&(c, _)| c)
            .chain(multi.iter().map(|&(c, _, _)| c))
            .filter(|&c| cs == Charset::Utf8 && all || c <= 0xFF)
            .collect();
        cps.sort_unstable();
        cps.dedup();
        for cp in cps {
            if !quote_ok(cp) {
                continue;
            }
            let mut key = Vec::new();
            if cs == Charset::Utf8 && all {
                append_utf8(&mut key, cp);
            } else {
                key.push(cp as u8);
            }
            row(&mut result, key, cp);
        }
    } else {
        // Other single-byte charsets: their byte order, mapped to Unicode.
        for byte in 0..=0xFFu32 {
            if !quote_ok(byte) {
                continue;
            }
            let Some(table) = cs.to_uni_table() else {
                continue;
            };
            let uni = cs.to_unicode(table, byte);
            if uni != 0xFFFF {
                row(&mut result, vec![byte as u8], uni);
            }
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
