//! `mb_detect_encoding()`: picks the most plausible encoding of a list.
//!
//! The scoring model was established by observation on PHP 8.5 (probes in
//! tools/charset/fixtures/mb_detect_weights.php and mb_detect.php):
//!
//! - every decoded code point costs demerits: 1 for the common characters
//!   of living scripts (a fixed list that includes a few thousand frequent
//!   CJK ideographs and Hangul syllables), 6 for ASCII `!"#$%&'()*+,-./`,
//!   30 for other BMP characters (controls included), 40 outside the BMP
//!   and 1000 for an invalid sequence;
//! - a byte order mark at the very start costs nothing in UTF-8 and the
//!   UTF-16/UTF-32 encodings of fixed byte order;
//! - UTF-7, UTF7-IMAP, JIS and ISO-2022-JP cost 500 more when the string is
//!   not valid in them (they have checks beyond decoding errors);
//! - the candidate at position `p` of the `n` listed has its total raised
//!   by `total * 3p / 10n` (rounded down), so earlier candidates are
//!   preferred: the first of two wins unless the other one is better by
//!   more than 15%;
//! - the lowest raised total wins, the earliest on ties.
//!
//! In strict mode, the candidates the string is not valid in are dropped
//! (the positions of the others stay as listed).

use crate::Encoding;
use crate::codec::{self, CHUNK, ERR};
use crate::tables::detect::DEMERITS;

/// The demerits of one decoded code point.
pub fn demerit(cp: u32) -> u64 {
    if cp == ERR {
        return 1000;
    }
    if cp >= 0x10000 {
        return 40;
    }
    let c = cp as u16;
    let i = DEMERITS.partition_point(|e| e.1 < c);
    match DEMERITS.get(i) {
        Some(&(a, _, v)) if a <= c => v as u64,
        _ => 30,
    }
}

/// Whether a leading U+FEFF is free in `enc`.
fn free_bom(enc: Encoding) -> bool {
    use Encoding::*;
    matches!(enc, Utf8 | Utf16Be | Utf16Le | Utf32Be | Utf32Le)
}

/// The total demerits of `s` decoded as `enc`.
pub fn score(s: &[u8], enc: Encoding) -> u64 {
    let mut d = codec::decoder(enc);
    let mut buf = Vec::with_capacity(CHUNK + 64);
    let mut pos = 0;
    let mut total = 0u64;
    let mut first = true;
    let skip_bom = free_bom(enc);
    let mut add = |buf: &[u32], total: &mut u64| {
        let mut it = buf.iter();
        if first && !buf.is_empty() {
            first = false;
            if skip_bom && buf[0] == 0xFEFF {
                it.next();
            }
        }
        *total += it.map(|&c| demerit(c)).sum::<u64>();
    };
    while pos < s.len() {
        buf.clear();
        d.decode(s, &mut pos, &mut buf, CHUNK);
        add(&buf, &mut total);
    }
    buf.clear();
    d.finish(&mut buf);
    add(&buf, &mut total);
    if matches!(
        enc,
        Encoding::Utf7 | Encoding::Utf7Imap | Encoding::Jis | Encoding::Iso2022Jp
    ) && !crate::check_encoding(s, enc)
    {
        total += 500;
    }
    total
}

/// `mb_detect_encoding()`: the encoding of `list` that best fits `s`; `None`
/// when strict detection rejects every one.
pub fn detect_encoding(s: &[u8], list: &[Encoding], strict: bool) -> Option<Encoding> {
    use Encoding::*;
    // The non-text encodings are never candidates (PHP manual, 8.2.0).
    let list: Vec<Encoding> = list
        .iter()
        .copied()
        .filter(|e| {
            !matches!(
                e,
                Base64 | Uuencode | HtmlEntities | QuotedPrintable | SevenBit | EightBit
            )
        })
        .collect();
    let n = list.len() as u64;
    let mut best: Option<(u64, Encoding)> = None;
    for (p, &e) in list.iter().enumerate() {
        if strict && !crate::check_encoding(s, e) {
            continue;
        }
        let t = score(s, e);
        let t = t + t * 3 * p as u64 / (10 * n);
        if best.is_none_or(|(b, _)| t < b) {
            best = Some((t, e));
        }
    }
    best.map(|(_, e)| e)
}

/// `mb_convert_encoding()` with several source encodings: a single one is
/// used as it is; from a list, the non-text encodings are dropped and the
/// source is detected (not strictly) among the rest (observed on PHP 8.5).
pub fn convert_from_list(
    s: &[u8],
    to: Encoding,
    from: &[Encoding],
    sub: &crate::Subst,
) -> Result<crate::Converted, super::MbError> {
    let src = match from {
        [one] => *one,
        _ => detect_encoding(s, from, false).ok_or_else(|| {
            super::MbError::ValueError(
                "Argument #3 ($from_encoding) must specify at least one encoding".into(),
            )
        })?,
    };
    Ok(crate::convert(s, to, src, sub))
}
