//! The `grapheme_*()` functions (PHP manual: Grapheme Functions): string
//! operations counted in extended grapheme clusters (UAX #29, the intl
//! crate's segmenter).

/// The byte offsets of the grapheme boundaries of `s` (0 and `s.len()`
/// included).
pub fn boundaries(s: &str) -> Vec<usize> {
    let mut v = vec![0];
    let mut pos = 0;
    for g in intl::unicode::graphemes(s) {
        pos += g.len();
        v.push(pos);
    }
    v
}

/// The graphemes of `s`.
pub fn graphemes(s: &str) -> Vec<&str> {
    intl::unicode::graphemes(s).collect()
}

pub fn strlen(s: &str) -> usize {
    intl::unicode::graphemes(s).count()
}

/// Case-folded (full case folding, UCD CaseFolding.txt C+F) and NFC.
pub fn fold(s: &str) -> String {
    let f: String = intl::unicode::fold(s.chars()).collect();
    intl::unicode::nfc(f.chars()).collect()
}

fn units(s: &str, ci: bool) -> Vec<String> {
    graphemes(s)
        .into_iter()
        .map(|g| if ci { fold(g) } else { g.to_string() })
        .collect()
}

/// Whether the needle's graphemes are at grapheme `i` of the haystack's.
fn matches_at(h: &[String], n: &[String], i: usize) -> bool {
    i + n.len() <= h.len() && h[i..i + n.len()] == *n
}

/// An offset error: the offset is outside the haystack.
#[derive(Debug, PartialEq)]
pub struct OffsetError;

/// The grapheme index of an offset (negative from the end).
fn resolve_offset(count: usize, offset: i64) -> Result<usize, OffsetError> {
    let c = count as i64;
    let o = if offset < 0 { c + offset } else { offset };
    if o < 0 || o > c {
        return Err(OffsetError);
    }
    Ok(o as usize)
}

/// Graphemes compared by a predicate (a collator's equality: PHP
/// searches with ICU's collation-based string search, so `ß` matches
/// `ss` at primary strength: ext/intl/tests/grapheme_strpos_locale_
/// dependency.phpt).
pub type Eq<'a> = &'a dyn Fn(&str, &str) -> bool;

/// Whether the needle matches at grapheme `i` with `eq`: some run of
/// whole graphemes from `i` equals it.
fn matches_with(h: &str, b: &[usize], n: &str, i: usize, eq: Eq<'_>) -> bool {
    if n.is_empty() {
        return true;
    }
    (i + 1..b.len()).any(|j| eq(&h[b[i]..b[j]], n))
}

/// First match at or after grapheme `offset` with `eq`.
pub fn strpos_with(h: &str, n: &str, offset: i64, eq: Eq<'_>) -> Result<Option<usize>, OffsetError> {
    let b = boundaries(h);
    let count = b.len() - 1;
    let start = resolve_offset(count, offset)?;
    Ok((start..=count).find(|&i| (i < count || n.is_empty()) && matches_with(h, &b, n, i, eq)))
}

/// Last match with `eq` (offsets as [`strrpos`]).
pub fn strrpos_with(h: &str, n: &str, offset: i64, eq: Eq<'_>) -> Result<Option<usize>, OffsetError> {
    let b = boundaries(h);
    let count = b.len() - 1;
    let at = resolve_offset(count, offset)?;
    let (lo, hi) = if offset >= 0 { (at, count) } else { (0, at) };
    let mut i = hi + 1;
    while i > lo {
        i -= 1;
        if (i < count || n.is_empty()) && matches_with(h, &b, n, i, eq) {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

/// `grapheme_strpos()` / `grapheme_stripos()`: the grapheme index of the
/// first match at or after `offset`.
pub fn strpos(h: &str, n: &str, offset: i64, ci: bool) -> Result<Option<usize>, OffsetError> {
    let hu = units(h, ci);
    let nu = units(n, ci);
    let start = resolve_offset(hu.len(), offset)?;
    Ok((start..=hu.len()).find(|&i| matches_at(&hu, &nu, i)))
}

/// `grapheme_strrpos()` / `grapheme_strripos()`: the grapheme index of
/// the last match (starting at or after a positive offset, at or before
/// the position of a negative one).
pub fn strrpos(h: &str, n: &str, offset: i64, ci: bool) -> Result<Option<usize>, OffsetError> {
    let hu = units(h, ci);
    let nu = units(n, ci);
    let at = resolve_offset(hu.len(), offset)?;
    let (lo, hi) = if offset >= 0 {
        (at, hu.len())
    } else {
        (0, at.min(hu.len()))
    };
    let mut i = hi + 1;
    while i > lo {
        i -= 1;
        if matches_at(&hu, &nu, i) {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

/// The byte range of graphemes `[from, to)` of `s`.
pub fn byte_range(s: &str, from: usize, to: usize) -> (usize, usize) {
    let b = boundaries(s);
    (b[from.min(b.len() - 1)], b[to.min(b.len() - 1)])
}

/// `grapheme_substr()`: None when the offset is outside the string.
pub fn substr(s: &str, offset: i64, length: Option<i64>) -> Option<String> {
    let b = boundaries(s);
    let count = (b.len() - 1) as i64;
    let start = (if offset < 0 { count + offset } else { offset }).clamp(0, count);
    let end = match length {
        None => count,
        Some(l) if l < 0 => count + l,
        Some(l) => start.saturating_add(l),
    };
    let end = end.clamp(start, count);
    Some(s[b[start as usize]..b[end as usize]].to_string())
}

/// `grapheme_strstr()` / `grapheme_stristr()`: the part from the first
/// match (or before it).
pub fn strstr(h: &str, n: &str, before: bool, ci: bool) -> Option<String> {
    let pos = strpos(h, n, 0, ci).ok()??;
    let b = boundaries(h);
    let at = b[pos];
    Some(if before {
        h[..at].to_string()
    } else {
        h[at..].to_string()
    })
}

/// `grapheme_extract()` types.
pub const EXTR_COUNT: i64 = 0;
pub const EXTR_MAXBYTES: i64 = 1;
pub const EXTR_MAXCHARS: i64 = 2;

/// `grapheme_extract()`: the extracted bytes and the next byte offset.
pub fn extract(h: &str, size: usize, ty: i64, start: usize) -> (String, usize) {
    let rest = &h[start..];
    let mut out = 0;
    let mut chars = 0;
    for (n, g) in intl::unicode::graphemes(rest).enumerate() {
        let ok = match ty {
            EXTR_MAXBYTES => out + g.len() <= size,
            EXTR_MAXCHARS => chars + g.chars().count() <= size,
            _ => n < size,
        };
        if !ok {
            break;
        }
        out += g.len();
        chars += g.chars().count();
    }
    (rest[..out].to_string(), start + out)
}

/// `grapheme_str_split()`.
pub fn str_split(s: &str, len: usize) -> Vec<String> {
    let gs = graphemes(s);
    gs.chunks(len.max(1)).map(|c| c.concat()).collect()
}

/// `grapheme_levenshtein()`: the edit distance over graphemes, two
/// graphemes being the same when `same` says so (a collator's equality).
pub fn levenshtein(
    a: &str,
    b: &str,
    ins: i64,
    rep: i64,
    del: i64,
    same: &dyn Fn(&str, &str) -> bool,
) -> i64 {
    let x = graphemes(a);
    let y = graphemes(b);
    if x.is_empty() {
        return y.len() as i64 * ins;
    }
    if y.is_empty() {
        return x.len() as i64 * del;
    }
    let mut prev: Vec<i64> = (0..=y.len() as i64).map(|j| j * ins).collect();
    let mut cur = vec![0i64; y.len() + 1];
    for (i, gx) in x.iter().enumerate() {
        cur[0] = (i as i64 + 1) * del;
        for (j, gy) in y.iter().enumerate() {
            let c0 = prev[j] + if same(gx, gy) { 0 } else { rep };
            let c1 = prev[j + 1] + del;
            let c2 = cur[j] + ins;
            cur[j + 1] = c0.min(c1).min(c2);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[y.len()]
}
