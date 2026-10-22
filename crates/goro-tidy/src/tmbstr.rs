//! tmbstr.c: string helpers. Byte slices stand for NUL-terminated C
//! strings: reading past the end yields the terminator (0), and an
//! embedded 0 ends the string. Comparisons use `char` (signed) ordering
//! like the C code on x86.

#[inline]
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

#[inline]
fn lower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

#[inline]
fn signed_gt(a: u8, b: u8) -> bool {
    (a as i8) > (b as i8)
}

/// The C length of a string (stops at an embedded NUL).
pub fn tmbstrlen(s: &[u8]) -> usize {
    s.iter().position(|&c| c == 0).unwrap_or(s.len())
}

/// The string up to its first NUL.
pub fn cstr(s: &[u8]) -> &[u8] {
    &s[..tmbstrlen(s)]
}

/// strcmp
pub fn tmbstrcmp(s1: &[u8], s2: &[u8]) -> i32 {
    let mut i = 0;
    loop {
        let c = at(s1, i);
        if c != at(s2, i) {
            break;
        }
        if c == 0 {
            return 0;
        }
        i += 1;
    }
    if signed_gt(at(s1, i), at(s2, i)) {
        1
    } else {
        -1
    }
}

/// strcasecmp (ASCII folding)
pub fn tmbstrcasecmp(s1: &[u8], s2: &[u8]) -> i32 {
    let mut i = 0;
    loop {
        let c = at(s1, i);
        if lower(c) != lower(at(s2, i)) {
            break;
        }
        if c == 0 {
            return 0;
        }
        i += 1;
    }
    if signed_gt(at(s1, i), at(s2, i)) {
        1
    } else {
        -1
    }
}

/// strncmp
pub fn tmbstrncmp(s1: &[u8], s2: &[u8], n: usize) -> i32 {
    let mut i = 0;
    let mut n = n;
    loop {
        let c = at(s1, i);
        if c != at(s2, i) {
            break;
        }
        if c == 0 || n == 0 {
            return 0;
        }
        i += 1;
        n -= 1;
    }
    if n == 0 {
        return 0;
    }
    if signed_gt(at(s1, i), at(s2, i)) {
        1
    } else {
        -1
    }
}

/// strncasecmp
pub fn tmbstrncasecmp(s1: &[u8], s2: &[u8], n: usize) -> i32 {
    let mut i = 0;
    let mut n = n;
    loop {
        let c = at(s1, i);
        if lower(c) != lower(at(s2, i)) {
            break;
        }
        if c == 0 || n == 0 {
            return 0;
        }
        i += 1;
        n -= 1;
    }
    if n == 0 {
        return 0;
    }
    if signed_gt(at(s1, i), at(s2, i)) {
        1
    } else {
        -1
    }
}

/// tmbsubstrn: case-sensitive search of s2 in the first len1 bytes of s1;
/// returns the offset.
pub fn tmbsubstrn(s1: &[u8], len1: usize, s2: &[u8]) -> Option<usize> {
    let len2 = tmbstrlen(s2) as isize;
    let diff = len1 as isize - len2;
    let mut ix = 0isize;
    while ix <= diff {
        if tmbstrncmp(&s1[(ix as usize).min(s1.len())..], s2, len2 as usize) == 0 {
            return Some(ix as usize);
        }
        ix += 1;
    }
    None
}

/// tmbsubstr: case-insensitive search of s2 in s1; returns the offset.
pub fn tmbsubstr(s1: &[u8], s2: &[u8]) -> Option<usize> {
    let len1 = tmbstrlen(s1) as isize;
    let len2 = tmbstrlen(s2) as isize;
    let diff = len1 - len2;
    let mut ix = 0isize;
    while ix <= diff {
        if tmbstrncasecmp(&s1[ix as usize..], s2, len2 as usize) == 0 {
            return Some(ix as usize);
        }
        ix += 1;
    }
    None
}

/// tmbstrtolower (ASCII)
pub fn tmbstrtolower(s: &mut [u8]) {
    for c in s.iter_mut() {
        *c = c.to_ascii_lowercase();
    }
}

/// tmbstrtoupper (ASCII)
pub fn tmbstrtoupper(s: &mut [u8]) {
    for c in s.iter_mut() {
        *c = c.to_ascii_uppercase();
    }
}

/// tmbstrndup: at most `len` bytes, stopping at a NUL; None for len 0.
pub fn tmbstrndup(s: &[u8], len: usize) -> Option<Vec<u8>> {
    if len == 0 {
        return None;
    }
    let s = cstr(s);
    Some(s[..len.min(s.len())].to_vec())
}
