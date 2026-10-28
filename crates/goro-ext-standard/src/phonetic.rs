//! String similarity and phonetic functions: `similar_text()`,
//! `levenshtein()`, `soundex()`, `metaphone()`.
//!
//! Sources: PHP manual pages; similar_text follows the algorithm of Oliver
//! ("Programming Classics", 1993, as the manual cites); levenshtein is the
//! standard edit-distance recurrence with PHP's insertion, replacement and
//! deletion costs; soundex is the American Soundex published by the US
//! National Archives; metaphone follows Lawrence Philips' original rules
//! (Computer Language, December 1990). Details checked against tests
//! ext/standard/tests/strings/{similar_text,levenshtein,soundex,metaphone}*
//! and observations on PHP 8.5 (tests/fixtures/strings.txt).

use crate::error::StdError;

/// `similar_text()`: the number of matching characters, and the percentage
/// `matches * 2 * 100 / (len(a) + len(b))`.
pub fn similar_text(a: &[u8], b: &[u8]) -> (i64, f64) {
    let sim = similar(a, b);
    let total = a.len() + b.len();
    let pct = if total == 0 {
        0.0
    } else {
        sim as f64 * 2.0 * 100.0 / total as f64
    };
    (sim as i64, pct)
}

fn similar(a: &[u8], b: &[u8]) -> usize {
    // The first longest common substring, then the same on both sides.
    let mut max = 0;
    let (mut pa, mut pb) = (0, 0);
    for i in 0..a.len() {
        for j in 0..b.len() {
            let mut k = 0;
            while i + k < a.len() && j + k < b.len() && a[i + k] == b[j + k] {
                k += 1;
            }
            if k > max {
                max = k;
                pa = i;
                pb = j;
            }
        }
    }
    if max == 0 {
        return 0;
    }
    max + similar(&a[..pa], &b[..pb]) + similar(&a[pa + max..], &b[pb + max..])
}

/// `levenshtein()`: the cost of turning `a` into `b`.
pub fn levenshtein(a: &[u8], b: &[u8], ins: i64, rep: i64, del: i64) -> i64 {
    if a.is_empty() {
        return b.len() as i64 * ins;
    }
    if b.is_empty() {
        return a.len() as i64 * del;
    }
    let mut prev: Vec<i64> = (0..=b.len() as i64).map(|j| j * ins).collect();
    let mut cur = vec![0i64; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = (i as i64 + 1) * del;
        for (j, &cb) in b.iter().enumerate() {
            let sub = prev[j] + if ca == cb { 0 } else { rep };
            let insert = cur[j] + ins;
            let delete = prev[j + 1] + del;
            cur[j + 1] = sub.min(insert).min(delete);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn soundex_code(c: u8) -> u8 {
    match c {
        b'B' | b'F' | b'P' | b'V' => b'1',
        b'C' | b'G' | b'J' | b'K' | b'Q' | b'S' | b'X' | b'Z' => b'2',
        b'D' | b'T' => b'3',
        b'L' => b'4',
        b'M' | b'N' => b'5',
        b'R' => b'6',
        _ => 0,
    }
}

/// `soundex()`: a letter and three digits.
pub fn soundex(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4);
    let mut last = 0u8;
    for &c in s {
        let c = c.to_ascii_uppercase();
        if !c.is_ascii_uppercase() {
            continue;
        }
        let code = soundex_code(c);
        if out.is_empty() {
            out.push(c);
            last = code;
        } else {
            if code != 0 && code != last {
                out.push(code);
                if out.len() == 4 {
                    break;
                }
            }
            // Observed on PHP 8.5: H and W separate letters with the same
            // code, as vowels do ("Ashcraft" is A226).
            last = code;
        }
    }
    // Observed on PHP 8.5: a string without letters gives "0000".
    while out.len() < 4 {
        out.push(b'0');
    }
    out
}

fn is_vowel(c: u8) -> bool {
    matches!(c, b'A' | b'E' | b'I' | b'O' | b'U')
}

/// `metaphone()`: the Metaphone key of the ASCII letters of `s`, at most
/// `max_phonemes` phonemes when positive.
pub fn metaphone(s: &[u8], max_phonemes: i64) -> Result<Vec<u8>, StdError> {
    if max_phonemes < 0 {
        return Err(StdError::ValueError(
            "metaphone(): Argument #2 ($max_phonemes) must be greater than or equal to 0".into(),
        ));
    }
    // Non-letters are skipped but still end words: lookups see them as
    // boundaries (test metaphone.phpt: "grovelling heedlessly" is
    // ...NKHTLSL).
    let w: Vec<u8> = s
        .iter()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                c.to_ascii_uppercase()
            } else {
                0
            }
        })
        .collect();
    let max = if max_phonemes == 0 {
        usize::MAX
    } else {
        max_phonemes as usize
    };
    let mut out = Vec::new();
    let at = |i: isize| -> u8 {
        if i < 0 {
            0
        } else {
            w.get(i as usize).copied().unwrap_or(0)
        }
    };
    let first = w.iter().position(|&c| c != 0).unwrap_or(w.len());
    let f = first as isize;
    let mut i = first;
    // Initial-letter exceptions.
    match (at(f), at(f + 1)) {
        (b'A', b'E') => {
            out.push(b'E');
            i += 2;
        }
        (b'G', b'N') | (b'K', b'N') | (b'P', b'N') | (b'W', b'R') => {
            out.push(at(f + 1));
            i += 2;
        }
        (b'X', _) => {
            out.push(b'S');
            i += 1;
        }
        (b'W', b'H') => {
            out.push(b'W');
            i += 2;
        }
        _ => {}
    }
    while i < w.len() && out.len() < max {
        let c = w[i];
        if c == 0 {
            i += 1;
            continue;
        }
        let ii = i as isize;
        let prev = at(ii - 1);
        let next = at(ii + 1);
        let after = at(ii + 2);
        // Doubled letters count once, except C.
        if c == prev && c != b'C' {
            i += 1;
            continue;
        }
        let mut skip = 0;
        match c {
            b'A' | b'E' | b'I' | b'O' | b'U' => {
                if i == first {
                    out.push(c);
                }
            }
            b'B' => {
                // Observed on PHP 8.5: B after M is silent anywhere ("number" is NMR).
                if prev != b'M' {
                    out.push(b'B');
                }
            }
            b'C' => {
                if next == b'I' && after == b'A' {
                    out.push(b'X');
                } else if next == b'H' {
                    out.push(b'X');
                    skip = 1;
                } else if matches!(next, b'I' | b'E' | b'Y') {
                    if prev != b'S' {
                        out.push(b'S');
                    }
                } else {
                    out.push(b'K');
                }
            }
            b'D' => {
                if next == b'G' && matches!(after, b'E' | b'Y' | b'I') {
                    out.push(b'J');
                    skip = 1;
                } else {
                    out.push(b'T');
                }
            }
            b'G' => {
                if next == b'H' {
                    // Observed on PHP 8.5: "GH" is F ("laugh", "knight",
                    // "ghost"), silent when the letter three back is B, D or
                    // H or the one four back is H ("dough", "bough").
                    if !(matches!(at(ii - 3), b'B' | b'D' | b'H') || at(ii - 4) == b'H') {
                        out.push(b'F');
                    }
                    skip = 1;
                } else if next == b'N' {
                    if !(after == 0 || (after == b'E' && at(ii + 3) == b'D' && at(ii + 4) == 0)) {
                        out.push(b'K');
                    }
                } else if matches!(next, b'I' | b'E' | b'Y') && prev != b'G' {
                    out.push(b'J');
                } else {
                    out.push(b'K');
                }
            }
            b'H' => {
                if is_vowel(next) && !matches!(prev, b'C' | b'S' | b'P' | b'T' | b'G') {
                    out.push(b'H');
                }
            }
            b'K' => {
                if prev != b'C' {
                    out.push(b'K');
                }
            }
            b'P' => {
                if next == b'H' {
                    out.push(b'F');
                } else {
                    out.push(b'P');
                }
            }
            b'Q' => out.push(b'K'),
            b'S' => {
                if next == b'H' {
                    out.push(b'X');
                    skip = 1;
                } else if next == b'I' && matches!(after, b'O' | b'A') {
                    out.push(b'X');
                } else {
                    out.push(b'S');
                }
            }
            b'T' => {
                if next == b'I' && matches!(after, b'O' | b'A') {
                    out.push(b'X');
                } else if next == b'H' {
                    out.push(b'0');
                    skip = 1;
                } else if !(next == b'C' && after == b'H') {
                    out.push(b'T');
                }
            }
            b'V' => out.push(b'F'),
            b'W' | b'Y' => {
                if is_vowel(next) {
                    out.push(c);
                }
            }
            b'X' => {
                out.push(b'K');
                if out.len() < max {
                    out.push(b'S');
                }
            }
            b'Z' => out.push(b'S'),
            _ => out.push(c),
        }
        i += 1 + skip;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // test ext/standard/tests/strings/soundex.phpt (classic examples)
    #[test]
    fn soundex_examples() {
        assert_eq!(soundex(b"Euler"), b"E460");
        assert_eq!(soundex(b"Gauss"), b"G200");
        assert_eq!(soundex(b"Hilbert"), b"H416");
        assert_eq!(soundex(b"Knuth"), b"K530");
        assert_eq!(soundex(b"Lloyd"), b"L300");
        assert_eq!(soundex(b"Lukasiewicz"), b"L222");
        assert_eq!(soundex(b""), b"0000");
    }

    // test ext/standard/tests/strings/metaphone.phpt
    #[test]
    fn metaphone_phpt() {
        assert_eq!(metaphone(b"", 0).unwrap(), b"");
        assert_eq!(metaphone(b"-1", 0).unwrap(), b"");
        assert!(metaphone(b"valid phrase", -1).is_err());
        assert_eq!(metaphone(b"valid phrase", 0).unwrap(), b"FLTFRS");
        assert_eq!(metaphone(b"valid phrase", 10000).unwrap(), b"FLTFRS");
        let cases: [(&str, &str); 4] = [
            (
                "They fell forward, grovelling heedlessly on the cold earth.",
                "0FLFRWRTKRFLNKHTLSLN0KLTR0",
            ),
            (
                "But the shadow of horror wheeled and returned, passing lower now, right above them, sweeping the fen-reek with its ghastly wings.",
                "BT0XTFHRRHLTNTRTRNTPSNKLWRNRFTBF0MSWPNK0FNRKW0TSFSTLWNKS",
            ),
            (
                "And then it was gone, flying back to Mordor with the speed of the wrath of Sauron; and behind it the wind roared away, leaving the Dead Marshes bare and bleak.",
                "ANT0NTWSKNFLYNKBKTMRTRW00SPTF0R0FSRNNTBHNTT0WNTRRTWLFNK0TTMRXSBRNTBLK",
            ),
            (
                "The naked waste, as far as the eye could pierce, even to the distant menace of the mountains, was dappled with the fitful moonlight.",
                "0NKTWSTSFRS0YKLTPRSFNT0TSTNTMNSF0MNTNSWSTPLTW00FTFLMNLFT",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(
                String::from_utf8(metaphone(input.as_bytes(), 0).unwrap()).unwrap(),
                want,
                "{input}"
            );
        }
    }

    #[test]
    fn similar_and_levenshtein() {
        assert_eq!(similar_text(b"World", b"Word").0, 4);
        assert_eq!(levenshtein(b"kitten", b"sitting", 1, 1, 1), 3);
    }
}
