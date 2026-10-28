//! Natural-order string comparison: `strnatcmp()`, `strnatcasecmp()`, and
//! the `SORT_NATURAL` sort flag.
//!
//! Digit runs compare as numbers, other bytes one by one (PHP manual:
//! strnatcmp, "natural order" as described by Martin Pool). The details
//! (whitespace, leading zeros, fractional runs) are matched against
//! observations on PHP 8.5 (tests/fixtures/strings.txt, 2304 pairs per
//! function) and tests ext/standard/tests/strings/strnat*.phpt.

use std::cmp::Ordering;

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
}

/// Skips whitespace, unless only whitespace remains (observed on PHP 8.5:
/// "1" sorts before "1 ", while "x 1" equals "x  1").
fn skip_space(s: &[u8], mut i: usize) -> usize {
    let from = i;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    if i == s.len() { from } else { i }
}

/// Compares two strings in natural order; `ci` folds ASCII case.
pub fn natcmp(a: &[u8], b: &[u8], ci: bool) -> Ordering {
    let fold = |c: u8| if ci { c.to_ascii_lowercase() } else { c };
    // Observed on PHP 8.5: leading zeros at the very start are ignored
    // ("01" equals "1", "00" equals "0"), not after whitespace (test
    // strnatcmp_leftalign.phpt: " 00" sorts after " 0").
    let start = |s: &[u8]| {
        let mut i = 0;
        while i + 1 < s.len() && s[i] == b'0' && s[i + 1].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let (mut i, mut j) = (start(a), start(b));
    loop {
        i = skip_space(a, i);
        j = skip_space(b, j);
        let ca = a.get(i).copied();
        let cb = b.get(j).copied();
        match (ca, cb) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let si = i;
                while i < a.len() && a[i].is_ascii_digit() {
                    i += 1;
                }
                let sj = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                let ra = &a[si..i];
                let rb = &b[sj..j];
                let o = if ra[0] == b'0' || rb[0] == b'0' {
                    // Fractional comparison: digit by digit, left aligned.
                    ra.cmp(rb)
                } else {
                    ra.len().cmp(&rb.len()).then_with(|| ra.cmp(rb))
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
            // Trailing whitespace of both sides compares equal, whatever
            // the bytes (observed on PHP 8.5: strnatcmp("\f", "\n") is 0;
            // test ext/standard/tests/array/sort/natcasesort_variation4.phpt).
            (Some(x), Some(y)) if is_space(x) && is_space(y) => {
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) => {
                let o = fold(x).cmp(&fold(y));
                if o != Ordering::Equal {
                    return o;
                }
                i += 1;
                j += 1;
            }
        }
    }
}

/// `strnatcmp()` / `strnatcasecmp()`: -1, 0 or 1.
pub fn strnatcmp(a: &[u8], b: &[u8], ci: bool) -> i64 {
    match natcmp(a, b, ci) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}
