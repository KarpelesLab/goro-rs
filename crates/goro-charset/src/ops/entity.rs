//! `mb_encode_numericentity()` and `mb_decode_numericentity()` (PHP
//! manual; tests ext/mbstring/tests/mb_encode_numericentity.phpt and
//! mb_decode_numericentity.phpt).
//!
//! A conversion map is a list of `[start, end, offset, mask]` quads; the
//! caller validates the array (its length must be a multiple of 4).

use crate::{Encoding, Subst, convert};

/// One `[start, end, offset, mask]` entry of a conversion map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapEntry {
    /// First code point.
    pub start: u32,
    /// Last code point.
    pub end: u32,
    /// Added to the code point when encoding, subtracted when decoding.
    pub offset: u32,
    /// Applied to the result.
    pub mask: u32,
}

impl MapEntry {
    /// An entry from PHP integers (truncated to 32 bits, as mbstring does).
    pub fn from_php(start: i64, end: i64, offset: i64, mask: i64) -> MapEntry {
        MapEntry {
            start: start as u32,
            end: end as u32,
            offset: offset as u32,
            mask: mask as u32,
        }
    }
}

/// Encodes the code points the map covers as `&#N;` (or `&#xN;`).
pub fn encode_numericentity(
    s: &[u8],
    map: &[MapEntry],
    enc: Encoding,
    hex: bool,
    sub: &Subst,
) -> Vec<u8> {
    let cps = convert::decode(s, enc);
    let mut out = Vec::with_capacity(cps.len());
    for &cp in &cps {
        // The invalid-input marker 0xFFFFFFFF is mapped like any value
        // (test mb_encode_numericentity_large_ints.phpt).
        let hit = map.iter().find(|m| m.start <= cp && cp <= m.end);
        match hit {
            Some(m) => {
                let v = cp.wrapping_add(m.offset) & m.mask;
                let digits = if hex { format!("{v:X}") } else { v.to_string() };
                out.extend(b"&#".iter().map(|&b| b as u32));
                if hex {
                    out.push(b'x' as u32);
                }
                out.extend(digits.bytes().map(|b| b as u32));
                out.push(b';' as u32);
            }
            None => out.push(cp),
        }
    }
    convert::encode(&out, enc, sub).bytes
}

/// Decodes `&#N;`/`&#xN;` references whose value the map covers. The `;` is
/// optional; up to 10 decimal or 8 hexadecimal digits are read.
pub fn decode_numericentity(s: &[u8], map: &[MapEntry], enc: Encoding, sub: &Subst) -> Vec<u8> {
    let cps = convert::decode(s, enc);
    let mut out = Vec::with_capacity(cps.len());
    let mut i = 0;
    let n = cps.len();
    while i < n {
        if cps[i] != b'&' as u32 || i + 1 >= n || cps[i + 1] != b'#' as u32 {
            out.push(cps[i]);
            i += 1;
            continue;
        }
        // Only a lowercase 'x' starts a hexadecimal reference (observed on
        // PHP 8.5).
        let hex = cps.get(i + 2) == Some(&(b'x' as u32));
        let start = i + 2 + hex as usize;
        let mut j = start;
        let mut v: u32 = 0;
        let mut overflow = false;
        let max = if hex { 8 } else { 10 };
        while j < n {
            let c = cps[j];
            let d = match c {
                0x30..=0x39 => c - 0x30,
                0x41..=0x46 if hex => c - 0x41 + 10,
                0x61..=0x66 if hex => c - 0x61 + 10,
                _ => break,
            };
            // The value wraps at 32 bits (observed on PHP 8.5).
            // A value over u32::MAX / 10 before the next decimal digit
            // makes it no entity ("&#7001492542" stays as it is, test
            // mb_decode_numericentity_large_ints.phpt), but the last step
            // can still wrap at 32 bits (observed on PHP 8.5:
            // "&#4294967296;" decodes to U+0000).
            overflow |= !hex && v > u32::MAX / 10;
            v = v.wrapping_mul(if hex { 16 } else { 10 }).wrapping_add(d);
            j += 1;
        }
        let ndig = j - start;
        if ndig == 0 || ndig > max || overflow {
            // Not an entity: copy what was scanned; a '&' starts afresh.
            out.extend_from_slice(&cps[i..j]);
            i = j;
            continue;
        }
        // The mask does not apply when decoding (observed on PHP 8.5).
        let hit = map
            .iter()
            .find(|m| {
                let c = v.wrapping_sub(m.offset);
                m.start <= c && c <= m.end
            })
            .map(|m| v.wrapping_sub(m.offset));
        let end = if cps.get(j) == Some(&(b';' as u32)) {
            j + 1
        } else {
            j
        };
        match hit {
            Some(c) => {
                out.push(c);
                i = end;
            }
            None => {
                out.extend_from_slice(&cps[i..j]);
                i = j;
            }
        }
    }
    convert::encode(&out, enc, sub).bytes
}
