//! The string operations behind the `mb_*` functions.
//!
//! Functions take byte strings and an [`Encoding`]; the ones that write
//! characters back take the substitution policy for invalid input. Errors
//! PHP raises as `ValueError` come back as [`MbError`], whose message omits
//! the `func(): ` prefix (the wiring layer adds it).

use crate::codec::{self, CHUNK, ERR};
use crate::{Encoding, Subst, convert};

pub mod case;
pub mod detect;
pub mod entity;
pub mod kana;
pub mod mime;
pub mod width;

pub use case::{CaseMode, convert_case, strtolower, strtoupper};

/// An error PHP throws as `ValueError`; the message has no `func(): `
/// prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MbError {
    /// A `ValueError`; the message has no `func(): ` prefix.
    ValueError(String),
    /// A `ValueError` for an encoding the function cannot work with
    /// (`mb_ord() does not support the "UTF-7" encoding`).
    Unsupported(Encoding),
}

impl MbError {
    /// The full `getMessage()` text for function `func` (e.g. "mb_strpos").
    pub fn message(&self, func: &str) -> String {
        match self {
            MbError::ValueError(m) => format!("{func}(): {m}"),
            MbError::Unsupported(e) => {
                format!("{func}() does not support the \"{}\" encoding", e.name())
            }
        }
    }
}

/// Whether `mb_ord`/`mb_chr` refuse the encoding (observed on PHP 8.5).
fn ord_unsupported(enc: Encoding) -> bool {
    use Encoding::*;
    matches!(
        enc,
        Base64
            | Uuencode
            | HtmlEntities
            | QuotedPrintable
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
    )
}

fn value_error(msg: &str) -> MbError {
    MbError::ValueError(msg.to_string())
}

/// The number of characters in `s` (`mb_strlen`).
///
/// For UCS-2, UCS-4 and UTF-32 it is the byte length divided by the unit
/// size, as mbstring computes it (observed on PHP 8.5: a BOM counts, a
/// trailing partial unit does not); UUENCODE counts bytes.
pub fn strlen(s: &[u8], enc: Encoding) -> usize {
    if let Some(w) = enc.fixed_unit_width() {
        return s.len() / w;
    }
    match enc {
        Encoding::EightBit | Encoding::Iso8859_1 | Encoding::Uuencode => s.len(),
        Encoding::Base64 | Encoding::QuotedPrintable => convert::decode(s, enc).len(),
        Encoding::Utf8 if codec::utf8_valid_up_to(s) == s.len() => utf8_count(s),
        _ => {
            let mut d = codec::decoder(enc);
            let mut buf = Vec::with_capacity(CHUNK + 64);
            let mut pos = 0;
            let mut n = 0;
            while pos < s.len() {
                buf.clear();
                d.decode(s, &mut pos, &mut buf, CHUNK);
                n += buf.len();
            }
            buf.clear();
            d.finish(&mut buf);
            n + buf.len()
        }
    }
}

/// The number of characters of valid UTF-8 (non-continuation bytes),
/// eight bytes at a time.
fn utf8_count(s: &[u8]) -> usize {
    let mut n = 0;
    let (chunks, rest) = s.as_chunks::<8>();
    for c in chunks {
        let w = u64::from_le_bytes(*c);
        // A continuation byte is 10xxxxxx: bit 7 set and bit 6 clear.
        let cont = w & !(w << 1) & 0x8080_8080_8080_8080;
        n += 8 - cont.count_ones() as usize;
    }
    n + rest.iter().filter(|&&b| (b as i8) >= -0x40).count()
}

/// The byte offset of character `k` of valid UTF-8 (the length when `k` is
/// the number of characters, `None` past that), counting eight bytes at a
/// time.
fn utf8_byte_index(s: &[u8], k: usize) -> Option<usize> {
    let mut count = 0;
    let mut i = 0;
    let (chunks, _) = s.as_chunks::<8>();
    for c in chunks {
        let w = u64::from_le_bytes(*c);
        let n = 8 - (w & !(w << 1) & 0x8080_8080_8080_8080).count_ones() as usize;
        if count + n > k {
            break;
        }
        count += n;
        i += 8;
    }
    while i < s.len() {
        if (s[i] as i8) >= -0x40 {
            if count == k {
                return Some(i);
            }
            count += 1;
        }
        i += 1;
    }
    (count == k).then_some(s.len())
}

fn valid_utf8(s: &[u8], enc: Encoding) -> bool {
    enc == Encoding::Utf8 && codec::utf8_valid_up_to(s) == s.len()
}

/// Resolves PHP-style `start`/`length` on `n` items: the range taken.
fn clamp_range(n: usize, start: i64, len: Option<i64>) -> (usize, usize) {
    let n = n as i64;
    let mut a = if start < 0 { (n + start).max(0) } else { start };
    if a > n {
        a = n;
    }
    let b = match len {
        None => n,
        Some(l) if l < 0 => (n + l).max(a),
        Some(l) => (a.saturating_add(l)).min(n),
    };
    (a as usize, b.max(a) as usize)
}

/// `mb_substr()`.
pub fn substr(s: &[u8], start: i64, len: Option<i64>, enc: Encoding, sub: &Subst) -> Vec<u8> {
    if enc.is_single_byte() || s.is_ascii() && enc.is_ascii_compatible() {
        let (a, b) = clamp_range(s.len(), start, len);
        return s[a..b].to_vec();
    }
    if valid_utf8(s, enc) {
        let (a, b) = clamp_range(utf8_count(s), start, len);
        let from = utf8_byte_index(s, a).unwrap_or(s.len());
        let to = from + utf8_byte_index(&s[from..], b - a).unwrap_or(s.len() - from);
        return s[from..to].to_vec();
    }
    if let Some(w) = enc.fixed_unit_width() {
        // Byte ranges of whole units; an open end takes a trailing partial
        // unit too (observed on PHP 8.5).
        let n = s.len() / w;
        if start > n as i64 {
            return Vec::new();
        }
        let (a, b) = clamp_range(n, start, len);
        let end = match len {
            None => s.len(),
            Some(l) if l >= 0 => ((a as u64 + l as u64) * w as u64).min(s.len() as u64) as usize,
            Some(_) => b * w,
        };
        let a = (a * w).min(s.len());
        return s[a..end.max(a)].to_vec();
    }
    let cps = convert::decode(s, enc);
    let (a, b) = clamp_range(cps.len(), start, len);
    convert::encode(&cps[a..b], enc, sub).bytes
}

/// The character length given by a lead byte, for the encodings
/// `mb_str_split` splits by lead bytes (as the raw bytes, invalid or not;
/// observed on PHP 8.5).
fn mblen(enc: Encoding) -> Option<fn(u8) -> usize> {
    use Encoding::*;
    Some(match enc {
        Utf8 | Utf8Docomo | Utf8KddiA | Utf8KddiB | Utf8Softbank => |b| match b {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        },
        Sjis | SjisDocomo | SjisKddi | SjisSoftbank | SjisMac | Sjis2004 | Cp932 | SjisWin => {
            |b| match b {
                0x81..=0x9F | 0xE0..=0xFC => 2,
                _ => 1,
            }
        }
        EucJp | EucJpWin | EucJp2004 | Cp51932 => |b| match b {
            0x8E | 0xA1..=0xFE => 2,
            0x8F => 3,
            _ => 1,
        },
        EucCn | EucKr => |b| if (0xA1..=0xFE).contains(&b) { 2 } else { 1 },
        EucTw => |b| match b {
            0x8E => 4,
            0xA1..=0xFE => 2,
            _ => 1,
        },
        Big5 | Cp950 | Cp936 | Uhc => |b| if (0x81..=0xFE).contains(&b) { 2 } else { 1 },
        _ => return None,
    })
}

/// `mb_str_split()`: `split_length` must be at least 1 (checked by the
/// caller: "Argument #2 ($length) must be greater than 0").
pub fn str_split(s: &[u8], split_length: usize, enc: Encoding, sub: &Subst) -> Vec<Vec<u8>> {
    let k = split_length.max(1);
    if enc.is_single_byte() || s.is_ascii() && enc.is_ascii_compatible() {
        return s.chunks(k).map(|c| c.to_vec()).collect();
    }
    if let Some(w) = enc.fixed_unit_width() {
        return s.chunks(k * w).map(|c| c.to_vec()).collect();
    }
    if let Some(len) = mblen(enc) {
        let mut out = Vec::new();
        let mut i = 0;
        while i < s.len() {
            let start = i;
            for _ in 0..k {
                if i >= s.len() {
                    break;
                }
                i = (i + len(s[i])).min(s.len());
            }
            out.push(s[start..i].to_vec());
        }
        return out;
    }
    let cps = convert::decode(s, enc);
    cps.chunks(k)
        .map(|c| convert::encode(c, enc, sub).bytes)
        .collect()
}

/// The code points of `s` in a form searches can compare: invalid input
/// markers become a value no needle contains.
fn search_cps(s: &[u8], enc: Encoding) -> Vec<u32> {
    convert::decode(s, enc)
}

/// Case-folds for the case-insensitive searches (simple folding, observed
/// on PHP 8.5).
fn fold_cps(cps: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(cps.len());
    case::convert_case_cps(cps, CaseMode::FoldSimple, &mut out);
    out
}

fn find_from(h: &[u32], n: &[u32], from: usize) -> Option<usize> {
    if n.is_empty() {
        return (from <= h.len()).then_some(from);
    }
    if n.len() > h.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| h[i..i + n.len()] == *n)
}

fn rfind_range(h: &[u32], n: &[u32], lo: usize, hi: usize) -> Option<usize> {
    // Last i in lo..=hi with h[i..i+n] == n.
    if hi < lo {
        return None;
    }
    (lo..=hi)
        .rev()
        .find(|&i| i + n.len() <= h.len() && h[i..i + n.len()] == *n)
}

const OFFSET_ERR: &str = "Argument #3 ($offset) must be contained in argument #1 ($haystack)";

/// The decoded haystack and needle of a search, folded when insensitive.
fn prepare(hay: &[u8], needle: &[u8], enc: Encoding, fold: bool) -> (Vec<u32>, Vec<u32>) {
    // (Invalid sequences match each other, observed on PHP 8.5.)
    let h = search_cps(hay, enc);
    let n = search_cps(needle, enc);
    if fold {
        (fold_cps(&h), fold_cps(&n))
    } else {
        (h, n)
    }
}

/// Folding can change lengths; maps a position in the folded haystack back
/// to a character index.
fn unfold_index(hay: &[u32], folded_pos: usize) -> usize {
    let mut pos = 0;
    for (i, &c) in hay.iter().enumerate() {
        if pos >= folded_pos {
            return i;
        }
        let mut tmp = Vec::new();
        case::convert_case_cps(&[c], CaseMode::FoldSimple, &mut tmp);
        pos += tmp.len();
    }
    hay.len()
}

fn fold_index(hay: &[u32], char_pos: usize) -> usize {
    let mut tmp = Vec::new();
    case::convert_case_cps(
        &hay[..char_pos.min(hay.len())],
        CaseMode::FoldSimple,
        &mut tmp,
    );
    tmp.len()
}

fn strpos_impl(
    hay: &[u8],
    needle: &[u8],
    offset: i64,
    enc: Encoding,
    fold: bool,
) -> Result<Option<usize>, MbError> {
    if !fold && valid_utf8(hay, enc) && valid_utf8(needle, enc) {
        let off = if offset < 0 {
            offset + utf8_count(hay) as i64
        } else {
            offset
        };
        let from = usize::try_from(off)
            .ok()
            .and_then(|o| utf8_byte_index(hay, o))
            .ok_or_else(|| value_error(OFFSET_ERR))?;
        let found = memchr::memmem::find(&hay[from..], needle);
        return Ok(found.map(|p| off as usize + utf8_count(&hay[from..from + p])));
    }
    if !fold && enc == Encoding::Utf8 {
        // Invalid UTF-8 matches only the same bytes (test mb_strpos.phpt:
        // "\xFF" is not found in "abc\x80\x80").
        let (_, offs) = char_spans(hay, enc);
        let len = offs.len() as i64 - 1;
        let off = if offset < 0 { offset + len } else { offset };
        if off < 0 || off > len {
            return Err(value_error(OFFSET_ERR));
        }
        let from = offs[off as usize];
        let found = memchr::memmem::find(&hay[from..], needle);
        return Ok(found.map(|p| offs.partition_point(|&b| b <= from + p) - 1));
    }
    let raw = search_cps(hay, enc);
    let (h, n) = prepare(hay, needle, enc, fold);
    let len = raw.len() as i64;
    let off = if offset < 0 { offset + len } else { offset };
    if off < 0 || off > len {
        return Err(value_error(OFFSET_ERR));
    }
    let start = if fold {
        fold_index(&raw, off as usize)
    } else {
        off as usize
    };
    Ok(find_from(&h, &n, start).map(|p| if fold { unfold_index(&raw, p) } else { p }))
}

fn strrpos_impl(
    hay: &[u8],
    needle: &[u8],
    offset: i64,
    enc: Encoding,
    fold: bool,
) -> Result<Option<usize>, MbError> {
    let raw = search_cps(hay, enc);
    let (h, n) = prepare(hay, needle, enc, fold);
    let len = raw.len() as i64;
    if offset > len || offset < -len {
        return Err(value_error(OFFSET_ERR));
    }
    // Positions here are in folded units when folding.
    let flen = h.len();
    let (lo, hi) = if offset >= 0 {
        let lo = if fold {
            fold_index(&raw, offset as usize)
        } else {
            offset as usize
        };
        (lo, flen.saturating_sub(n.len()))
    } else {
        let end_char = (len + offset) as usize;
        let mut e = if fold {
            fold_index(&raw, end_char)
        } else {
            end_char
        };
        if (-offset) as usize >= n.len() {
            // The match may start at the cut point.
        } else {
            e = flen.saturating_sub(n.len());
        }
        (0, e)
    };
    if n.len() > flen {
        return Ok(None);
    }
    Ok(rfind_range(&h, &n, lo, hi.min(flen - n.len()))
        .map(|p| if fold { unfold_index(&raw, p) } else { p }))
}

/// `mb_strpos()`: the character index of the first occurrence of `needle`
/// at or after `offset`.
pub fn strpos(
    hay: &[u8],
    needle: &[u8],
    offset: i64,
    enc: Encoding,
) -> Result<Option<usize>, MbError> {
    strpos_impl(hay, needle, offset, enc, false)
}

/// `mb_stripos()`.
pub fn stripos(
    hay: &[u8],
    needle: &[u8],
    offset: i64,
    enc: Encoding,
) -> Result<Option<usize>, MbError> {
    strpos_impl(hay, needle, offset, enc, true)
}

/// `mb_strrpos()`.
pub fn strrpos(
    hay: &[u8],
    needle: &[u8],
    offset: i64,
    enc: Encoding,
) -> Result<Option<usize>, MbError> {
    strrpos_impl(hay, needle, offset, enc, false)
}

/// `mb_strripos()`.
pub fn strripos(
    hay: &[u8],
    needle: &[u8],
    offset: i64,
    enc: Encoding,
) -> Result<Option<usize>, MbError> {
    strrpos_impl(hay, needle, offset, enc, true)
}

/// The part of `hay` before character `pos`, or from it, re-encoded the way
/// `mb_substr` does.
fn split_at(hay: &[u8], pos: usize, before: bool, enc: Encoding, sub: &Subst) -> Vec<u8> {
    if before {
        substr(hay, 0, Some(pos as i64), enc, sub)
    } else {
        substr(hay, pos as i64, None, enc, sub)
    }
}

/// `mb_strstr()`.
pub fn strstr(
    hay: &[u8],
    needle: &[u8],
    before: bool,
    enc: Encoding,
    sub: &Subst,
) -> Option<Vec<u8>> {
    let p = strpos_impl(hay, needle, 0, enc, false).ok()??;
    Some(split_at(hay, p, before, enc, sub))
}

/// `mb_stristr()`.
pub fn stristr(
    hay: &[u8],
    needle: &[u8],
    before: bool,
    enc: Encoding,
    sub: &Subst,
) -> Option<Vec<u8>> {
    let p = strpos_impl(hay, needle, 0, enc, true).ok()??;
    Some(split_at(hay, p, before, enc, sub))
}

/// `mb_strrchr()`.
pub fn strrchr(
    hay: &[u8],
    needle: &[u8],
    before: bool,
    enc: Encoding,
    sub: &Subst,
) -> Option<Vec<u8>> {
    let p = strrpos_impl(hay, needle, 0, enc, false).ok()??;
    Some(split_at(hay, p, before, enc, sub))
}

/// `mb_strrichr()`.
pub fn strrichr(
    hay: &[u8],
    needle: &[u8],
    before: bool,
    enc: Encoding,
    sub: &Subst,
) -> Option<Vec<u8>> {
    let p = strrpos_impl(hay, needle, 0, enc, true).ok()??;
    Some(split_at(hay, p, before, enc, sub))
}

/// `mb_substr_count()`: non-overlapping occurrences.
pub fn substr_count(hay: &[u8], needle: &[u8], enc: Encoding) -> Result<usize, MbError> {
    if needle.is_empty() {
        return Err(value_error("Argument #2 ($needle) must not be empty"));
    }
    let (h, n) = prepare(hay, needle, enc, false);
    // A needle of no characters ("\x1B(B" in ISO-2022-JP) is empty too
    // (test mb_substr_count.phpt).
    if n.is_empty() {
        return Err(value_error("Argument #2 ($needle) must not be empty"));
    }
    let mut count = 0;
    let mut i = 0;
    while let Some(p) = find_from(&h, &n, i) {
        count += 1;
        i = p + n.len();
    }
    Ok(count)
}

/// The display width of `s` (`mb_strwidth`).
pub fn strwidth(s: &[u8], enc: Encoding) -> usize {
    if enc == Encoding::Utf8 && s.is_ascii() {
        return s.len();
    }
    width::cps_width(&convert::decode(s, enc))
}

/// `mb_strimwidth()`. A negative `width` counts back from the width of the
/// rest of the string (deprecated since PHP 8.3; the caller emits that).
pub fn strimwidth(
    s: &[u8],
    start: i64,
    width: i64,
    trim_marker: &[u8],
    enc: Encoding,
    sub: &Subst,
) -> Result<Vec<u8>, MbError> {
    let cps = convert::decode(s, enc);
    // The range check uses mb_strlen's count (observed on PHP 8.5).
    let n = strlen(s, enc) as i64;
    let st = if start < 0 { start + n } else { start };
    if st < 0 || st > n {
        return Err(value_error("Argument #2 ($start) is out of range"));
    }
    let rest = &cps[(st as usize).min(cps.len())..];
    let rest_w = width::cps_width(rest) as i64;
    let w = if width < 0 { rest_w + width } else { width };
    if w < 0 {
        return Err(value_error("Argument #3 ($width) is out of range"));
    }
    if rest_w <= w {
        return Ok(convert::encode(rest, enc, sub).bytes);
    }
    let marker = convert::decode(trim_marker, enc);
    let mw = width::cps_width(&marker) as i64;
    let budget = w - mw;
    let mut used = 0i64;
    let mut k = 0;
    while k < rest.len() {
        let cw = width::cp_width(rest[k]) as i64;
        if used + cw > budget {
            break;
        }
        used += cw;
        k += 1;
    }
    // The trim marker is appended as it is (observed on PHP 8.5: UTF-7
    // closes its Base64 section before it).
    let mut out = convert::encode(&rest[..k], enc, sub).bytes;
    out.extend_from_slice(trim_marker);
    Ok(out)
}

/// `str_pad` pad types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadType {
    /// `STR_PAD_LEFT` (0)
    Left,
    /// `STR_PAD_RIGHT` (1)
    Right,
    /// `STR_PAD_BOTH` (2)
    Both,
}

impl PadType {
    /// The pad type of a `STR_PAD_*` value.
    pub fn from_php(v: i64) -> Result<PadType, MbError> {
        match v {
            0 => Ok(PadType::Left),
            1 => Ok(PadType::Right),
            2 => Ok(PadType::Both),
            _ => Err(value_error(
                "Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH",
            )),
        }
    }
}

/// `mb_str_pad()`.
///
/// `pad_type` is the `STR_PAD_*` value; it and `pad` are only checked when
/// padding is needed (observed on PHP 8.5).
pub fn str_pad(
    s: &[u8],
    length: i64,
    pad: &[u8],
    pad_type: i64,
    enc: Encoding,
    sub: &Subst,
) -> Result<Vec<u8>, MbError> {
    let len = strlen(s, enc) as i64;
    if length <= len {
        return Ok(s.to_vec());
    }
    // A pad string without a whole character counts as empty (test
    // gh20833.phpt: "1" in UTF-32).
    if pad.is_empty() || strlen(pad, enc) == 0 {
        return Err(value_error("Argument #3 ($pad_string) must not be empty"));
    }
    let pad_type = PadType::from_php(pad_type)?;
    let padc = convert::decode(pad, enc);
    let total = (length - len) as usize;
    let (left, right) = match pad_type {
        PadType::Left => (total, 0),
        PadType::Right => (0, total),
        PadType::Both => (total / 2, total - total / 2),
    };
    let fill = |k: usize| -> Vec<u32> { padc.iter().copied().cycle().take(k).collect() };
    // The string itself is copied as it is (observed on PHP 8.5).
    if is_stateful(enc) {
        let mut cps = fill(left);
        cps.extend(convert::decode(s, enc));
        cps.extend(fill(right));
        return Ok(convert::encode(&cps, enc, sub).bytes);
    }
    let mut out = convert::encode(&fill(left), enc, sub).bytes;
    out.extend_from_slice(s);
    out.extend(convert::encode(&fill(right), enc, sub).bytes);
    Ok(out)
}

/// Encodings with shift states, whose byte ranges cannot be cut and pasted.
fn is_stateful(enc: Encoding) -> bool {
    use Encoding::*;
    matches!(
        enc,
        Utf7 | Utf7Imap
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
            | SjisSoftbank
            | Base64
            | QuotedPrintable
            | Uuencode
            | HtmlEntities
    )
}

/// The decoded characters of `s` with the byte offset each starts at (and
/// the end offset last).
fn char_spans(s: &[u8], enc: Encoding) -> (Vec<Vec<u32>>, Vec<usize>) {
    let mut d = codec::decoder(enc);
    let mut chars = Vec::new();
    let mut starts = Vec::new();
    let mut pos = 0;
    while pos < s.len() {
        let at = pos;
        let mut buf = Vec::new();
        d.decode(s, &mut pos, &mut buf, 1);
        if !buf.is_empty() {
            chars.push(buf);
            starts.push(at);
        }
    }
    let mut tail = Vec::new();
    d.finish(&mut tail);
    if !tail.is_empty() {
        chars.push(tail);
        starts.push(s.len());
    }
    starts.push(s.len());
    (chars, starts)
}

/// The characters `mb_trim` strips by default (PHP manual: mb_trim).
pub const TRIM_DEFAULT: &[u32] = &[
    0x20, 0x09, 0x0A, 0x0D, 0x00, 0x0B, 0x0C, 0xA0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004,
    0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0x85,
    0x180E,
];

/// Which ends `mb_trim` strips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMode {
    /// `mb_trim`
    Both,
    /// `mb_ltrim`
    Left,
    /// `mb_rtrim`
    Right,
}

/// `mb_trim()`, `mb_ltrim()`, `mb_rtrim()`. `chars` is the list of
/// characters to strip (in `enc`), or `None` for [`TRIM_DEFAULT`].
pub fn trim(s: &[u8], chars: Option<&[u8]>, mode: TrimMode, enc: Encoding, sub: &Subst) -> Vec<u8> {
    let set: Vec<u32> = match chars {
        Some(c) => convert::decode(c, enc),
        None => TRIM_DEFAULT.to_vec(),
    };
    if is_stateful(enc) {
        let cps = convert::decode(s, enc);
        let mut a = 0;
        let mut b = cps.len();
        if mode != TrimMode::Right {
            while a < b && set.contains(&cps[a]) {
                a += 1;
            }
        }
        if mode != TrimMode::Left {
            while b > a && set.contains(&cps[b - 1]) {
                b -= 1;
            }
        }
        return convert::encode(&cps[a..b], enc, sub).bytes;
    }
    // Nothing to strip: the string as it is; otherwise what mb_substr
    // gives for the kept characters (observed on PHP 8.5).
    let cps = convert::decode(s, enc);
    let mut a = 0;
    let mut b = cps.len();
    if mode != TrimMode::Right {
        while a < b && set.contains(&cps[a]) {
            a += 1;
        }
    }
    if mode != TrimMode::Left {
        while b > a && set.contains(&cps[b - 1]) {
            b -= 1;
        }
    }
    if a == 0 && b == cps.len() {
        return s.to_vec();
    }
    let len = if b == cps.len() {
        None
    } else {
        Some((b - a) as i64)
    };
    substr(s, a as i64, len, enc, sub)
}

/// `mb_ucfirst()`: the first character titlecased.
pub fn ucfirst(s: &[u8], enc: Encoding, sub: &Subst) -> Vec<u8> {
    first_char(s, enc, sub, CaseMode::Title)
}

/// `mb_lcfirst()`: the first character lowercased.
pub fn lcfirst(s: &[u8], enc: Encoding, sub: &Subst) -> Vec<u8> {
    first_char(s, enc, sub, CaseMode::Lower)
}

fn first_char(s: &[u8], enc: Encoding, sub: &Subst, mode: CaseMode) -> Vec<u8> {
    if is_stateful(enc) {
        let cps = convert::decode(s, enc);
        if cps.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(cps.len() + 2);
        case::convert_case_cps(&cps[..1], mode, &mut out);
        out.extend_from_slice(&cps[1..]);
        return convert::encode(&out, enc, sub).bytes;
    }
    // An unchanged first character leaves the string as it is; otherwise
    // the converted character and what mb_substr gives for the rest
    // (observed on PHP 8.5).
    let (chars, _) = char_spans(s, enc);
    let Some(first) = chars.first() else {
        return Vec::new();
    };
    let mut cps = Vec::new();
    case::convert_case_cps(first, mode, &mut cps);
    if cps == *first {
        return s.to_vec();
    }
    let mut out = convert::encode(&cps, enc, sub).bytes;
    out.extend(substr(s, 1, None, enc, sub));
    out
}

/// `mb_ord()`: the code point of the first character, `None` (false) for
/// invalid input. The caller rejects an empty string ("Argument #1
/// ($string) must not be empty").
pub fn ord(s: &[u8], enc: Encoding) -> Result<Option<u32>, MbError> {
    if ord_unsupported(enc) {
        return Err(MbError::Unsupported(enc));
    }
    let cps = convert::decode(s, enc);
    Ok(match cps.first() {
        Some(&c) if c != ERR => Some(c),
        _ => None,
    })
}

/// `mb_chr()`: the character for `cp`, `None` (false) when the encoding
/// lacks it.
pub fn chr(cp: u32, enc: Encoding) -> Result<Option<Vec<u8>>, MbError> {
    if ord_unsupported(enc) {
        return Err(MbError::Unsupported(enc));
    }
    // UTF-8 refuses surrogates; the UTF-16 family writes them (observed on
    // PHP 8.5).
    if cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) && enc == Encoding::Utf8 {
        return Ok(None);
    }
    let c = convert::encode(
        &[cp],
        enc,
        &Subst {
            mode: crate::SubstMode::None,
            ch: 0,
        },
    );
    Ok((c.illegal == 0).then_some(c.bytes))
}

/// `mb_strcut()`: a byte range, moved to character boundaries (start moves
/// back to the start of its character; characters cut by the end are
/// dropped).
pub fn strcut(s: &[u8], start: i64, len: Option<i64>, enc: Encoding) -> Vec<u8> {
    let n = s.len() as i64;
    let start = if start < 0 { (n + start).max(0) } else { start };
    if start >= n {
        return Vec::new();
    }
    // A negative length counts back from the end of the string; the length
    // then applies from the start moved back to a character boundary
    // (observed on PHP 8.5).
    if matches!(enc, Encoding::Utf16 | Encoding::Utf16Be | Encoding::Utf16Le) {
        return strcut_utf16(s, start, len, enc);
    }
    if mblen(enc).is_none()
        && enc.fixed_unit_width().is_none()
        && !enc.is_single_byte()
        && !(enc.is_ascii_compatible() && s.is_ascii())
    {
        return strcut_reencode(s, start, len, enc);
    }
    let len = match len {
        None => n,
        Some(l) if l < 0 => (n - start + l).max(0),
        Some(l) => l,
    };
    let unit = enc.fixed_unit_width();
    let a = if let Some(w) = unit {
        start - start % w as i64
    } else if enc.is_single_byte() || enc.is_ascii_compatible() && s.is_ascii() {
        start
    } else {
        let bounds = char_boundaries(s, enc);
        bounds[bounds.partition_point(|&x| x as i64 <= start) - 1] as i64
    };
    let end = a.saturating_add(len).min(n);
    let b = if let Some(w) = unit {
        end - (end - a) % w as i64
    } else if enc.is_single_byte() || enc.is_ascii_compatible() && s.is_ascii() {
        end
    } else {
        let bounds = char_boundaries(s, enc);
        bounds[bounds.partition_point(|&x| x as i64 <= end) - 1] as i64
    };
    if b <= a {
        return Vec::new();
    }
    s[a as usize..b as usize].to_vec()
}

/// `mb_strcut` for the stateful and other variable-length encodings: from
/// the character holding byte `start`, as many characters as fit in `len`
/// bytes once re-encoded (with their shift sequences) (observed on PHP 8.5).
fn strcut_reencode(s: &[u8], start: i64, len: Option<i64>, enc: Encoding) -> Vec<u8> {
    let n = s.len() as i64;
    let len = match len {
        None => n - start,
        Some(l) if l < 0 => (n - start + l).max(0),
        Some(l) => l.min(n - start),
    } as usize;
    // Per character: its code points and where its bytes start.
    let mut d = codec::decoder(enc);
    let mut chars: Vec<(usize, Vec<u32>)> = Vec::new();
    let mut pos = 0;
    while pos < s.len() {
        let at = pos;
        let mut buf = Vec::new();
        d.decode(s, &mut pos, &mut buf, 1);
        if !buf.is_empty() {
            chars.push((at, buf));
        }
    }
    let mut tail = Vec::new();
    d.finish(&mut tail);
    if !tail.is_empty() {
        chars.push((s.len(), tail));
    }
    // The character whose bytes hold `start`: escapes belong to the
    // character after them.
    let k = chars.partition_point(|c| (c.0 as i64) <= start);
    let k = if k == 0 {
        0
    } else if k < chars.len() && chars[k - 1].0 as i64 <= start {
        // `start` falls after the start of chars[k-1]; it holds `start`
        // unless `start` is in escape bytes before chars[k].
        k - 1
    } else {
        k - 1
    };
    let sub = Subst::default();
    let enc_len = |j: usize| -> Vec<u8> {
        let cps: Vec<u32> = chars[k..j]
            .iter()
            .flat_map(|c| c.1.iter().copied())
            .collect();
        convert::encode(&cps, enc, &sub).bytes
    };
    let mut best = Vec::new();
    for j in k + 1..=chars.len() {
        let e = enc_len(j);
        if e.len() > len {
            break;
        }
        best = e;
    }
    best
}

/// `mb_strcut` for UTF-16: whole units from the unit holding `start`; the
/// length counts from `start`, and a high surrogate cut from its pair at
/// the end is dropped (observed on PHP 8.5).
fn strcut_utf16(s: &[u8], start: i64, len: Option<i64>, enc: Encoding) -> Vec<u8> {
    let n = s.len() as i64;
    let len = match len {
        None => n - start,
        Some(l) if l < 0 => (n - start + l).max(0),
        Some(l) => l.min(n - start),
    };
    let le = enc == Encoding::Utf16Le || enc == Encoding::Utf16 && s.starts_with(b"\xFF\xFE");
    let a = start & !1;
    let mut end = a.saturating_add(len).min(n);
    end -= (end - a) % 2;
    if end - a >= 2 {
        let u = &s[end as usize - 2..end as usize];
        let hi = if le { u[1] } else { u[0] };
        if (0xD8..=0xDB).contains(&hi) {
            end -= 2;
        }
    }
    if end <= a {
        return Vec::new();
    }
    s[a as usize..end as usize].to_vec()
}

/// The byte offsets where characters start, plus the end of the string:
/// by lead bytes for the encodings `mb_str_split` splits that way, from the
/// decoder's progress otherwise.
pub fn char_boundaries(s: &[u8], enc: Encoding) -> Vec<usize> {
    if let Some(len) = mblen(enc) {
        let mut out = Vec::with_capacity(s.len() + 1);
        let mut i = 0;
        while i < s.len() {
            out.push(i);
            i = (i + len(s[i])).min(s.len());
        }
        out.push(s.len());
        return out;
    }
    let mut d = codec::decoder(enc);
    let mut out = vec![0];
    let mut buf = Vec::with_capacity(32);
    let mut pos = 0;
    while pos < s.len() {
        buf.clear();
        d.decode(s, &mut pos, &mut buf, 1);
        if *out.last().unwrap() != pos {
            out.push(pos);
        }
    }
    if *out.last().unwrap() != s.len() {
        out.push(s.len());
    }
    out
}
