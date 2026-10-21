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
    Big5,        // partial: ASCII only
    Big5HkScs,   // partial: ASCII only
    Gb2312,      // partial: ASCII only
    Sjis,        // partial: ASCII only
    EucJp,       // partial: ASCII only
}

impl Charset {
    fn is_partial(self) -> bool {
        matches!(
            self,
            Charset::Big5 | Charset::Big5HkScs | Charset::Gb2312 | Charset::Sjis | Charset::EucJp
        )
    }

    fn is_single_byte(self) -> bool {
        matches!(
            self,
            Charset::Iso88591
                | Charset::Iso885915
                | Charset::Iso88595
                | Charset::Cp1252
                | Charset::Cp1251
                | Charset::Cp866
                | Charset::Koi8r
                | Charset::MacRoman
        )
    }
}

pub fn register(vm: &mut Vm) {
    vm.register_function(b"htmlspecialchars", htmlspecialchars_fn);
    vm.register_function(b"htmlentities", htmlentities_fn);
    vm.register_function(b"htmlspecialchars_decode", htmlspecialchars_decode_fn);
    vm.register_function(b"html_entity_decode", html_entity_decode_fn);
    vm.register_function(b"get_html_translation_table", get_html_translation_table_fn);
}

/// Resolve PHP's encoding string to our Charset enum.
/// Returns (charset, was_supported). When unsupported, defaults to UTF-8.
fn determine_charset(encoding: &str) -> (Charset, bool) {
    // PHP's determine_charset (html.c) is case-insensitive.
    let n = encoding.to_ascii_lowercase();
    let n = n.as_str();
    match n {
        "" | "utf-8" | "utf8" => (Charset::Utf8, true),
        "iso-8859-1" | "iso8859-1" | "iso_8859-1" | "iso_8859_1" | "latin1" => {
            (Charset::Iso88591, true)
        }
        "iso-8859-5" | "iso8859-5" | "iso_8859-5" | "iso_8859_5" => (Charset::Iso88595, true),
        "iso-8859-15" | "iso8859-15" | "iso_8859-15" | "iso_8859_15" | "latin9" => {
            (Charset::Iso885915, true)
        }
        "cp1252" | "windows-1252" | "1252" => (Charset::Cp1252, true),
        "cp1251" | "windows-1251" | "win-1251" | "1251" => (Charset::Cp1251, true),
        "cp866" | "ibm866" | "866" => (Charset::Cp866, true),
        "koi8-r" | "koi8r" | "koi8-ru" => (Charset::Koi8r, true),
        "macroman" => (Charset::MacRoman, true),
        "big5" | "950" => (Charset::Big5, true),
        "big5-hkscs" => (Charset::Big5HkScs, true),
        "gb2312" | "936" => (Charset::Gb2312, true),
        "shift_jis" | "sjis" | "sjis-win" | "cp932" | "932" => (Charset::Sjis, true),
        "eucjp" | "euc-jp" | "eucjp-win" => (Charset::EucJp, true),
        _ => (Charset::Utf8, false),
    }
}

/// Look up the effective default charset from INI settings, mirroring PHP's
/// get_default_charset(): `internal_encoding` first, then `default_charset`.
/// Returns the raw string when set and non-empty.
fn ini_default_charset(vm: &Vm) -> Option<String> {
    for key in [b"internal_encoding".as_ref(), b"default_charset".as_ref()] {
        if let Some(val) = vm.constants.get(key) {
            let s = val.to_php_string();
            let raw = s.to_string_lossy();
            if !raw.is_empty() {
                return Some(raw);
            }
        }
    }
    None
}

fn resolve_charset(vm: &mut Vm, args: &[Value], fn_name: &str, arg_idx: usize) -> Charset {
    // Determine the raw charset string. Empty / null / missing all fall back
    // to PHP's get_default_charset() (internal_encoding → default_charset).
    let raw_owned: Option<String> = match args.get(arg_idx) {
        None | Some(Value::Null) => ini_default_charset(vm),
        Some(v) => {
            let s = v.to_php_string();
            let r = s.to_string_lossy();
            if r.is_empty() {
                ini_default_charset(vm)
            } else {
                Some(r)
            }
        }
    };
    let raw = match raw_owned {
        Some(r) if !r.is_empty() => r,
        _ => return Charset::Utf8,
    };
    let (cs, supported) = determine_charset(&raw);
    if !supported {
        vm.emit_warning(&format!(
            "{}(): Charset \"{}\" is not supported, assuming UTF-8",
            fn_name, raw
        ));
    }
    cs
}

/// Decode the next character at `bytes[i..]` as a Unicode codepoint.
/// Returns (codepoint, bytes_consumed). On invalid sequences returns
/// (None, 1) — the caller decides whether to skip, substitute, or pass through.
#[allow(clippy::if_same_then_else)] // branches mirror php-src
fn next_codepoint(bytes: &[u8], i: usize, cs: Charset) -> (Option<u32>, usize) {
    if i >= bytes.len() {
        return (None, 0);
    }
    let b = bytes[i];
    match cs {
        Charset::Utf8 => decode_utf8(bytes, i),
        Charset::Iso88591 => (Some(CHARSET_ISO_8859_1[b as usize]), 1),
        Charset::Iso885915 => (Some(CHARSET_ISO_8859_15[b as usize]), 1),
        Charset::Iso88595 => (Some(CHARSET_ISO_8859_5[b as usize]), 1),
        Charset::Cp1252 => {
            let cp = CHARSET_CP1252[b as usize];
            if cp == 0 && b != 0 {
                (None, 1)
            } else {
                (Some(cp), 1)
            }
        }
        Charset::Cp1251 => {
            let cp = CHARSET_CP1251[b as usize];
            if cp == 0 && b != 0 {
                (None, 1)
            } else {
                (Some(cp), 1)
            }
        }
        Charset::Cp866 => {
            let cp = CHARSET_CP866[b as usize];
            if cp == 0 && b != 0 {
                (None, 1)
            } else {
                (Some(cp), 1)
            }
        }
        Charset::Koi8r => {
            let cp = CHARSET_KOI8R[b as usize];
            if cp == 0 && b != 0 {
                (None, 1)
            } else {
                (Some(cp), 1)
            }
        }
        Charset::MacRoman => {
            let cp = CHARSET_MACROMAN[b as usize];
            // PHP rejects 0x7F for MacRoman
            if b == 0x7F {
                (None, 1)
            } else if cp == 0 && b != 0 {
                (None, 1)
            } else {
                (Some(cp), 1)
            }
        }
        Charset::Big5 => {
            // ASCII (or any byte < 0x81): single byte, pass through.
            // Lead 0x81-0xFE: requires trail (0x40-0x7E or 0xA1-0xFE).
            if (0x81..=0xFE).contains(&b) {
                if i + 1 >= bytes.len() {
                    return (None, 1);
                }
                let n = bytes[i + 1];
                if (0x40..=0x7E).contains(&n) || (0xA1..=0xFE).contains(&n) {
                    (Some(((b as u32) << 8) | n as u32), 2)
                } else {
                    (None, 1)
                }
            } else {
                (Some(b as u32), 1)
            }
        }
        Charset::Big5HkScs => {
            if (0x81..=0xFE).contains(&b) {
                if i + 1 >= bytes.len() {
                    return (None, 1);
                }
                let n = bytes[i + 1];
                if (0x40..=0x7E).contains(&n) || (0xA1..=0xFE).contains(&n) {
                    (Some(((b as u32) << 8) | n as u32), 2)
                } else if n != 0x80 && n != 0xFF {
                    (None, 1)
                } else {
                    (None, 2)
                }
            } else {
                (Some(b as u32), 1)
            }
        }
        Charset::Gb2312 => {
            // gb2312_lead = c != 0x8E, 0x8F, 0xA0, 0xFF
            // gb2312_trail = 0xA1..=0xFE
            let lead = |c: u8| c != 0x8E && c != 0x8F && c != 0xA0 && c != 0xFF;
            let trail = |c: u8| (0xA1..=0xFE).contains(&c);
            if (0xA1..=0xFE).contains(&b) {
                if i + 1 >= bytes.len() {
                    return (None, 1);
                }
                let n = bytes[i + 1];
                if trail(n) {
                    (Some(((b as u32) << 8) | n as u32), 2)
                } else if lead(n) {
                    (None, 1)
                } else {
                    (None, 2)
                }
            } else if lead(b) {
                (Some(b as u32), 1)
            } else {
                (None, 1)
            }
        }
        Charset::Sjis => {
            // sjis_lead = c != 0x80 && c != 0xA0 && c < 0xFD
            // sjis_trail = c >= 0x40 && c != 0x7F && c < 0xFD
            let lead = |c: u8| c != 0x80 && c != 0xA0 && c < 0xFD;
            let trail = |c: u8| c >= 0x40 && c != 0x7F && c < 0xFD;
            if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
                if i + 1 >= bytes.len() {
                    return (None, 1);
                }
                let n = bytes[i + 1];
                if trail(n) {
                    (Some(((b as u32) << 8) | n as u32), 2)
                } else if lead(n) {
                    (None, 1)
                } else {
                    (None, 2)
                }
            } else if b < 0x80 || (0xA1..=0xDF).contains(&b) {
                (Some(b as u32), 1)
            } else {
                (None, 1)
            }
        }
        Charset::EucJp => {
            // PHP's "non-consume if potentially valid lead" rule: when failing
            // on a multi-byte EUC-JP sequence, advance just 1 if the next byte
            // could start a new character (i.e. not 0xA0/0xFF). Advance further
            // only when subsequent bytes are 0xA0/0xFF, which can't start anything.
            let avail = bytes.len() - i;
            if (0xA1..=0xFE).contains(&b) {
                if avail < 2 {
                    return (None, 1);
                }
                let n = bytes[i + 1];
                if (0xA1..=0xFE).contains(&n) {
                    (Some(((b as u32) << 8) | n as u32), 2)
                } else {
                    let adv = if n != 0xA0 && n != 0xFF { 1 } else { 2 };
                    (None, adv)
                }
            } else if b == 0x8E {
                if avail < 2 {
                    return (None, 1);
                }
                let n = bytes[i + 1];
                if (0xA1..=0xDF).contains(&n) {
                    (Some(((b as u32) << 8) | n as u32), 2)
                } else {
                    let adv = if n != 0xA0 && n != 0xFF { 1 } else { 2 };
                    (None, adv)
                }
            } else if b == 0x8F {
                if avail < 3
                    || !(bytes[i + 1] >= 0xA1 && bytes[i + 1] <= 0xFE)
                    || !(bytes[i + 2] >= 0xA1 && bytes[i + 2] <= 0xFE)
                {
                    // Determine how many bytes to consume on failure
                    if avail < 2 || (bytes[i + 1] != 0xA0 && bytes[i + 1] != 0xFF) {
                        (None, 1)
                    } else if avail < 3 || (bytes[i + 2] != 0xA0 && bytes[i + 2] != 0xFF) {
                        (None, 2)
                    } else {
                        (None, 3)
                    }
                } else {
                    let n1 = bytes[i + 1];
                    let n2 = bytes[i + 2];
                    (
                        Some(((b as u32) << 16) | ((n1 as u32) << 8) | n2 as u32),
                        3,
                    )
                }
            } else if b != 0xA0 && b != 0xFF {
                (Some(b as u32), 1)
            } else {
                (None, 1)
            }
        }
    }
}

/// Decode one UTF-8 codepoint at `bytes[i..]`. Returns (codepoint, byte_len).
/// On invalid sequences returns (None, consumed) where consumed reflects how
/// many bytes of the broken sequence to skip — Unicode TR #36 "do not consume
/// valid successor bytes": we advance over the lead and any valid continuation
/// bytes, but stop at the first byte that could legitimately start a new
/// character (any byte that's not a continuation byte, or end of string).
fn decode_utf8(bytes: &[u8], i: usize) -> (Option<u32>, usize) {
    if i >= bytes.len() {
        return (None, 0);
    }
    let b0 = bytes[i];
    if b0 < 0x80 {
        return (Some(b0 as u32), 1);
    }
    if b0 < 0xC2 {
        // continuation byte without lead, or overlong (0xC0/0xC1)
        return (None, 1);
    }
    let (need, mut cp): (usize, u32) = if b0 < 0xE0 {
        (1, (b0 & 0x1F) as u32)
    } else if b0 < 0xF0 {
        (2, (b0 & 0x0F) as u32)
    } else if b0 < 0xF5 {
        (3, (b0 & 0x07) as u32)
    } else {
        return (None, 1);
    };
    // Count how many valid continuation bytes follow, up to `need`.
    let mut k = 1;
    while k <= need {
        if i + k >= bytes.len() {
            // End of input mid-sequence: consume what we've validated so far.
            return (None, k);
        }
        let bc = bytes[i + k];
        if bc & 0xC0 != 0x80 {
            // Not a continuation byte. Don't consume it.
            return (None, k);
        }
        cp = (cp << 6) | (bc & 0x3F) as u32;
        k += 1;
    }
    // Validate ranges
    let len = need + 1;
    let min = match len {
        2 => 0x80,
        3 => 0x800,
        4 => 0x10000,
        _ => 0,
    };
    if cp < min {
        return (None, len);
    }
    // Surrogates and out-of-range
    if (0xD800..=0xDFFF).contains(&cp) || cp > 0x10FFFF {
        return (None, len);
    }
    (Some(cp), len)
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

/// Look up a multi-codepoint entity (cp1, cp2) -> entity name.
fn lookup_multi_cp(
    cp1: u32,
    cp2: u32,
    table: &'static [(u32, u32, &'static str)],
) -> Option<&'static str> {
    // Linear scan within the cp1 range — multi-cp table has at most ~80 entries.
    for &(a, b, n) in table {
        if a == cp1 && b == cp2 {
            return Some(n);
        }
        if a > cp1 {
            break;
        }
    }
    None
}

/// Look up an entity name in the decode table. Returns (cp1, cp2) — cp2=0 if single.
fn lookup_entity_decode(name: &[u8], table: &[(&str, u32, u32)]) -> Option<(u32, u32)> {
    let s = std::str::from_utf8(name).ok()?;
    match table.binary_search_by_key(&s, |&(n, _, _)| n) {
        Ok(idx) => Some((table[idx].1, table[idx].2)),
        Err(_) => None,
    }
}

/// Return the encode table appropriate for the given doctype + all-entities flag.
/// Mirrors PHP's determine_entity_table: HTML5 → html5, everything else → html4
/// (with XML 1.0 forcibly downgraded to htmlspecialchars before reaching here).
fn pick_encode_table(doctype: i64, all_entities: bool) -> &'static [(u32, &'static str)] {
    if !all_entities {
        // htmlspecialchars: only the basic 5 (with or without apos depending on doctype)
        match doctype {
            ENT_HTML_DOC_XHTML | ENT_HTML_DOC_XML1 | ENT_HTML_DOC_HTML5 => {
                BASIC_ENTS_APOS_ENCODE
            }
            _ => BASIC_ENTS_ENCODE,
        }
    } else {
        match doctype {
            ENT_HTML_DOC_HTML5 => ENT_HTML5_ENCODE,
            // XML 1.0 has only the 5 basic entities (with apos)
            ENT_HTML_DOC_XML1 => BASIC_ENTS_APOS_ENCODE,
            // HTML 4.01 and XHTML both use the HTML 4.01 entity set for encoding
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

/// Return the decode table appropriate for the given doctype.
fn pick_decode_table(doctype: i64) -> &'static [(&'static str, u32, u32)] {
    match doctype {
        ENT_HTML_DOC_HTML5 => DEC_HTML5,
        ENT_HTML_DOC_XHTML => DEC_XHTML,
        // XML 1.0 has only the 5 basic entities (with apos)
        ENT_HTML_DOC_XML1 => DEC_BASIC_APOS,
        _ => DEC_HTML4,
    }
}

// XML 1.0 decode table: only the 5 basic entities (with apos).
static DEC_BASIC_APOS: &[(&str, u32, u32)] = &[
    ("amp", 0x26, 0),
    ("apos", 0x27, 0),
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

/// Map a single byte in `cs` to its Unicode codepoint. Used by htmlentities
/// on non-unicode-compat single-byte charsets (everything except UTF-8 and
/// ISO-8859-1) to look up entities and apply doctype rules in Unicode space.
/// Returns `None` if the byte is undefined in this charset.
fn map_byte_to_unicode(cp_raw: u32, cs: Charset) -> Option<u32> {
    if cp_raw > 0xFF {
        return Some(cp_raw);
    }
    let table = match cs {
        Charset::Iso885915 => &CHARSET_ISO_8859_15,
        Charset::Iso88595 => &CHARSET_ISO_8859_5,
        Charset::Cp1252 => &CHARSET_CP1252,
        Charset::Cp1251 => &CHARSET_CP1251,
        Charset::Cp866 => &CHARSET_CP866,
        Charset::Koi8r => &CHARSET_KOI8R,
        Charset::MacRoman => &CHARSET_MACROMAN,
        _ => return None,
    };
    let mapped = table[cp_raw as usize];
    if mapped == 0 && cp_raw != 0 {
        None
    } else {
        Some(mapped)
    }
}

/// Map a Unicode codepoint to its single-byte encoding in `cs` (if any).
/// Used by htmlentities to keep characters in their original byte form when
/// re-emitting (the htmlspecialchars path) but more importantly to verify a
/// codepoint is even representable.
fn map_unicode_to_byte(cp: u32, cs: Charset) -> Option<u8> {
    let table = match cs {
        Charset::Iso88591 => &CHARSET_ISO_8859_1,
        Charset::Iso885915 => &CHARSET_ISO_8859_15,
        Charset::Iso88595 => &CHARSET_ISO_8859_5,
        Charset::Cp1252 => &CHARSET_CP1252,
        Charset::Cp1251 => &CHARSET_CP1251,
        Charset::Cp866 => &CHARSET_CP866,
        Charset::Koi8r => &CHARSET_KOI8R,
        Charset::MacRoman => &CHARSET_MACROMAN,
        _ => return None,
    };
    // Linear scan — these are 256-entry tables. cp can match position 0 (NUL),
    // skip entries where the table has 0 unless looking for 0.
    if cp == 0 {
        return Some(0);
    }
    for (b, &mapped) in table.iter().enumerate() {
        if mapped == cp && b != 0 {
            return Some(b as u8);
        }
    }
    None
}

/// Common encoder: walks the input character by character and replaces
/// per the policy. Output is always UTF-8 unless the input charset is single
/// byte AND no entity replacement happens (we emit original bytes).
#[allow(clippy::if_same_then_else)] // branches mirror php-src
fn encode_html(
    input: &[u8],
    cs: Charset,
    flags: i64,
    double_encode: bool,
    all_entities: bool,
) -> Vec<u8> {
    let doctype = flags & ENT_HTML_DOC_TYPE_MASK;
    let quote_double = flags & ENT_HTML_QUOTE_DOUBLE != 0;
    let quote_single = flags & ENT_HTML_QUOTE_SINGLE != 0;
    let ignore_errors = flags & ENT_HTML_IGNORE_ERRORS != 0;
    let substitute_errors = flags & ENT_HTML_SUBSTITUTE_ERRORS != 0;
    let substitute_disallowed = flags & ENT_HTML_SUBSTITUTE_DISALLOWED_CHARS != 0;

    // LIMIT_ALL: htmlentities downgrades to htmlspecialchars for
    // partial-support charsets and for XML 1.0.
    let all_entities = all_entities && !cs.is_partial() && doctype != ENT_HTML_DOC_XML1;

    let encode_table = pick_encode_table(doctype, all_entities);
    let multi_cp_table = pick_multi_cp_table(doctype, all_entities);

    // Replacement sequence for SUBSTITUTE_ERRORS / SUBSTITUTE_DISALLOWED.
    // UTF-8: the encoded U+FFFD bytes. Otherwise: the literal "&#xFFFD;" string.
    let replacement: &[u8] = if cs == Charset::Utf8 {
        b"\xEF\xBF\xBD"
    } else {
        b"&#xFFFD;"
    };

    // PHP sets up `to_uni_table` only when `all && !CHARSET_UNICODE_COMPAT(charset)`.
    // CHARSET_UNICODE_COMPAT is true for UTF-8 and ISO-8859-1 only (both expose
    // codepoint values directly). For htmlentities on other single-byte
    // charsets, the byte must be mapped to Unicode before entity lookup and
    // disallowed-char checks.
    let uses_uni_table = all_entities && !matches!(cs, Charset::Utf8 | Charset::Iso88591);

    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let (cp_opt, consumed) = next_codepoint(input, i, cs);
        let cp_raw = match cp_opt {
            Some(c) => c,
            None => {
                if ignore_errors {
                    // drop the byte
                } else if substitute_errors {
                    out.extend_from_slice(replacement);
                } else {
                    // Default for both htmlspecialchars and htmlentities: invalid -> empty result.
                    return Vec::new();
                }
                i += consumed.max(1);
                continue;
            }
        };

        // Map raw byte to Unicode if needed (htmlentities + non-unicode-compat single-byte).
        let cp = if uses_uni_table {
            map_byte_to_unicode(cp_raw, cs).unwrap_or(cp_raw)
        } else {
            cp_raw
        };

        // Pre-existing entity handling for !double_encode mode. When the cursor
        // sits on `&` followed by a valid entity (named or numeric), PHP's
        // traverse_for_entities passes the WHOLE entity through verbatim and
        // advances past it. Detect that here before anything else so we don't
        // accidentally encode the inner `#` / `;` / digits.
        if cp == 0x26 && !double_encode {
            if let Some(ent_len) =
                existing_entity_len(input, i, cs, doctype, substitute_disallowed)
            {
                out.extend_from_slice(&input[i..i + ent_len]);
                i += ent_len;
                continue;
            }
        }

        // Multi-codepoint lookahead (HTML5 only). Look at the next codepoint
        // and, if (cp, next_cp) maps to an entity, emit it and consume both.
        // Skip this branch for quote characters that the caller wants left raw.
        let try_multi = !multi_cp_table.is_empty()
            && !(cp == 0x22 && !quote_double)
            && !(cp == 0x27 && !quote_single);
        if try_multi {
            let (next_cp_opt, next_consumed) = next_codepoint(input, i + consumed, cs);
            if let Some(next_cp_raw) = next_cp_opt {
                let next_cp = if uses_uni_table {
                    map_byte_to_unicode(next_cp_raw, cs).unwrap_or(next_cp_raw)
                } else {
                    next_cp_raw
                };
                if let Some(name) = lookup_multi_cp(cp, next_cp, multi_cp_table) {
                    out.push(b'&');
                    out.extend_from_slice(name.as_bytes());
                    out.push(b';');
                    i += consumed + next_consumed;
                    continue;
                }
            }
        }

        // Apply quote-style filters: skip entity for unwanted quote chars.
        let entity = if cp == 0x22 && !quote_double {
            None
        } else if cp == 0x27 && !quote_single {
            None
        } else {
            lookup_entity_encode(cp, encode_table)
        };

        // Determine if the codepoint is disallowed by doctype. The rule depends
        // on what value we know:
        //   UTF-8 or ISO-8859-1 (unicode-compat): cp is the real Unicode value.
        //   non-unicode-compat with all_entities: cp was mapped via uni table.
        //   otherwise (partial multi-byte, or htmlspecialchars on non-unicode-compat
        //   single-byte): we only trust values <= 0x7D as Unicode codepoints.
        let unicode_known =
            matches!(cs, Charset::Utf8 | Charset::Iso88591) || uses_uni_table;
        let is_disallowed = substitute_disallowed
            && (unicode_known || cp <= 0x7D)
            && !unicode_cp_is_allowed(cp, doctype);

        if let Some(name) = entity {
            out.push(b'&');
            out.extend_from_slice(name.as_bytes());
            out.push(b';');
        } else if is_disallowed {
            out.extend_from_slice(replacement);
        } else if cs == Charset::Utf8 {
            // Pass through original UTF-8 bytes
            out.extend_from_slice(&input[i..i + consumed]);
        } else if cs.is_single_byte() {
            // Pass through original single byte
            out.push(input[i]);
        } else {
            // Partial-support multi-byte charset (sjis/eucjp/big5/gb2312):
            // pass through the entire consumed byte sequence.
            out.extend_from_slice(&input[i..i + consumed]);
        }
        i += consumed;
    }

    out
}

/// Check whether `input[i..]` begins with `&` followed by a valid entity reference
/// (named or numeric) terminated by `;`. Returns the length INCLUDING the leading
/// `&` and trailing `;` when a valid entity is found, or `None` otherwise.
///
/// When `substitute_disallowed` is set, also rejects entities whose codepoint(s)
/// would be disallowed by the doctype's numeric_entity_is_allowed rules — those
/// entities must be re-encoded as `&amp;` to make the disallowed escape visible.
fn existing_entity_len(
    input: &[u8],
    i: usize,
    _cs: Charset,
    doctype: i64,
    substitute_disallowed: bool,
) -> Option<usize> {
    // Already at `&`. Look ahead.
    let rest = &input[i + 1..];
    if rest.is_empty() {
        return None;
    }
    // Numeric: &#... or &#x...
    if rest[0] == b'#' {
        let mut k = 1;
        let hex = rest.get(1).map(|&b| b == b'x' || b == b'X').unwrap_or(false);
        if hex {
            k = 2;
        }
        let start = k;
        while k < rest.len() && {
            let b = rest[k];
            if hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        } {
            k += 1;
        }
        if k == start || rest.get(k) != Some(&b';') {
            return None;
        }
        // Parse the numeric value and apply PHP's process_numeric_entity rule.
        let digits = std::str::from_utf8(&rest[start..k]).unwrap_or("");
        let code: u64 = if hex {
            u64::from_str_radix(digits, 16).unwrap_or(u64::MAX)
        } else {
            digits.parse::<u64>().unwrap_or(u64::MAX)
        };
        if code > 0x10FFFF {
            return None;
        }
        if substitute_disallowed && !numeric_entity_is_allowed(code as u32, doctype) {
            return None;
        }
        // total length = 1 (`&`) + k (digits region) + 1 (`;`)
        return Some(k + 2);
    }
    // Named: alphanumeric run, terminated by `;`
    let mut k = 0;
    while k < rest.len() && rest[k].is_ascii_alphanumeric() {
        k += 1;
    }
    if k == 0 || rest.get(k) != Some(&b';') {
        return None;
    }
    let name = &rest[..k];
    match lookup_entity_decode(name, pick_decode_table(doctype)) {
        Some((cp1, cp2)) => {
            if substitute_disallowed
                && !numeric_entity_is_allowed(cp1, doctype)
                && (cp2 == 0 || !numeric_entity_is_allowed(cp2, doctype))
            {
                None
            } else {
                Some(k + 2)
            }
        }
        None => None,
    }
}

fn parse_html_args(vm: &mut Vm, args: &[Value], fn_name: &str) -> (Vec<u8>, i64, Charset, bool) {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = s.as_bytes().to_vec();
    // PHP 8.1+ default: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401
    let flags = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(ENT_HTML_QUOTE_SINGLE | ENT_HTML_QUOTE_DOUBLE | ENT_HTML_SUBSTITUTE_ERRORS);
    let cs = resolve_charset(vm, args, fn_name, 2);
    let double_encode = match args.get(3) {
        None | Some(Value::Null) => true,
        Some(v) => v.is_truthy(),
    };
    (bytes, flags, cs, double_encode)
}

fn htmlspecialchars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (bytes, flags, cs, double_encode) = parse_html_args(vm, args, "htmlspecialchars");
    let out = encode_html(&bytes, cs, flags, double_encode, false);
    Ok(Value::String(PhpString::from_vec(out)))
}

fn htmlentities_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (bytes, flags, cs, double_encode) = parse_html_args(vm, args, "htmlentities");
    let doctype = flags & ENT_HTML_DOC_TYPE_MASK;
    // PHP fires this notice once when htmlentities is asked to use the full
    // table but the charset can't support it (partial-support multi-byte) or
    // the doctype is XML 1.0 (only 5 basic entities).
    // PHP fires this notice only for partial-support multi-byte charsets
    // (big5/big5hkscs/gb2312/sjis/eucjp). XML 1.0 is still downgraded to the
    // basic entity set but does not produce the notice.
    if cs.is_partial() {
        let line = vm.current_line;
        vm.emit_notice_at(
            "htmlentities(): Only basic entities substitution is supported for \
             multi-byte encodings other than UTF-8; functionality is equivalent \
             to htmlspecialchars",
            line,
        );
    }
    let _ = doctype;
    let out = encode_html(&bytes, cs, flags, double_encode, true);
    Ok(Value::String(PhpString::from_vec(out)))
}

/// Common decoder. Walks input and replaces `&...;` references with their
/// expansion, encoded back into the input charset. For non-UTF-8 charsets,
/// values that don't fit in the charset are left as the original `&...;` text.
fn decode_html(input: &[u8], cs: Charset, flags: i64, all_entities: bool) -> Vec<u8> {
    let doctype = flags & ENT_HTML_DOC_TYPE_MASK;
    let quote_double = flags & ENT_HTML_QUOTE_DOUBLE != 0;
    let quote_single = flags & ENT_HTML_QUOTE_SINGLE != 0;
    let decode_table = pick_decode_table(doctype);

    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] != b'&' {
            out.push(input[i]);
            i += 1;
            continue;
        }
        // Look for terminating `;` within a reasonable distance.
        // LONGEST_ENTITY_LENGTH = 31 (e.g. &CounterClockwiseContourIntegral;).
        // Allow extra room for numeric entities (&#1234567;).
        let max_len = 32usize;
        let end_search = input.len().min(i + 1 + max_len);
        let semi = (i + 1..end_search).find(|&k| input[k] == b';');
        let semi = match semi {
            Some(s) => s,
            None => {
                out.push(input[i]);
                i += 1;
                continue;
            }
        };
        let body = &input[i + 1..semi];
        if body.is_empty() {
            out.push(input[i]);
            i += 1;
            continue;
        }

        // Numeric entity
        let mut cp_opt: Option<u32> = None;
        if body[0] == b'#' {
            if body.len() < 2 {
                // skip
            } else if body[1] == b'x' || body[1] == b'X' {
                if body.len() > 2 && body[2..].iter().all(|b| b.is_ascii_hexdigit()) {
                    if let Ok(s) = std::str::from_utf8(&body[2..]) {
                        cp_opt = u32::from_str_radix(s, 16).ok();
                    }
                }
            } else if body[1..].iter().all(|b| b.is_ascii_digit()) {
                if let Ok(s) = std::str::from_utf8(&body[1..]) {
                    cp_opt = s.parse::<u32>().ok();
                }
            }
            // Numeric entities are only decoded if the codepoint is allowed in
            // the chosen doctype. HTML5 has the additional quirk that U+000D
            // is allowed literally but not via numeric entity.
            if let Some(c) = cp_opt {
                if !unicode_cp_is_allowed(c, doctype)
                    || (doctype == ENT_HTML_DOC_HTML5 && c == 0x0D)
                {
                    cp_opt = None;
                }
            }
            // For htmlspecialchars_decode (!all_entities), only the basic
            // entities are decoded — that is, codepoints {34, 38, 39, 60, 62}
            // (`"`, `&`, `'`, `<`, `>`). PHP's check is `code > 63 ||
            // stage3_table_be_apos_00000[code].ent == NULL` — applied
            // regardless of doctype; doctype filtering kicks in for the apos
            // case later via quote-style filter.
            if !all_entities {
                if let Some(c) = cp_opt {
                    if !matches!(c, 34 | 38 | 39 | 60 | 62) {
                        cp_opt = None;
                    }
                }
            }
        }

        let mut cp2: u32 = 0;
        if cp_opt.is_none() && all_entities {
            // Named lookup
            if let Some((cp1, cp2x)) = lookup_entity_decode(body, decode_table) {
                cp_opt = Some(cp1);
                cp2 = cp2x;
            }
        } else if cp_opt.is_none() {
            // For htmlspecialchars_decode the recognized named entities are
            // doctype-dependent:
            //   HTML 4.01 : amp, lt, gt, quot (no apos)
            //   XHTML / HTML5 / XML1 : amp, lt, gt, quot, apos
            cp_opt = match body {
                b"amp" => Some(0x26),
                b"lt" => Some(0x3C),
                b"gt" => Some(0x3E),
                b"quot" => Some(0x22),
                b"apos" if doctype != ENT_HTML_DOC_HTML401 => Some(0x27),
                _ => None,
            };
        }

        let cp = match cp_opt {
            Some(c) => c,
            None => {
                out.push(input[i]);
                i += 1;
                continue;
            }
        };

        // Quote-style filter
        if (cp == 0x22 && !quote_double) || (cp == 0x27 && !quote_single) {
            out.push(input[i]);
            i += 1;
            continue;
        }

        // Try to emit. UTF-8 always works; single-byte tries to map back.
        let emit_cp = |out: &mut Vec<u8>, cp: u32, cs: Charset| -> bool {
            if cs == Charset::Utf8 {
                append_utf8(out, cp);
                true
            } else if let Some(b) = map_unicode_to_byte(cp, cs) {
                out.push(b);
                true
            } else {
                false
            }
        };

        let ok = emit_cp(&mut out, cp, cs);
        let ok2 = if cp2 != 0 { emit_cp(&mut out, cp2, cs) } else { true };
        if !ok || !ok2 {
            // Couldn't represent — bail and leave the original `&...;` literal.
            // Roll back any partial emission of cp1.
            if ok && cp2 != 0 && !ok2 {
                // Remove what we appended for cp.
                let _ = out.pop();
            }
            out.push(input[i]);
            i += 1;
            continue;
        }

        i = semi + 1;
    }
    out
}

fn htmlspecialchars_decode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // PHP 8.1+ default: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401
    let flags = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(ENT_HTML_QUOTE_SINGLE | ENT_HTML_QUOTE_DOUBLE | ENT_HTML_SUBSTITUTE_ERRORS);
    let _ = vm;
    // htmlspecialchars_decode doesn't take a charset arg — output is UTF-8
    // pass-through (the basic 5 are ASCII).
    let out = decode_html(s.as_bytes(), Charset::Utf8, flags, false);
    Ok(Value::String(PhpString::from_vec(out)))
}

fn html_entity_decode_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    // PHP 8.1+ default: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401
    let flags = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(ENT_HTML_QUOTE_SINGLE | ENT_HTML_QUOTE_DOUBLE | ENT_HTML_SUBSTITUTE_ERRORS);
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

    let mut result = PhpArray::new();
    for &(cp, name) in table {
        if cp == 0x22 && !quote_double {
            continue;
        }
        if cp == 0x27 && !quote_single {
            continue;
        }
        // Key is the codepoint encoded in the requested charset.
        let key_bytes: Vec<u8> = if cs == Charset::Utf8 {
            if let Some(ch) = char::from_u32(cp) {
                let mut buf = [0u8; 4];
                ch.encode_utf8(&mut buf).as_bytes().to_vec()
            } else {
                continue;
            }
        } else if let Some(b) = map_unicode_to_byte(cp, cs) {
            vec![b]
        } else {
            continue;
        };

        // PHP also skips entities not representable in target charset.
        let val = format!("&{};", name);
        result.set(
            ArrayKey::String(PhpString::from_vec(key_bytes)),
            Value::String(PhpString::from_string(val)),
        );
    }

    // Multi-codepoint entities (HTML5 only). Keyed by the concatenated UTF-8
    // (or charset) encoding of cp1+cp2.
    for &(cp1, cp2, name) in pick_multi_cp_table(doctype, all) {
        let mut key_bytes: Vec<u8> = Vec::new();
        let mut skip = false;
        for cp in [cp1, cp2] {
            if cs == Charset::Utf8 {
                if let Some(ch) = char::from_u32(cp) {
                    let mut buf = [0u8; 4];
                    key_bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                } else {
                    skip = true;
                    break;
                }
            } else if let Some(b) = map_unicode_to_byte(cp, cs) {
                key_bytes.push(b);
            } else {
                skip = true;
                break;
            }
        }
        if skip {
            continue;
        }
        let val = format!("&{};", name);
        result.set(
            ArrayKey::String(PhpString::from_vec(key_bytes)),
            Value::String(PhpString::from_string(val)),
        );
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
