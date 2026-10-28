//! htmlspecialchars(), htmlentities(), their decoders,
//! get_html_translation_table() and strip_tags().
//!
//! Behaviour sources: the PHP manual pages of these functions, the tests
//! ext/standard/tests/strings/html*.phpt, get_html_translation_table*.phpt and
//! strip_tags*.phpt, and observation of PHP 8.5 (the fixture
//! tests/fixtures/html.txt and its driver html.php).

mod charset;
mod strip_tags;
#[rustfmt::skip]
mod tables;

pub use charset::Charset;
pub use strip_tags::{AllowedTags, strip_tags};

use crate::error::Diag;

pub const ENT_HTML_QUOTE_NONE: i64 = 0;
pub const ENT_HTML_QUOTE_SINGLE: i64 = 1;
pub const ENT_HTML_QUOTE_DOUBLE: i64 = 2;
pub const ENT_COMPAT: i64 = 2;
pub const ENT_QUOTES: i64 = 3;
pub const ENT_NOQUOTES: i64 = 0;
pub const ENT_IGNORE: i64 = 4;
pub const ENT_SUBSTITUTE: i64 = 8;
pub const ENT_DISALLOWED: i64 = 128;
pub const ENT_HTML401: i64 = 0;
pub const ENT_XML1: i64 = 16;
pub const ENT_XHTML: i64 = 32;
pub const ENT_HTML5: i64 = 48;
pub const HTML_SPECIALCHARS: i64 = 0;
pub const HTML_ENTITIES: i64 = 1;

/// Default `$flags` of all these functions (PHP manual, since 8.1).
pub const DEFAULT_FLAGS: i64 = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401;

/// Document type selected by the ENT_HTML401/XML1/XHTML/HTML5 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doctype {
    Html401,
    Xml1,
    Xhtml,
    Html5,
}

impl Doctype {
    pub fn from_flags(flags: i64) -> Doctype {
        match flags & 48 {
            0 => Doctype::Html401,
            16 => Doctype::Xml1,
            32 => Doctype::Xhtml,
            _ => Doctype::Html5,
        }
    }

    /// Whether a literal character may appear in a document of this type
    /// (ENT_DISALLOWED replaces the others). Observed on PHP 8.5.
    fn literal_allowed(self, c: u32) -> bool {
        match self {
            Doctype::Html401 => {
                matches!(c, 0x09 | 0x0A | 0x0D | 0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0x10FFFF)
            }
            Doctype::Xml1 | Doctype::Xhtml => {
                matches!(c, 0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
            }
            Doctype::Html5 => {
                matches!(c, 0x09 | 0x0A | 0x0C | 0x0D | 0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0x10FFFF)
                    && !is_noncharacter(c)
            }
        }
    }

    /// Whether a numeric character reference to `c` is valid in this
    /// document type (test htmlentities22.phpt; observed on PHP 8.5).
    fn numeric_allowed(self, c: u32) -> bool {
        match self {
            Doctype::Html401 => c <= 0x10FFFF,
            Doctype::Xml1 | Doctype::Xhtml => {
                matches!(c, 0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
            }
            Doctype::Html5 => {
                matches!(c, 0x09 | 0x0A | 0x0C | 0x20..=0x7E | 0xA0..=0x10FFFF)
                    && !is_noncharacter(c)
            }
        }
    }

    /// Whether html_entity_decode() decodes `&#c;` (observed on PHP 8.5).
    fn numeric_decodes(self, c: u32) -> bool {
        match self {
            Doctype::Html401 => {
                matches!(c, 0x09 | 0x0A | 0x0D | 0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0x10FFFF)
            }
            Doctype::Xml1 | Doctype::Xhtml => {
                matches!(c, 0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
            }
            Doctype::Html5 => {
                matches!(c, 0x09 | 0x0A | 0x0C | 0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0x10FFFF)
                    && !is_noncharacter(c)
            }
        }
    }

    /// The entity for `'`: htmlentities() keeps `&#039;` for XHTML, like
    /// HTML 4.01 (test get_html_translation_table_basic7.phpt).
    fn apos(self, entities: bool) -> &'static [u8] {
        match self {
            Doctype::Html401 => b"&#039;",
            Doctype::Xhtml if entities => b"&#039;",
            _ => b"&apos;",
        }
    }
}

fn is_noncharacter(c: u32) -> bool {
    (0xFDD0..=0xFDEF).contains(&c) || (c & 0xFFFE) == 0xFFFE
}

/// Resolves the `$encoding` argument. `None` or an empty name selects
/// `default_charset` (the ini setting; empty means UTF-8). An unknown name
/// falls back to UTF-8 with a warning (PHP manual; observed on PHP 8.5).
pub fn resolve_charset(
    name: Option<&[u8]>,
    default_charset: &[u8],
    diags: &mut Vec<Diag>,
) -> Charset {
    let cut = |n: &[u8]| -> Vec<u8> { n.iter().take_while(|&&b| b != 0).copied().collect() };
    let mut n = cut(name.unwrap_or(b""));
    if n.is_empty() {
        n = cut(default_charset);
        if n.is_empty() {
            return Charset::Utf8;
        }
    }
    match Charset::from_name(&n) {
        Some(c) => c,
        None => {
            diags.push(Diag::Warning(format!(
                "Charset \"{}\" is not supported, assuming UTF-8",
                String::from_utf8_lossy(&n)
            )));
            Charset::Utf8
        }
    }
}

fn find_html5_encode(a: u32, b: u32) -> Option<&'static str> {
    tables::HTML5_ENCODE
        .binary_search_by(|e| (e.0, e.1).cmp(&(a, b)))
        .ok()
        .map(|i| tables::HTML5_ENCODE[i].2)
}

fn html5_has_pair_prefix(a: u32) -> bool {
    let i = tables::HTML5_ENCODE.partition_point(|e| (e.0, e.1) <= (a, 0));
    i < tables::HTML5_ENCODE.len() && tables::HTML5_ENCODE[i].0 == a
}

fn find_html401_by_cp(c: u32) -> Option<&'static str> {
    use std::sync::OnceLock;
    static IDX: OnceLock<Vec<(u32, &'static str)>> = OnceLock::new();
    let idx = IDX.get_or_init(|| {
        let mut v: Vec<(u32, &'static str)> =
            tables::HTML401.iter().map(|&(n, c)| (c, n)).collect();
        v.sort();
        v
    });
    idx.binary_search_by(|e| e.0.cmp(&c)).ok().map(|i| idx[i].1)
}

/// Looks up a named reference (without `&` and `;`) for a doctype; returns
/// the code point(s).
fn lookup_name(doctype: Doctype, name: &[u8]) -> Option<(u32, u32)> {
    match doctype {
        Doctype::Html5 => tables::HTML5_NAMES
            .binary_search_by(|e| e.0.as_bytes().cmp(name))
            .ok()
            .map(|i| (tables::HTML5_NAMES[i].1, tables::HTML5_NAMES[i].2)),
        Doctype::Xml1 => match name {
            b"amp" => Some((b'&' as u32, 0)),
            b"lt" => Some((b'<' as u32, 0)),
            b"gt" => Some((b'>' as u32, 0)),
            b"quot" => Some((b'"' as u32, 0)),
            b"apos" => Some((b'\'' as u32, 0)),
            _ => None,
        },
        Doctype::Html401 | Doctype::Xhtml => {
            if name == b"apos" {
                return if doctype == Doctype::Xhtml {
                    Some((b'\'' as u32, 0))
                } else {
                    None
                };
            }
            use std::sync::OnceLock;
            static IDX: OnceLock<Vec<(&'static str, u32)>> = OnceLock::new();
            let idx = IDX.get_or_init(|| {
                let mut v = tables::HTML401.to_vec();
                v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
                v
            });
            idx.binary_search_by(|e| e.0.as_bytes().cmp(name))
                .ok()
                .map(|i| (idx[i].1, 0))
        }
    }
}

/// Parses a character reference after `&` at `s[pos..]`: returns the
/// reference kind and the length up to and including `;`.
enum Ref {
    Numeric(u32),
    Named(u32, u32),
}

fn parse_numeric(s: &[u8], pos: usize) -> Option<(u32, usize)> {
    // s[pos] == b'#'
    let mut i = pos + 1;
    let hex = matches!(s.get(i), Some(b'x' | b'X'));
    if hex {
        i += 1;
    }
    let start = i;
    let mut v: u64 = 0;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => (c - b'0') as u64,
            c @ b'a'..=b'f' if hex => (c - b'a' + 10) as u64,
            c @ b'A'..=b'F' if hex => (c - b'A' + 10) as u64,
            _ => break,
        };
        v = v * if hex { 16 } else { 10 } + d;
        if v > 0x10FFFF {
            return None;
        }
        i += 1;
    }
    if i == start || s.get(i) != Some(&b';') {
        return None;
    }
    Some((v as u32, i + 1 - pos))
}

fn parse_ref(doctype: Doctype, s: &[u8], pos: usize) -> Option<(Ref, usize)> {
    if s.get(pos) == Some(&b'#') {
        let (c, len) = parse_numeric(s, pos)?;
        return Some((Ref::Numeric(c), len));
    }
    let mut i = pos;
    while i < s.len() && s[i].is_ascii_alphanumeric() {
        i += 1;
    }
    if i == pos || s.get(i) != Some(&b';') {
        return None;
    }
    let (a, b) = lookup_name(doctype, &s[pos..i])?;
    Some((Ref::Named(a, b), i + 1 - pos))
}

fn push_entity(out: &mut Vec<u8>, name: &str) {
    out.push(b'&');
    out.extend_from_slice(name.as_bytes());
    out.push(b';');
}

fn encode(s: &[u8], flags: i64, cs: Charset, double_encode: bool, all: bool) -> Vec<u8> {
    let doctype = Doctype::from_flags(flags);
    let ignore = flags & ENT_IGNORE != 0;
    let substitute = flags & ENT_SUBSTITUTE != 0;
    let disallowed = flags & ENT_DISALLOWED != 0;
    // htmlentities() only knows the entities of single-byte charsets and UTF-8.
    let all = all && !cs.is_cjk() && doctype != Doctype::Xml1;
    let replacement: &[u8] = if cs == Charset::Utf8 {
        "\u{FFFD}".as_bytes()
    } else {
        b"&#xFFFD;"
    };
    let mut out = Vec::with_capacity(s.len() + s.len() / 8);
    let mut i = 0;
    while i < s.len() {
        let (cp, n) = match cs.decode(s, i) {
            Ok(x) => x,
            Err(consumed) => {
                if ignore {
                } else if substitute {
                    out.extend_from_slice(replacement);
                } else {
                    return Vec::new();
                }
                i += consumed;
                continue;
            }
        };
        let bytes = &s[i..i + n];
        i += n;
        // "<\u{20D2}" and ">\u{20D2}" have their own HTML5 names.
        if all
            && doctype == Doctype::Html5
            && matches!(cp, Some(0x3C | 0x3E))
            && let Ok((Some(0x20D2), n2)) = cs.decode(s, i)
        {
            push_entity(&mut out, if cp == Some(0x3C) { "nvlt" } else { "nvgt" });
            i += n2;
            continue;
        }
        match cp {
            Some(0x26) => {
                if !double_encode
                    && let Some((r, len)) = parse_ref(doctype, s, i)
                    && match r {
                        Ref::Numeric(c) => !disallowed || doctype.numeric_allowed(c),
                        Ref::Named(..) => true,
                    }
                {
                    out.push(b'&');
                    out.extend_from_slice(&s[i..i + len]);
                    i += len;
                    continue;
                }
                out.extend_from_slice(b"&amp;");
                continue;
            }
            Some(0x22) => {
                if flags & ENT_HTML_QUOTE_DOUBLE != 0 {
                    out.extend_from_slice(b"&quot;");
                } else {
                    out.extend_from_slice(bytes);
                }
                continue;
            }
            Some(0x27) => {
                if flags & ENT_HTML_QUOTE_SINGLE != 0 {
                    out.extend_from_slice(doctype.apos(all));
                } else {
                    out.extend_from_slice(bytes);
                }
                continue;
            }
            Some(0x3C) => {
                out.extend_from_slice(b"&lt;");
                continue;
            }
            Some(0x3E) => {
                out.extend_from_slice(b"&gt;");
                continue;
            }
            _ => {}
        }
        if let Some(c) = cp {
            // MacRoman has no mapping for the controls (observed on PHP 8.5).
            if all && !(cs == Charset::MacRoman && (c < 0x20 || c == 0x7F)) {
                let name = match doctype {
                    Doctype::Html5 => {
                        let mut found = None;
                        if html5_has_pair_prefix(c)
                            && let Ok((Some(c2), n2)) = cs.decode(s, i)
                            && let Some(name) = find_html5_encode(c, c2)
                        {
                            i += n2;
                            found = Some(name);
                        }
                        found.or_else(|| find_html5_encode(c, 0))
                    }
                    _ => find_html401_by_cp(c),
                };
                if let Some(name) = name {
                    push_entity(&mut out, name);
                    continue;
                }
            }
            // UTF-8 and ISO-8859-1 bytes are code points; other charsets only
            // get code points through the entity tables (htmlentities()), and
            // without them only the ASCII controls are checked (test
            // htmlentities21.phpt; observed on PHP 8.5).
            let checked = match cs {
                Charset::Utf8 | Charset::Iso8859_1 => true,
                Charset::MacRoman if all => c >= 0x20 && c != 0x7F,
                _ => all || c < 0x7F,
            };
            if disallowed && checked && !doctype.literal_allowed(c) {
                out.extend_from_slice(replacement);
                continue;
            }
        }
        out.extend_from_slice(bytes);
    }
    out
}

/// htmlspecialchars(): `charset` comes from [`resolve_charset`].
pub fn htmlspecialchars(s: &[u8], flags: i64, charset: Charset, double_encode: bool) -> Vec<u8> {
    encode(s, flags, charset, double_encode, false)
}

/// htmlentities(). A CJK multi-byte charset only gets the basic entities and
/// a notice (test htmlentities22.phpt).
pub fn htmlentities(
    s: &[u8],
    flags: i64,
    charset: Charset,
    double_encode: bool,
    diags: &mut Vec<Diag>,
) -> Vec<u8> {
    if charset.is_cjk() {
        diags.push(Diag::Notice(
            "Only basic entities substitution is supported for multi-byte encodings other than UTF-8; \
             functionality is equivalent to htmlspecialchars"
                .into(),
        ));
    }
    encode(s, flags, charset, double_encode, true)
}

fn quote_ok(flags: i64, c: u32) -> bool {
    match c {
        0x22 => flags & ENT_HTML_QUOTE_DOUBLE != 0,
        0x27 => flags & ENT_HTML_QUOTE_SINGLE != 0,
        _ => true,
    }
}

fn decode(s: &[u8], flags: i64, cs: Charset, all: bool) -> Vec<u8> {
    let doctype = Doctype::from_flags(flags);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if b != b'&' {
            out.push(b);
            i += 1;
            continue;
        }
        if let Some((r, len)) = parse_ref(doctype, s, i + 1) {
            let done = match r {
                Ref::Numeric(c) => {
                    let special = matches!(c, 0x22 | 0x26 | 0x27 | 0x3C | 0x3E);
                    // CJK charsets only decode printable ASCII.
                    let cjk_ok = !cs.is_cjk() || cjk_decodable(cs, c);
                    if (all || special)
                        && cjk_ok
                        && quote_ok(flags, c)
                        && doctype.numeric_decodes(c)
                    {
                        cs.encode_char(c, &mut out)
                    } else {
                        false
                    }
                }
                Ref::Named(a, b2) => {
                    let name = &s[i + 1..i + len];
                    // htmlspecialchars_decode() knows the lowercase names only;
                    // CJK charsets decode any name of a printable ASCII
                    // character (observed on PHP 8.5).
                    let special = b2 == 0
                        && if all {
                            (0x20..0x7F).contains(&a)
                        } else {
                            is_basic_name(doctype, name)
                        };
                    // Two-code-point references only decode to UTF-8 (observed on PHP 8.5).
                    let pair_ok = b2 == 0 || cs == Charset::Utf8;
                    // CJK charsets only decode the basic named entities.
                    let all_named = all && !cs.is_cjk();
                    let cjk_ok = !cs.is_cjk() || cjk_decodable(cs, a);
                    if (all_named || special) && pair_ok && cjk_ok && quote_ok(flags, a) {
                        let mark = out.len();
                        if cs.encode_char(a, &mut out) && (b2 == 0 || cs.encode_char(b2, &mut out))
                        {
                            true
                        } else {
                            out.truncate(mark);
                            false
                        }
                    } else {
                        false
                    }
                }
            };
            if done {
                i += 1 + len;
                continue;
            }
        }
        out.push(b'&');
        i += 1;
    }
    out
}

/// What html_entity_decode() decodes in a CJK charset: printable ASCII,
/// except 0x5C and 0x7E in the Japanese charsets (observed on PHP 8.5).
fn cjk_decodable(cs: Charset, c: u32) -> bool {
    (0x20..0x7F).contains(&c)
        && !(matches!(c, 0x5C | 0x7E) && matches!(cs, Charset::ShiftJis | Charset::EucJp))
}

fn is_basic_name(doctype: Doctype, name: &[u8]) -> bool {
    match name {
        b"amp" | b"lt" | b"gt" | b"quot" => true,
        b"apos" => doctype != Doctype::Html401,
        _ => false,
    }
}

/// html_entity_decode().
pub fn html_entity_decode(s: &[u8], flags: i64, charset: Charset) -> Vec<u8> {
    decode(s, flags, charset, true)
}

/// htmlspecialchars_decode(): only the five special characters (the
/// charset does not matter, they are ASCII).
pub fn htmlspecialchars_decode(s: &[u8], flags: i64) -> Vec<u8> {
    decode(s, flags, Charset::Utf8, false)
}

/// get_html_translation_table(): (character, entity) pairs in PHP's order
/// (by code point). `table` is HTML_SPECIALCHARS or anything else for
/// HTML_ENTITIES (observed on PHP 8.5).
pub fn get_html_translation_table(
    table: i64,
    flags: i64,
    charset: Charset,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let doctype = Doctype::from_flags(flags);
    let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let add = |a: u32, b: u32, name: &[u8], rows: &mut Vec<(Vec<u8>, Vec<u8>)>| {
        let mut k = Vec::new();
        if (charset == Charset::MacRoman && (a < 0x20 || a == 0x7F))
            || !charset.encode_char(a, &mut k)
        {
            return;
        }
        if b != 0 && charset == Charset::Iso8859_1 {
            // The second code point is truncated to a byte (observed on PHP 8.5:
            // "<\xD2" => "&nvlt;").
            k.push(b as u8);
        } else if b != 0 && !charset.encode_char(b, &mut k) {
            return;
        }
        let mut v = vec![b'&'];
        v.extend_from_slice(name);
        v.push(b';');
        rows.push((k, v));
    };
    let entities = table != HTML_SPECIALCHARS && !charset.is_cjk() && doctype != Doctype::Xml1;
    if entities && doctype == Doctype::Html5 {
        for &(a, b, name) in tables::HTML5_ENCODE {
            if !quote_ok(flags, a) {
                continue;
            }
            add(a, b, name.as_bytes(), &mut rows);
        }
    } else if entities {
        for &(name, c) in tables::HTML401 {
            if !quote_ok(flags, c) {
                continue;
            }
            add(c, 0, name.as_bytes(), &mut rows);
        }
        if flags & ENT_HTML_QUOTE_SINGLE != 0 {
            let apos = doctype.apos(true);
            add(0x27, 0, &apos[1..apos.len() - 1], &mut rows);
        }
    } else {
        for (c, name) in [
            (0x22, &b"quot"[..]),
            (0x26, b"amp"),
            (0x3C, b"lt"),
            (0x3E, b"gt"),
        ] {
            if quote_ok(flags, c) {
                add(c, 0, name, &mut rows);
            }
        }
        if flags & ENT_HTML_QUOTE_SINGLE != 0 {
            let apos: &[u8] = if doctype == Doctype::Html401 {
                b"#039"
            } else {
                b"apos"
            };
            add(0x27, 0, apos, &mut rows);
        }
    }
    // Sorted by the characters' bytes (observed on PHP 8.5).
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

#[cfg(test)]
mod tests;
