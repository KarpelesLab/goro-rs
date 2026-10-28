//! Document encoding detection (XML 1.0 Appendix F) and decoding to UTF-8
//! through goro-charset.

use goro_charset::{ERR, Encoding};

/// The result of decoding a document to UTF-8.
#[derive(Debug, Default)]
pub struct Decoded {
    /// UTF-8 text with line ends normalized (XML 1.0 section 2.11).
    pub text: Vec<u8>,
    /// Offset in `text` where an encoding error was found (the text is
    /// truncated there).
    pub error_at: Option<usize>,
    /// The encoding the bytes were decoded from, when it is not UTF-8.
    pub from: Option<Encoding>,
    /// A BOM was present.
    pub bom: bool,
    /// Line feeds in `text` that came from a carriage return, as
    /// `offset << 1 | 1` for CR LF and `offset << 1` for a lone CR.
    pub cr: Vec<usize>,
}

/// The encoding signalled by a byte-order mark or the first bytes
/// (Appendix F.1), and the BOM length.
pub fn sniff(input: &[u8]) -> (Option<Encoding>, usize) {
    match input {
        [0xEF, 0xBB, 0xBF, ..] => (Some(Encoding::Utf8), 3),
        [0x00, 0x00, 0xFE, 0xFF, ..] => (Some(Encoding::Utf32Be), 4),
        [0xFF, 0xFE, 0x00, 0x00, ..] => (Some(Encoding::Utf32Le), 4),
        [0xFE, 0xFF, ..] => (Some(Encoding::Utf16Be), 2),
        [0xFF, 0xFE, ..] => (Some(Encoding::Utf16Le), 2),
        [0x00, 0x00, 0x00, 0x3C, ..] => (Some(Encoding::Utf32Be), 0),
        [0x3C, 0x00, 0x00, 0x00, ..] => (Some(Encoding::Utf32Le), 0),
        [0x00, 0x3C, 0x00, 0x3F, ..] => (Some(Encoding::Utf16Be), 0),
        [0x3C, 0x00, 0x3F, 0x00, ..] => (Some(Encoding::Utf16Le), 0),
        _ => (None, 0),
    }
}

/// Finds the `encoding="..."` pseudo-attribute of an XML declaration at the
/// start of ASCII-compatible `input`. Returns the name and its byte offset.
pub fn declared_encoding(input: &[u8]) -> Option<(String, usize)> {
    if !input.starts_with(b"<?xml") || input.len() < 6 || !crate::chars::is_space(input[5]) {
        return None;
    }
    let end = memchr::memmem::find(&input[..input.len().min(1024)], b"?>")?;
    let decl = &input[..end];
    let mut i = 5;
    while i < decl.len() {
        if decl[i..].starts_with(b"encoding") {
            let mut j = i + 8;
            while j < decl.len() && crate::chars::is_space(decl[j]) {
                j += 1;
            }
            if j < decl.len() && decl[j] == b'=' {
                j += 1;
                while j < decl.len() && crate::chars::is_space(decl[j]) {
                    j += 1;
                }
                if j < decl.len() && (decl[j] == b'"' || decl[j] == b'\'') {
                    let q = decl[j];
                    let start = j + 1;
                    let len = decl[start..].iter().position(|&b| b == q)?;
                    return Some((
                        String::from_utf8_lossy(&decl[start..start + len]).into_owned(),
                        start,
                    ));
                }
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Looks up an encoding name as an XML parser would (IANA names and the
/// aliases goro-charset knows). UTF-16 without a byte order means
/// little-endian unless a BOM said otherwise.
pub fn lookup(name: &str) -> Option<Encoding> {
    let n = name.trim();
    let up = n.to_ascii_uppercase();
    Some(match up.as_str() {
        "UTF-8" | "UTF8" => Encoding::Utf8,
        "UTF-16" | "UTF16" => Encoding::Utf16,
        "ISO-10646-UCS-2" | "UCS-2" | "UCS2" => Encoding::Ucs2,
        "ISO-10646-UCS-4" | "UCS-4" | "UCS4" => Encoding::Ucs4,
        "US-ASCII" | "ASCII" => Encoding::Ascii,
        "LATIN1" | "ISO_8859-1" | "ISO-LATIN-1" | "ISO8859-1" => Encoding::Iso8859_1,
        "SHIFT_JIS" => Encoding::Sjis,
        "WINDOWS-1252" | "CP1252" => Encoding::Cp1252,
        "WINDOWS-1251" | "CP1251" => Encoding::Cp1251,
        _ => return Encoding::from_name(n.as_bytes()),
    })
}

/// Normalizes line ends (`\r\n` and `\r` become `\n`) in place.
pub fn normalize_newlines(v: &mut Vec<u8>) {
    normalize_newlines_tracked(v, None);
}

/// [`normalize_newlines`], recording where carriage returns were.
pub fn normalize_newlines_tracked(v: &mut Vec<u8>, mut cr: Option<&mut Vec<usize>>) {
    let Some(first) = memchr::memchr(b'\r', v) else {
        return;
    };
    let mut w = first;
    let mut r = first;
    while r < v.len() {
        let b = v[r];
        if b == b'\r' {
            let crlf = r + 1 < v.len() && v[r + 1] == b'\n';
            if let Some(c) = cr.as_deref_mut() {
                c.push((w << 1) | crlf as usize);
            }
            v[w] = b'\n';
            w += 1;
            r += 1;
            if crlf {
                r += 1;
            }
        } else {
            v[w] = b;
            w += 1;
            r += 1;
        }
    }
    v.truncate(w);
}

/// Decodes `input` (from `enc`, or UTF-8 when `None`) into normalized UTF-8.
pub fn decode(input: &[u8], enc: Option<Encoding>) -> Decoded {
    let mut out = Decoded::default();
    match enc {
        None | Some(Encoding::Utf8) => {
            let (bom, rest) = if input.starts_with(&[0xEF, 0xBB, 0xBF]) {
                (true, &input[3..])
            } else {
                (false, input)
            };
            out.bom = bom;
            match simdutf8::basic::from_utf8(rest) {
                Ok(_) => out.text = rest.to_vec(),
                Err(_) => {
                    let e = std::str::from_utf8(rest).unwrap_err();
                    out.text = rest[..e.valid_up_to()].to_vec();
                    out.error_at = Some(out.text.len());
                }
            }
        }
        Some(Encoding::Iso8859_1) => {
            let mut s = String::with_capacity(input.len() + input.len() / 8);
            for &b in input {
                s.push(b as char);
            }
            out.text = s.into_bytes();
            out.from = enc;
        }
        Some(e) => {
            let (_, bomlen) = sniff(input);
            let body = &input[bomlen.min(input.len())..];
            out.bom = bomlen > 0;
            let e = match e {
                Encoding::Utf16 => {
                    if input.starts_with(&[0xFE, 0xFF]) {
                        Encoding::Utf16Be
                    } else {
                        Encoding::Utf16Le
                    }
                }
                other => other,
            };
            let cps = goro_charset::decode(body, e);
            let mut s = String::with_capacity(cps.len());
            for c in cps {
                if c == ERR {
                    out.error_at = Some(s.len());
                    break;
                }
                match char::from_u32(c) {
                    Some(ch) => s.push(ch),
                    None => {
                        out.error_at = Some(s.len());
                        break;
                    }
                }
            }
            out.text = s.into_bytes();
            out.from = Some(e);
        }
    }
    let mut cr = Vec::new();
    normalize_newlines_tracked(&mut out.text, Some(&mut cr));
    out.cr = cr;
    if out.error_at.is_some() {
        out.error_at = Some(out.text.len());
    }
    out
}
