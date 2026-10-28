//! Character encoding determination and decoding for the HTML parser
//! (WHATWG HTML "13.2.3.2 Determining the character encoding", WHATWG
//! Encoding Standard labels and single-byte indexes).
//!
//! Single-byte encodings decode through the Encoding Standard's index
//! tables; multi-byte legacy encodings go through `goro-charset`.

pub use super::encoding_tables::WhatwgEncoding;
use super::encoding_tables::{LABELS, single_byte_table};

/// "Get an encoding" from a label: trims ASCII whitespace and matches the
/// Encoding Standard's labels case-insensitively.
pub fn encoding_for_label(label: &str) -> Option<WhatwgEncoding> {
    let l = label.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' '));
    if l.len() > 32 {
        return None;
    }
    let lower = l.to_ascii_lowercase();
    LABELS
        .binary_search_by(|(k, _)| k.cmp(&lower.as_str()))
        .ok()
        .map(|i| LABELS[i].1)
}

/// BOM sniffing: the encoding and the BOM length.
pub fn bom_sniff(input: &[u8]) -> Option<(WhatwgEncoding, usize)> {
    if input.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some((WhatwgEncoding::Utf8, 3))
    } else if input.starts_with(&[0xFE, 0xFF]) {
        Some((WhatwgEncoding::Utf16be, 2))
    } else if input.starts_with(&[0xFF, 0xFE]) {
        Some((WhatwgEncoding::Utf16le, 2))
    } else {
        None
    }
}

#[inline]
fn is_sp(b: u8) -> bool {
    matches!(b, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

/// "The algorithm for extracting a character encoding from a meta element".
pub fn encoding_from_meta_content(s: &[u8]) -> Option<WhatwgEncoding> {
    let mut i = 0;
    loop {
        // Find "charset" (case-insensitive) at or after i.
        let rel = s[i..]
            .windows(7)
            .position(|w| w.eq_ignore_ascii_case(b"charset"))?;
        i += rel + 7;
        let mut j = i;
        while j < s.len() && is_sp(s[j]) {
            j += 1;
        }
        if j >= s.len() || s[j] != b'=' {
            // Loop: search again from here.
            continue;
        }
        j += 1;
        while j < s.len() && is_sp(s[j]) {
            j += 1;
        }
        if j >= s.len() {
            return None;
        }
        let (start, end) = if s[j] == b'"' || s[j] == b'\'' {
            let q = s[j];
            let st = j + 1;
            let e = s[st..].iter().position(|&c| c == q)? + st;
            (st, e)
        } else {
            let st = j;
            let e = s[st..]
                .iter()
                .position(|&c| is_sp(c) || c == b';')
                .map_or(s.len(), |p| p + st);
            (st, e)
        };
        let label = String::from_utf8_lossy(&s[start..end]);
        return encoding_for_label(&label);
    }
}

/// "Prescan a byte stream to determine its encoding" over at most the first
/// 1024 bytes.
pub fn prescan(input: &[u8]) -> Option<WhatwgEncoding> {
    let b = &input[..input.len().min(1024)];
    let n = b.len();
    if b.starts_with(&[0x3C, 0, 0x3F, 0, 0x78, 0]) {
        return Some(WhatwgEncoding::Utf16le);
    }
    if b.starts_with(&[0, 0x3C, 0, 0x3F, 0, 0x78]) {
        return Some(WhatwgEncoding::Utf16be);
    }
    let mut p = 0;
    while p < n {
        if b[p] != b'<' {
            p += 1;
            continue;
        }
        let rest = &b[p..];
        if rest.starts_with(b"<!--") {
            // Find "-->" whose dashes may overlap with the opening ones.
            let mut q = p + 2;
            loop {
                if q + 2 >= n {
                    return None;
                }
                if b[q] == b'-' && b[q + 1] == b'-' && b[q + 2] == b'>' {
                    p = q + 3;
                    break;
                }
                q += 1;
            }
            continue;
        }
        if rest.len() >= 6
            && rest[1..5].eq_ignore_ascii_case(b"meta")
            && (is_sp(rest[5]) || rest[5] == b'/')
        {
            p += 5;
            let mut names: Vec<Vec<u8>> = Vec::new();
            let mut got_pragma = false;
            let mut need_pragma: Option<bool> = None;
            let mut charset: Option<Option<WhatwgEncoding>> = None;
            while let Some((name, value)) = get_attribute(b, &mut p) {
                if names.contains(&name) {
                    continue;
                }
                names.push(name.clone());
                match name.as_slice() {
                    b"http-equiv" => {
                        if value == b"content-type" {
                            got_pragma = true;
                        }
                    }
                    b"content" => {
                        if let Some(e) = encoding_from_meta_content(&value)
                            && charset.is_none()
                        {
                            charset = Some(Some(e));
                            need_pragma = Some(true);
                        }
                    }
                    b"charset" => {
                        charset = Some(encoding_for_label(&String::from_utf8_lossy(&value)));
                        need_pragma = Some(false);
                    }
                    _ => {}
                }
                if p >= n {
                    return None;
                }
            }
            if p >= n {
                return None;
            }
            match need_pragma {
                None => {}
                Some(true) if !got_pragma => {}
                _ => {
                    if let Some(Some(mut e)) = charset {
                        if matches!(e, WhatwgEncoding::Utf16be | WhatwgEncoding::Utf16le) {
                            e = WhatwgEncoding::Utf8;
                        }
                        if e == WhatwgEncoding::XUserDefined {
                            e = WhatwgEncoding::Windows1252;
                        }
                        return Some(e);
                    }
                }
            }
            p += 1;
            continue;
        }
        let tag_start = if rest.len() >= 2 && rest[1] == b'/' {
            2
        } else {
            1
        };
        if rest.len() > tag_start && rest[tag_start].is_ascii_alphabetic() {
            p += tag_start;
            while p < n && !is_sp(b[p]) && b[p] != b'>' {
                p += 1;
            }
            while get_attribute(b, &mut p).is_some() {}
            if p >= n {
                return None;
            }
            p += 1;
            continue;
        }
        if rest.starts_with(b"<!") || rest.starts_with(b"</") || rest.starts_with(b"<?") {
            let off = b[p + 1..].iter().position(|&c| c == b'>')?;
            p = p + 1 + off + 1;
            continue;
        }
        p += 1;
    }
    None
}

/// "Get an attribute" of the prescan. Returns None when there is none (or
/// the input ended).
fn get_attribute(b: &[u8], p: &mut usize) -> Option<(Vec<u8>, Vec<u8>)> {
    let n = b.len();
    while *p < n && (is_sp(b[*p]) || b[*p] == b'/') {
        *p += 1;
    }
    if *p >= n || b[*p] == b'>' {
        return None;
    }
    let mut name = Vec::new();
    let mut value = Vec::new();
    loop {
        if *p >= n {
            return None;
        }
        let c = b[*p];
        if c == b'=' && !name.is_empty() {
            *p += 1;
            break;
        }
        if is_sp(c) {
            while *p < n && is_sp(b[*p]) {
                *p += 1;
            }
            if *p >= n || b[*p] != b'=' {
                return Some((name, value));
            }
            *p += 1;
            break;
        }
        if c == b'/' || c == b'>' {
            return Some((name, value));
        }
        name.push(c.to_ascii_lowercase());
        *p += 1;
    }
    while *p < n && is_sp(b[*p]) {
        *p += 1;
    }
    if *p >= n {
        return None;
    }
    let c = b[*p];
    if c == b'"' || c == b'\'' {
        let q = c;
        loop {
            *p += 1;
            if *p >= n {
                return None;
            }
            if b[*p] == q {
                *p += 1;
                return Some((name, value));
            }
            value.push(b[*p].to_ascii_lowercase());
        }
    }
    if c == b'>' {
        return Some((name, value));
    }
    value.push(c.to_ascii_lowercase());
    *p += 1;
    loop {
        if *p >= n {
            return None;
        }
        let c = b[*p];
        if is_sp(c) || c == b'>' {
            return Some((name, value));
        }
        value.push(c.to_ascii_lowercase());
        *p += 1;
    }
}

/// The `goro-charset` encoding used to decode a multi-byte legacy
/// encoding.
fn charset_codec(e: WhatwgEncoding) -> Option<goro_charset::Encoding> {
    use goro_charset::Encoding as E;
    Some(match e {
        WhatwgEncoding::Gbk => E::Cp936,
        WhatwgEncoding::Gb18030 => E::Gb18030,
        WhatwgEncoding::Big5 => E::Cp950,
        WhatwgEncoding::EucJp => E::EucJp,
        WhatwgEncoding::Iso2022Jp => E::Iso2022Jp,
        WhatwgEncoding::ShiftJis => E::Cp932,
        WhatwgEncoding::EucKr => E::Uhc,
        WhatwgEncoding::Utf16be => E::Utf16Be,
        WhatwgEncoding::Utf16le => E::Utf16Le,
        _ => return None,
    })
}

/// Decodes `input` (without BOM) to UTF-8; invalid sequences become
/// U+FFFD.
pub fn decode(input: &[u8], e: WhatwgEncoding) -> String {
    match e {
        WhatwgEncoding::Utf8 => match std::str::from_utf8(input) {
            Ok(s) => s.to_string(),
            Err(_) => String::from_utf8_lossy(input).into_owned(),
        },
        WhatwgEncoding::Replacement => {
            if input.is_empty() {
                String::new()
            } else {
                "\u{FFFD}".to_string()
            }
        }
        WhatwgEncoding::XUserDefined => input
            .iter()
            .map(|&b| {
                if b < 0x80 {
                    b as char
                } else {
                    char::from_u32(0xF780 + b as u32 - 0x80).unwrap()
                }
            })
            .collect(),
        _ => {
            if let Some(t) = single_byte_table(e) {
                let mut s = String::with_capacity(input.len());
                for &b in input {
                    if b < 0x80 {
                        s.push(b as char);
                    } else {
                        let cp = t[(b - 0x80) as usize];
                        s.push(if cp == 0 {
                            '\u{FFFD}'
                        } else {
                            char::from_u32(cp as u32).unwrap_or('\u{FFFD}')
                        });
                    }
                }
                return s;
            }
            let codec = charset_codec(e).expect("multi-byte codec");
            let cps = goro_charset::decode(input, codec);
            cps.into_iter()
                .map(|c| char::from_u32(c).unwrap_or('\u{FFFD}'))
                .collect()
        }
    }
}

/// Encodes UTF-8 text into `e` for output. Characters the encoding cannot
/// represent are written as decimal character references (`&#NNNN;`), as
/// the WHATWG "encode" with the HTML error mode does.
pub fn encode(s: &str, e: WhatwgEncoding) -> Vec<u8> {
    match e {
        WhatwgEncoding::Utf8 | WhatwgEncoding::Replacement => s.as_bytes().to_vec(),
        WhatwgEncoding::Utf16be => s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect(),
        WhatwgEncoding::Utf16le => s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
        _ => {
            if let Some(t) = single_byte_table(e) {
                let mut out = Vec::with_capacity(s.len());
                for c in s.chars() {
                    let cp = c as u32;
                    if cp < 0x80 {
                        out.push(cp as u8);
                    } else if let Some(i) = t.iter().position(|&x| x as u32 == cp && x != 0) {
                        out.push(0x80 + i as u8);
                    } else {
                        out.extend_from_slice(format!("&#{cp};").as_bytes());
                    }
                }
                return out;
            }
            let Some(codec) = charset_codec(e) else {
                return s.as_bytes().to_vec();
            };
            let mut out = Vec::with_capacity(s.len());
            let mut buf = [0u8; 4];
            for c in s.chars() {
                if (c as u32) < 0x80 {
                    out.push(c as u8);
                    continue;
                }
                let r = goro_charset::convert(
                    c.encode_utf8(&mut buf).as_bytes(),
                    codec,
                    goro_charset::Encoding::Utf8,
                    &goro_charset::Subst::default(),
                );
                if r.illegal > 0 {
                    out.extend_from_slice(format!("&#{};", c as u32).as_bytes());
                } else {
                    out.extend_from_slice(&r.bytes);
                }
            }
            out
        }
    }
}

/// The result of the encoding sniffing algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sniffed {
    /// The encoding to decode with.
    pub encoding: WhatwgEncoding,
    /// Bytes of BOM to skip.
    pub bom_len: usize,
}

/// The encoding sniffing algorithm as PHP applies it: BOM, then the
/// caller's override (`$overrideEncoding`), then the transport layer
/// (`Content-Type` charset of `createFromFile` streams), then the prescan,
/// then UTF-8 (PHP's default, observed on PHP 8.5).
pub fn sniff(
    input: &[u8],
    override_encoding: Option<WhatwgEncoding>,
    transport: Option<WhatwgEncoding>,
) -> Sniffed {
    if let Some((e, n)) = bom_sniff(input) {
        return Sniffed {
            encoding: e,
            bom_len: n,
        };
    }
    let encoding = override_encoding
        .or(transport)
        .or_else(|| prescan(input))
        .unwrap_or(WhatwgEncoding::Utf8);
    Sniffed {
        encoding,
        bom_len: 0,
    }
}
