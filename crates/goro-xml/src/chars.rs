//! Character classes of XML 1.0 (Fifth Edition) section 2.2 and 2.3.

/// `Char` (XML 1.0 production [2]).
#[inline]
pub fn is_xml_char(c: u32) -> bool {
    matches!(c, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// `NameStartChar` (production [4]).
#[inline]
pub fn is_name_start(c: char) -> bool {
    let c = c as u32;
    matches!(c,
        0x3A | 0x41..=0x5A | 0x5F | 0x61..=0x7A | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF
        | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D | 0x2070..=0x218F | 0x2C00..=0x2FEF
        | 0x3001..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

/// `NameChar` (production [4a]).
#[inline]
pub fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c as u32, 0x2D | 0x2E | 0x30..=0x39 | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
}

/// `S` (production [3]).
#[inline]
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// True if `s` matches `Name`.
pub fn is_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if is_name_start(c) => it.all(is_name_char),
        _ => false,
    }
}

/// True if `s` matches `NCName` (Namespaces in XML 1.0 [4]).
pub fn is_ncname(s: &str) -> bool {
    is_name(s) && !s.contains(':')
}

/// True if `s` matches `QName` (Namespaces in XML 1.0 [7]).
pub fn is_qname(s: &str) -> bool {
    match s.split_once(':') {
        Some((p, l)) => is_ncname(p) && is_ncname(l),
        None => is_ncname(s),
    }
}

/// True if `s` matches `Nmtoken` (production [7]).
pub fn is_nmtoken(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_name_char)
}

/// `PubidChar` (production [13]).
pub fn is_pubid_char(c: char) -> bool {
    matches!(c, ' ' | '\r' | '\n' | 'a'..='z' | 'A'..='Z' | '0'..='9')
        || "-'()+,./:=?;!*#@$_%".contains(c)
}
