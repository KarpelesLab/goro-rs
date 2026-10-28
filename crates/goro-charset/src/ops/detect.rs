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

/// The demerits of every BMP code point, from [`DEMERITS`].
fn bmp_table() -> &'static [u8; 0x10000] {
    static T: std::sync::OnceLock<Box<[u8; 0x10000]>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([30u8; 0x10000]);
        for &(a, b, v) in DEMERITS.iter() {
            t[a as usize..=b as usize].fill(v);
        }
        t
    })
}

/// The demerits of one decoded code point.
pub fn demerit(cp: u32) -> u64 {
    match cp {
        ERR => 1000,
        0x10000.. => 40,
        _ => bmp_table()[cp as usize] as u64,
    }
}

/// Whether a leading U+FEFF is free in `enc`.
fn free_bom(enc: Encoding) -> bool {
    use Encoding::*;
    matches!(enc, Utf8 | Utf16Be | Utf16Le | Utf32Be | Utf32Le)
}

/// Whether `enc` costs 500 more when the string is not valid in it.
fn checked(enc: Encoding) -> bool {
    use Encoding::*;
    matches!(enc, Utf7 | Utf7Imap | Jis | Iso2022Jp)
}

/// The demerits of `s` in `enc` and whether `s` is valid in it, or `None`
/// once the demerits reach `bound`.
fn score_bounded(s: &[u8], enc: Encoding, bound: u64) -> Option<(u64, bool)> {
    let table = bmp_table();
    let mut d = codec::decoder(enc);
    let mut buf = Vec::with_capacity(CHUNK + 64);
    let mut pos = 0;
    let mut total = 0u64;
    let mut errors = false;
    let mut first = true;
    let skip_bom = free_bom(enc);
    let mut add = |buf: &[u32], total: &mut u64, errors: &mut bool| {
        let mut vals = buf;
        if first && !buf.is_empty() {
            first = false;
            if skip_bom && buf[0] == 0xFEFF {
                vals = &buf[1..];
            }
        }
        for &c in vals {
            *total += match c {
                ERR => {
                    *errors = true;
                    1000
                }
                0x10000.. => 40,
                _ => table[c as usize] as u64,
            };
        }
    };
    while pos < s.len() {
        buf.clear();
        d.decode(s, &mut pos, &mut buf, CHUNK);
        add(&buf, &mut total, &mut errors);
        if total >= bound {
            return None;
        }
    }
    buf.clear();
    d.finish(&mut buf);
    add(&buf, &mut total, &mut errors);
    if total >= bound {
        return None;
    }
    let valid = match enc {
        Encoding::Utf8 => !errors,
        _ => !errors && !d.check_failed(),
    };
    if checked(enc) && !valid {
        total += 500;
    }
    Some((total, valid))
}

/// The total demerits of `s` decoded as `enc`.
pub fn score(s: &[u8], enc: Encoding) -> u64 {
    score_bounded(s, enc, u64::MAX).map_or(u64::MAX, |r| r.0)
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
        // A candidate whose demerits reach the best raised total cannot win.
        let bound = best.map_or(u64::MAX, |b| b.0);
        let Some((t, valid)) = score_bounded(s, e, bound) else {
            continue;
        };
        if strict && !valid {
            continue;
        }
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
