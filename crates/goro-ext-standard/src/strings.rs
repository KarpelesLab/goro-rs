//! String functions over byte strings.
//!
//! PHP strings are bytes: everything here takes `&[u8]` and returns
//! `Vec<u8>`. Case mapping is ASCII-only, as in PHP 8.2 and later (PHP
//! manual: strtolower, "no longer locale-sensitive").
//!
//! Behaviour sources: the PHP manual function pages, tests
//! ext/standard/tests/strings/*.phpt, and observations on PHP 8.5 kept in
//! `tests/fixtures/strings.txt`.

use crate::error::{Diag, StdError};

fn value_error(m: impl Into<String>) -> StdError {
    StdError::ValueError(m.into())
}

/// `str_pad()` pad types.
pub const STR_PAD_LEFT: i64 = 0;
/// `STR_PAD_RIGHT`.
pub const STR_PAD_RIGHT: i64 = 1;
/// `STR_PAD_BOTH`.
pub const STR_PAD_BOTH: i64 = 2;

/// `str_pad()`.
pub fn str_pad(input: &[u8], length: i64, pad: &[u8], pad_type: i64) -> Result<Vec<u8>, StdError> {
    if length < 0 || length as u64 <= input.len() as u64 {
        return Ok(input.to_vec());
    }
    if pad.is_empty() {
        return Err(value_error(
            "str_pad(): Argument #3 ($pad_string) must not be empty",
        ));
    }
    let (left, right) = match pad_type {
        STR_PAD_LEFT => (length as usize - input.len(), 0),
        STR_PAD_RIGHT => (0, length as usize - input.len()),
        STR_PAD_BOTH => {
            let n = length as usize - input.len();
            (n / 2, n - n / 2)
        }
        _ => {
            return Err(value_error(
                "str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH",
            ));
        }
    };
    let mut out = Vec::with_capacity(length as usize);
    out.extend(pad.iter().cycle().take(left));
    out.extend_from_slice(input);
    out.extend(pad.iter().cycle().take(right));
    Ok(out)
}

/// `wordwrap()`.
pub fn wordwrap(text: &[u8], width: i64, brk: &[u8], cut: bool) -> Result<Vec<u8>, StdError> {
    // Observed on PHP 8.5: an empty text returns before the argument checks.
    if text.is_empty() {
        return Ok(Vec::new());
    }
    if brk.is_empty() {
        return Err(value_error(
            "wordwrap(): Argument #3 ($break) must not be empty",
        ));
    }
    if width == 0 && cut {
        return Err(value_error(
            "wordwrap(): Argument #4 ($cut_long_words) cannot be true when argument #2 ($width) is 0",
        ));
    }
    let width = width.max(0) as usize;
    let mut out = Vec::with_capacity(text.len() + text.len() / width.max(1) * brk.len());
    let mut line_start = 0usize;
    let mut last_space: Option<usize> = None;
    let mut i = 0usize;
    while i < text.len() {
        let c = text[i];
        if c == brk[0] && i + brk.len() < text.len() && text[i..].starts_with(brk) {
            // An existing break ends the line.
            out.extend_from_slice(&text[line_start..i + brk.len()]);
            i += brk.len();
            line_start = i;
            last_space = None;
            continue;
        }
        if c == b' ' {
            if i - line_start >= width {
                out.extend_from_slice(&text[line_start..i]);
                out.extend_from_slice(brk);
                line_start = i + 1;
                last_space = None;
            } else {
                last_space = Some(i);
            }
        } else if i - line_start >= width {
            match last_space {
                // A space opening the line is not a break point (observed:
                // "abc  def" at width 3 keeps " def" together).
                Some(s) if s > line_start => {
                    out.extend_from_slice(&text[line_start..s]);
                    out.extend_from_slice(brk);
                    line_start = s + 1;
                    last_space = None;
                    if cut && i - line_start >= width {
                        out.extend_from_slice(&text[line_start..i]);
                        out.extend_from_slice(brk);
                        line_start = i;
                    }
                }
                _ if cut => {
                    out.extend_from_slice(&text[line_start..i]);
                    out.extend_from_slice(brk);
                    line_start = i;
                    last_space = None;
                }
                _ => {}
            }
        }
        i += 1;
    }
    out.extend_from_slice(&text[line_start..]);
    Ok(out)
}

/// `chunk_split()`.
pub fn chunk_split(s: &[u8], length: i64, end: &[u8]) -> Result<Vec<u8>, StdError> {
    if length < 1 {
        return Err(value_error(
            "chunk_split(): Argument #2 ($length) must be greater than 0",
        ));
    }
    let len = length as usize;
    if len > s.len() {
        let mut out = s.to_vec();
        out.extend_from_slice(end);
        return Ok(out);
    }
    let mut out = Vec::with_capacity(s.len() + (s.len() / len + 1) * end.len());
    for chunk in s.chunks(len) {
        out.extend_from_slice(chunk);
        out.extend_from_slice(end);
    }
    Ok(out)
}

/// `nl2br()`: inserts `<br />` (or `<br>`) before each `\r\n`, `\n\r`,
/// `\n` and `\r`.
pub fn nl2br(s: &[u8], is_xhtml: bool) -> Vec<u8> {
    let tag: &[u8] = if is_xhtml { b"<br />" } else { b"<br>" };
    let mut out = Vec::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'\r' || c == b'\n' {
            out.extend_from_slice(tag);
            out.push(c);
            if i + 1 < s.len() && (s[i + 1] == b'\r' || s[i + 1] == b'\n') && s[i + 1] != c {
                out.push(s[i + 1]);
                i += 1;
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// `strrev()`.
pub fn strrev(s: &[u8]) -> Vec<u8> {
    s.iter().rev().copied().collect()
}

/// `strtolower()` (ASCII).
pub fn strtolower(s: &[u8]) -> Vec<u8> {
    s.to_ascii_lowercase()
}

/// `strtoupper()` (ASCII).
pub fn strtoupper(s: &[u8]) -> Vec<u8> {
    s.to_ascii_uppercase()
}

/// `ucfirst()`.
pub fn ucfirst(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    if let Some(c) = v.first_mut() {
        c.make_ascii_uppercase();
    }
    v
}

/// `lcfirst()`.
pub fn lcfirst(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    if let Some(c) = v.first_mut() {
        c.make_ascii_lowercase();
    }
    v
}

/// The default `ucwords()` delimiters.
pub const UCWORDS_DELIMITERS: &[u8] = b" \t\r\n\x0c\x0b";

/// `ucwords()`: uppercases the first character and every character that
/// follows one of `delimiters`.
pub fn ucwords(s: &[u8], delimiters: &[u8]) -> Vec<u8> {
    let mut mask = [false; 256];
    for &d in delimiters {
        mask[d as usize] = true;
    }
    let mut v = s.to_vec();
    let mut at_start = true;
    for c in v.iter_mut() {
        if at_start {
            c.make_ascii_uppercase();
        }
        at_start = mask[*c as usize];
    }
    v
}

/// The result of `str_word_count()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordCount {
    /// Format 0: the number of words.
    Count(i64),
    /// Format 1: the words.
    Words(Vec<Vec<u8>>),
    /// Format 2: the words keyed by their byte offset.
    Positions(Vec<(i64, Vec<u8>)>),
}

/// Parses a character list with `a..z` ranges into a byte mask, as
/// `trim()`, `addcslashes()`, `str_word_count()` and `ucwords()`-style
/// functions read their character arguments (PHP manual: trim). Invalid
/// ranges produce the warnings PHP gives, and their characters are taken
/// literally.
pub fn char_mask(list: &[u8], diags: &mut Vec<Diag>) -> [bool; 256] {
    let mut mask = [false; 256];
    let n = list.len();
    let mut i = 0;
    while i < n {
        let c = list[i];
        if i + 3 < n && list[i + 1] == b'.' && list[i + 2] == b'.' && list[i + 3] >= c {
            for b in c..=list[i + 3] {
                mask[b as usize] = true;
            }
            i += 4;
            continue;
        }
        if i + 1 < n && c == b'.' && list[i + 1] == b'.' {
            let why = if i == 0 {
                "Invalid '..'-range, no character to the left of '..'"
            } else if i + 2 >= n {
                "Invalid '..'-range, no character to the right of '..'"
            } else if list[i - 1] > list[i + 2] {
                "Invalid '..'-range, '..'-range needs to be incrementing"
            } else {
                "Invalid '..'-range"
            };
            diags.push(Diag::Warning(why.into()));
            i += 1;
            continue;
        }
        mask[c as usize] = true;
        i += 1;
    }
    mask
}

/// `str_word_count()`. Words are runs of ASCII letters, `'` and `-`, plus
/// the characters of `charlist` (observed on PHP 8.5: a word may start or
/// end with `'` or `-` when next to letters).
pub fn str_word_count(
    s: &[u8],
    format: i64,
    charlist: Option<&[u8]>,
    diags: &mut Vec<Diag>,
) -> Result<WordCount, StdError> {
    if !(0..=2).contains(&format) {
        return Err(value_error(
            "str_word_count(): Argument #2 ($format) must be a valid format value",
        ));
    }
    let extra = match charlist {
        Some(l) if !l.is_empty() => char_mask(l, diags),
        _ => [false; 256],
    };
    let is_word = |c: u8| c.is_ascii_alphabetic() || c == b'\'' || c == b'-' || extra[c as usize];
    let mut count = 0i64;
    let mut words = Vec::new();
    let mut positions = Vec::new();
    let mut i = 0;
    let n = s.len();
    // A leading "'" or "-" does not start a word unless allowed by the list
    // ... unless observation says otherwise; see the fixture tests.
    while i < n {
        if !is_word(s[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && is_word(s[i]) {
            i += 1;
        }
        count += 1;
        match format {
            1 => words.push(s[start..i].to_vec()),
            2 => positions.push((start as i64, s[start..i].to_vec())),
            _ => {}
        }
    }
    Ok(match format {
        0 => WordCount::Count(count),
        1 => WordCount::Words(words),
        _ => WordCount::Positions(positions),
    })
}

/// The default characters `trim()` strips.
pub const TRIM_DEFAULT: &[u8] = b" \t\n\r\0\x0b";

/// Which ends `trim()` works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMode {
    /// `ltrim()`.
    Left,
    /// `rtrim()`.
    Right,
    /// `trim()`.
    Both,
}

/// `trim()`, `ltrim()` and `rtrim()`; `characters` accepts `a..z` ranges.
pub fn trim(s: &[u8], characters: Option<&[u8]>, mode: TrimMode, diags: &mut Vec<Diag>) -> Vec<u8> {
    let mask = match characters {
        None => {
            let mut m = [false; 256];
            for &c in TRIM_DEFAULT {
                m[c as usize] = true;
            }
            m
        }
        Some(l) => char_mask(l, diags),
    };
    let mut start = 0;
    let mut end = s.len();
    if mode != TrimMode::Right {
        while start < end && mask[s[start] as usize] {
            start += 1;
        }
    }
    if mode != TrimMode::Left {
        while end > start && mask[s[end - 1] as usize] {
            end -= 1;
        }
    }
    s[start..end].to_vec()
}

/// `addslashes()`: backslashes before `'`, `"`, `\` and NUL (as `\0`).
pub fn addslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 8);
    for &c in s {
        match c {
            b'\'' | b'"' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            0 => out.extend_from_slice(b"\\0"),
            _ => out.push(c),
        }
    }
    out
}

/// `stripslashes()`.
pub fn stripslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' {
            i += 1;
            if i < s.len() {
                out.push(if s[i] == b'0' { 0 } else { s[i] });
            }
        } else {
            out.push(s[i]);
        }
        i += 1;
    }
    out
}

/// `addcslashes()`.
pub fn addcslashes(s: &[u8], characters: &[u8], diags: &mut Vec<Diag>) -> Vec<u8> {
    let mask = char_mask(characters, diags);
    let mut out = Vec::with_capacity(s.len() + 8);
    for &c in s {
        if !mask[c as usize] {
            out.push(c);
            continue;
        }
        out.push(b'\\');
        if !(32..=126).contains(&c) {
            match c {
                b'\n' => out.push(b'n'),
                b'\t' => out.push(b't'),
                b'\r' => out.push(b'r'),
                0x07 => out.push(b'a'),
                0x0b => out.push(b'v'),
                0x08 => out.push(b'b'),
                0x0c => out.push(b'f'),
                _ => out.extend_from_slice(format!("{c:03o}").as_bytes()),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `stripcslashes()`: C-style escapes (`\n`, `\x41`, `\101`, ...).
pub fn stripcslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    let n = s.len();
    while i < n {
        if s[i] != b'\\' || i + 1 >= n {
            out.push(s[i]);
            i += 1;
            continue;
        }
        i += 1;
        let c = s[i];
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(0x07),
            b'v' => out.push(0x0b),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'x' if i + 1 < n && s[i + 1].is_ascii_hexdigit() => {
                let mut v = 0u32;
                let mut k = 0;
                while k < 2 && i + 1 < n && s[i + 1].is_ascii_hexdigit() {
                    v = v * 16 + (s[i + 1] as char).to_digit(16).unwrap();
                    i += 1;
                    k += 1;
                }
                out.push(v as u8);
            }
            b'0'..=b'7' => {
                let mut v = 0u32;
                let mut k = 0;
                while k < 3 && i < n && (b'0'..=b'7').contains(&s[i]) {
                    v = v * 8 + (s[i] - b'0') as u32;
                    i += 1;
                    k += 1;
                }
                out.push(v as u8);
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// `quotemeta()`: backslashes before `. \ + * ? [ ^ ] $ ( )`.
pub fn quotemeta(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 8);
    for &c in s {
        if matches!(
            c,
            b'.' | b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'$' | b'(' | b')'
        ) {
            out.push(b'\\');
        }
        out.push(c);
    }
    out
}

/// `strtr($string, $from, $to)`: byte-for-byte translation over the
/// shorter of the two lists.
pub fn strtr_chars(s: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let n = from.len().min(to.len());
    if n == 0 {
        return s.to_vec();
    }
    let mut map: [u8; 256] = std::array::from_fn(|i| i as u8);
    for i in 0..n {
        map[from[i] as usize] = to[i];
    }
    s.iter().map(|&c| map[c as usize]).collect()
}

/// `strtr($string, $replace_pairs)`: at each position the longest matching
/// key is replaced, and replaced text is not scanned again. Empty keys are
/// ignored with a warning.
pub fn strtr_pairs(s: &[u8], pairs: &[(Vec<u8>, Vec<u8>)], diags: &mut Vec<Diag>) -> Vec<u8> {
    // Observed on PHP 8.5: an empty subject returns before the key checks.
    if s.is_empty() {
        return Vec::new();
    }
    if pairs.iter().any(|(k, _)| k.is_empty()) {
        diags.push(Diag::Warning("Ignoring replacement of empty string".into()));
    }
    let mut keys: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();
    if keys.is_empty() {
        return s.to_vec();
    }
    // Later duplicates win, like array keys; then longest first.
    keys.reverse();
    let mut seen = std::collections::HashSet::new();
    keys.retain(|(k, _)| seen.insert(*k));
    keys.sort_by_key(|k| std::cmp::Reverse(k.0.len()));
    let min_len = keys.iter().map(|(k, _)| k.len()).min().unwrap_or(1);
    let mut first = [false; 256];
    for (k, _) in &keys {
        first[k[0] as usize] = true;
    }
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < s.len() {
        if first[s[i] as usize] && s.len() - i >= min_len {
            for (k, v) in &keys {
                if s[i..].starts_with(k) {
                    out.extend_from_slice(v);
                    i += k.len();
                    continue 'outer;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn find(h: &[u8], n: &[u8], from: usize, ci: bool) -> Option<usize> {
    if n.is_empty() {
        return if from <= h.len() { Some(from) } else { None };
    }
    if n.len() > h.len() || from > h.len() - n.len() {
        return None;
    }
    if ci {
        let n0 = n[0].to_ascii_lowercase();
        (from..=h.len() - n.len())
            .find(|&i| h[i].to_ascii_lowercase() == n0 && h[i..i + n.len()].eq_ignore_ascii_case(n))
    } else if n.len() == 1 {
        h[from..].iter().position(|&c| c == n[0]).map(|p| p + from)
    } else {
        h[from..]
            .windows(n.len())
            .position(|w| w == n)
            .map(|p| p + from)
    }
}

fn rfind(h: &[u8], n: &[u8], ci: bool) -> Option<usize> {
    if n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).rev().find(|&i| {
        if ci {
            h[i..i + n.len()].eq_ignore_ascii_case(n)
        } else {
            &h[i..i + n.len()] == n
        }
    })
}

/// Replaces every occurrence of `search` in `subject`, left to right and
/// without overlap; returns the new string and the number of
/// replacements. An empty `search` replaces nothing.
pub fn replace(subject: &[u8], search: &[u8], replace: &[u8], ci: bool) -> (Vec<u8>, usize) {
    if search.is_empty() || search.len() > subject.len() {
        return (subject.to_vec(), 0);
    }
    let mut out = Vec::with_capacity(subject.len());
    let mut count = 0;
    let mut pos = 0;
    while let Some(p) = find(subject, search, pos, ci) {
        out.extend_from_slice(&subject[pos..p]);
        out.extend_from_slice(replace);
        pos = p + search.len();
        count += 1;
    }
    out.extend_from_slice(&subject[pos..]);
    (out, count)
}

/// The replacement side of `str_replace()` with an array search.
#[derive(Debug, Clone, Copy)]
pub enum Replacement<'a> {
    /// One string for every search string.
    Str(&'a [u8]),
    /// One string per search string; missing ones are empty.
    List(&'a [Vec<u8>]),
}

/// `str_replace()`/`str_ireplace()` on one subject string: each search
/// string is applied in turn to the result of the previous one. Returns the
/// result and the total number of replacements.
pub fn str_replace_one(
    subject: &[u8],
    searches: &[Vec<u8>],
    replacement: Replacement<'_>,
    ci: bool,
) -> (Vec<u8>, usize) {
    let mut cur = subject.to_vec();
    let mut total = 0;
    for (i, s) in searches.iter().enumerate() {
        let r: &[u8] = match replacement {
            Replacement::Str(r) => r,
            Replacement::List(l) => l.get(i).map_or(&[], |v| v.as_slice()),
        };
        let (next, n) = replace(&cur, s, r, ci);
        if n > 0 {
            cur = next;
            total += n;
        }
        if cur.is_empty() {
            break;
        }
    }
    (cur, total)
}

/// Resolves a PHP substring `offset` and optional `length` against a string
/// of `len` bytes, as `substr()` does: negative values count from the end
/// and out-of-range values clamp. Returns the byte range.
pub fn substr_range(len: usize, offset: i64, length: Option<i64>) -> std::ops::Range<usize> {
    let len_i = len as i64;
    let start = if offset < 0 {
        (len_i + offset).max(0)
    } else {
        offset.min(len_i)
    };
    let end = match length {
        None => len_i,
        Some(l) if l < 0 => (len_i + l).max(start),
        Some(l) => start.saturating_add(l).min(len_i),
    };
    start as usize..end.max(start) as usize
}

/// `substr()`.
pub fn substr(s: &[u8], offset: i64, length: Option<i64>) -> Vec<u8> {
    s[substr_range(s.len(), offset, length)].to_vec()
}

/// `substr_replace()` on one string.
pub fn substr_replace(s: &[u8], replacement: &[u8], offset: i64, length: Option<i64>) -> Vec<u8> {
    let r = substr_range(s.len(), offset, length);
    let mut out = Vec::with_capacity(s.len() + replacement.len());
    out.extend_from_slice(&s[..r.start]);
    out.extend_from_slice(replacement);
    out.extend_from_slice(&s[r.end..]);
    out
}

/// `substr_count()`.
pub fn substr_count(h: &[u8], n: &[u8], offset: i64, length: Option<i64>) -> Result<i64, StdError> {
    if n.is_empty() {
        return Err(value_error(
            "substr_count(): Argument #2 ($needle) must not be empty",
        ));
    }
    let len = h.len() as i64;
    let start = if offset < 0 { offset + len } else { offset };
    if start < 0 || start > len {
        return Err(value_error(
            "substr_count(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        ));
    }
    let end = match length {
        None => len,
        Some(l) => {
            let e = if l < 0 { len + l } else { start + l };
            if e < start || e > len {
                return Err(value_error(
                    "substr_count(): Argument #4 ($length) must be contained in argument #1 ($haystack)",
                ));
            }
            e
        }
    };
    let hay = &h[start as usize..end as usize];
    let mut count = 0;
    let mut pos = 0;
    while let Some(p) = find(hay, n, pos, false) {
        count += 1;
        pos = p + n.len();
    }
    Ok(count)
}

/// `str_contains()`.
pub fn str_contains(h: &[u8], n: &[u8]) -> bool {
    find(h, n, 0, false).is_some()
}

/// `str_starts_with()`.
pub fn str_starts_with(h: &[u8], n: &[u8]) -> bool {
    h.starts_with(n)
}

/// `str_ends_with()`.
pub fn str_ends_with(h: &[u8], n: &[u8]) -> bool {
    h.ends_with(n)
}

/// `strpbrk()`: the rest of the string from the first of `characters`.
pub fn strpbrk(s: &[u8], characters: &[u8]) -> Result<Option<Vec<u8>>, StdError> {
    if characters.is_empty() {
        return Err(value_error(
            "strpbrk(): Argument #2 ($characters) must be a non-empty string",
        ));
    }
    let mut mask = [false; 256];
    for &c in characters {
        mask[c as usize] = true;
    }
    Ok(s.iter()
        .position(|&c| mask[c as usize])
        .map(|p| s[p..].to_vec()))
}

fn span(s: &[u8], mask: &[u8], offset: i64, length: Option<i64>, accept: bool) -> i64 {
    let mut m = [false; 256];
    for &c in mask {
        m[c as usize] = true;
    }
    let r = substr_range(s.len(), offset, length);
    s[r].iter()
        .take_while(|&&c| m[c as usize] == accept)
        .count() as i64
}

/// `strspn()`.
pub fn strspn(s: &[u8], mask: &[u8], offset: i64, length: Option<i64>) -> i64 {
    span(s, mask, offset, length, true)
}

/// `strcspn()`.
pub fn strcspn(s: &[u8], mask: &[u8], offset: i64, length: Option<i64>) -> i64 {
    span(s, mask, offset, length, false)
}

/// `explode()`.
pub fn explode(sep: &[u8], s: &[u8], limit: i64) -> Result<Vec<Vec<u8>>, StdError> {
    if sep.is_empty() {
        return Err(value_error(
            "explode(): Argument #1 ($separator) must not be empty",
        ));
    }
    let mut parts = Vec::new();
    let mut pos = 0;
    let max = match limit {
        0 => 1,
        l if l > 0 => l as usize,
        _ => usize::MAX,
    };
    while parts.len() + 1 < max {
        match find(s, sep, pos, false) {
            Some(p) => {
                parts.push(s[pos..p].to_vec());
                pos = p + sep.len();
            }
            None => break,
        }
    }
    parts.push(s[pos..].to_vec());
    if limit < 0 {
        let drop = limit.unsigned_abs().min(parts.len() as u64) as usize;
        parts.truncate(parts.len() - drop);
    }
    Ok(parts)
}

/// `implode()` of strings already converted.
pub fn implode<T: AsRef<[u8]>>(sep: &[u8], parts: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(sep);
        }
        out.extend_from_slice(p.as_ref());
    }
    out
}

/// `str_split()`.
pub fn str_split(s: &[u8], length: i64) -> Result<Vec<Vec<u8>>, StdError> {
    if length < 1 {
        return Err(value_error(
            "str_split(): Argument #2 ($length) must be greater than 0",
        ));
    }
    Ok(s.chunks(length as usize).map(|c| c.to_vec()).collect())
}

/// `str_repeat()`.
pub fn str_repeat(s: &[u8], times: i64) -> Result<Vec<u8>, StdError> {
    if times < 0 {
        return Err(value_error(
            "str_repeat(): Argument #2 ($times) must be greater than or equal to 0",
        ));
    }
    Ok(s.repeat(times as usize))
}

/// Which `strpos()` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosKind {
    /// `strpos()`.
    Pos,
    /// `stripos()`.
    IPos,
    /// `strrpos()`.
    RPos,
    /// `strripos()`.
    RIPos,
}

/// `strpos()`, `stripos()`, `strrpos()` and `strripos()`.
pub fn strpos(h: &[u8], n: &[u8], offset: i64, kind: PosKind) -> Result<Option<i64>, StdError> {
    let len = h.len() as i64;
    let name = match kind {
        PosKind::Pos => "strpos",
        PosKind::IPos => "stripos",
        PosKind::RPos => "strrpos",
        PosKind::RIPos => "strripos",
    };
    if offset > len || offset < -len {
        return Err(StdError::ValueError(format!(
            "{name}(): Argument #3 ($offset) must be contained in argument #1 ($haystack)"
        )));
    }
    let ci = matches!(kind, PosKind::IPos | PosKind::RIPos);
    match kind {
        PosKind::Pos | PosKind::IPos => {
            let from = if offset < 0 { len + offset } else { offset } as usize;
            Ok(find(h, n, from, ci).map(|p| p as i64))
        }
        _ => {
            // Positive offsets bound the start of the match from below;
            // negative ones bound where the match may start from above
            // (PHP manual: strrpos).
            let (lo, hi) = if offset >= 0 {
                (offset as usize, h.len())
            } else {
                let last_start = len + offset;
                (0, (last_start as usize + n.len()).min(h.len()))
            };
            if lo > hi {
                return Ok(None);
            }
            Ok(rfind(&h[lo..hi], n, ci).map(|p| (p + lo) as i64))
        }
    }
}

/// `strstr()` and `stristr()`.
pub fn strstr(h: &[u8], n: &[u8], before: bool, ci: bool) -> Option<Vec<u8>> {
    let p = find(h, n, 0, ci)?;
    Some(if before {
        h[..p].to_vec()
    } else {
        h[p..].to_vec()
    })
}

/// `strrchr()`: from the last occurrence of the needle's first byte (NUL
/// for an empty needle).
pub fn strrchr(h: &[u8], n: &[u8], before: bool) -> Option<Vec<u8>> {
    let c = n.first().copied().unwrap_or(0);
    let p = h.iter().rposition(|&x| x == c)?;
    Some(if before {
        h[..p].to_vec()
    } else {
        h[p..].to_vec()
    })
}

fn sign(o: std::cmp::Ordering) -> i64 {
    match o {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn byte_cmp(a: &[u8], b: &[u8], ci: bool) -> i64 {
    let fold = |c: u8| if ci { c.to_ascii_lowercase() } else { c };
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (fold(x), fold(y));
        if x != y {
            return x as i64 - y as i64;
        }
    }
    sign(a.len().cmp(&b.len()))
}

/// `strcmp()`: the difference of the first differing bytes, or -1/1 when
/// one string is a prefix of the other (observed on PHP 8.5).
pub fn strcmp(a: &[u8], b: &[u8]) -> i64 {
    byte_cmp(a, b, false)
}

/// `strcasecmp()`: as [`strcmp`] on ASCII-lowercased bytes.
pub fn strcasecmp(a: &[u8], b: &[u8]) -> i64 {
    byte_cmp(a, b, true)
}

/// `strncmp()` and `strncasecmp()`.
pub fn strncmp(a: &[u8], b: &[u8], length: i64, ci: bool) -> Result<i64, StdError> {
    if length < 0 {
        return Err(StdError::ValueError(format!(
            "{}(): Argument #3 ($length) must be greater than or equal to 0",
            if ci { "strncasecmp" } else { "strncmp" }
        )));
    }
    let n = length as usize;
    let a = &a[..a.len().min(n)];
    let b = &b[..b.len().min(n)];
    Ok(if ci { strcasecmp(a, b) } else { strcmp(a, b) })
}

/// `substr_compare()`.
pub fn substr_compare(
    h: &[u8],
    n: &[u8],
    offset: i64,
    length: Option<i64>,
    ci: bool,
) -> Result<i64, StdError> {
    if let Some(l) = length
        && l < 0
    {
        return Err(value_error(
            "substr_compare(): Argument #4 ($length) must be greater than or equal to 0",
        ));
    }
    let len = h.len() as i64;
    let start = if offset < 0 {
        (len + offset).max(0)
    } else {
        offset
    };
    if start > len {
        return Err(value_error(
            "substr_compare(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        ));
    }
    let rest = &h[start as usize..];
    match length {
        Some(l) => strncmp(rest, n, l, ci),
        None => Ok(if ci {
            strcasecmp(rest, n)
        } else {
            strcmp(rest, n)
        }),
    }
}

/// `count_chars()` byte counts.
pub fn count_chars(s: &[u8]) -> [u64; 256] {
    let mut c = [0u64; 256];
    for &b in s {
        c[b as usize] += 1;
    }
    c
}

/// `count_chars()` modes 3 and 4: the bytes used (or unused), in order.
pub fn count_chars_string(s: &[u8], mode: i64) -> Result<Vec<u8>, StdError> {
    if !(0..=4).contains(&mode) {
        return Err(value_error(
            "count_chars(): Argument #2 ($mode) must be between 0 and 4 (inclusive)",
        ));
    }
    let c = count_chars(s);
    Ok((0..=255u8)
        .filter(|&b| (c[b as usize] > 0) == (mode == 3))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(str_pad(b"x", 5, b"ab", STR_PAD_BOTH).unwrap(), b"abxab");
        assert_eq!(nl2br(b"a\r\nb", true), b"a<br />\r\nb");
        assert_eq!(ucwords(b"hello_world-foo", b"_-"), b"Hello_World-Foo");
        let mut d = Vec::new();
        assert_eq!(trim(b"abc", Some(b"a.c"), TrimMode::Both, &mut d), b"b");
        assert_eq!(explode(b",", b"a,b,c", -1).unwrap().len(), 2);
        assert_eq!(
            strtr_pairs(b"Hi all", &[(b"Hi".to_vec(), b"Hello".to_vec())], &mut d),
            b"Hello all"
        );
    }
}
