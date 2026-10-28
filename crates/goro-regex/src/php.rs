//! PHP-level helpers for ext/pcre: pattern delimiters and modifiers, the
//! global matching loop used by preg_match_all(), preg_replace() and
//! preg_split(), and replacement-string expansion.
//!
//! Behaviour from the PHP manual (PCRE "Pattern Modifiers", "Delimiters",
//! preg_replace()) and observed on PHP 8.5.

use crate::{
    Captures, CompileConfig, Flags, Limits, MatchError, MatchOptions, Regex, Scratch, utf8,
};

/// Why a PHP pattern string could not be split into regex and modifiers.
/// These are PHP warnings (`preg_match(): <message>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    Empty,
    BadDelimiter,
    NoEndingDelimiter(u8),
    NoEndingMatchingDelimiter(u8),
    UnknownModifier(u8),
    NulModifier,
}

impl PatternError {
    pub fn message(&self) -> String {
        match self {
            PatternError::Empty => "Empty regular expression".to_string(),
            PatternError::BadDelimiter => {
                "Delimiter must not be alphanumeric, backslash, or NUL byte".to_string()
            }
            PatternError::NoEndingDelimiter(c) => {
                format!("No ending delimiter '{}' found", *c as char)
            }
            PatternError::NoEndingMatchingDelimiter(c) => {
                format!("No ending matching delimiter '{}' found", *c as char)
            }
            PatternError::UnknownModifier(c) => format!("Unknown modifier '{}'", *c as char),
            PatternError::NulModifier => "NUL byte is not a valid modifier".to_string(),
        }
    }
}

/// A PHP pattern split into its parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhpPattern<'a> {
    pub regex: &'a [u8],
    pub flags: Flags,
    /// The `S` modifier (ignored by PHP 8.5 but accepted).
    pub study: bool,
}

/// Splits `/regex/flags` (PHP manual "Delimiters" and "Pattern Modifiers").
pub fn split_pattern(p: &[u8]) -> Result<PhpPattern<'_>, PatternError> {
    let mut i = 0;
    while i < p.len() && p[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= p.len() {
        return Err(PatternError::Empty);
    }
    let d = p[i];
    if d.is_ascii_alphanumeric() || d == b'\\' || d == 0 {
        return Err(PatternError::BadDelimiter);
    }
    i += 1;
    let start = i;
    let close = match d {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        _ => d,
    };
    let end;
    if close == d {
        loop {
            if i >= p.len() {
                return Err(PatternError::NoEndingDelimiter(d));
            }
            if p[i] == b'\\' && i + 1 < p.len() {
                i += 2;
                continue;
            }
            if p[i] == d {
                end = i;
                break;
            }
            i += 1;
        }
    } else {
        let mut depth = 1;
        loop {
            if i >= p.len() {
                return Err(PatternError::NoEndingMatchingDelimiter(close));
            }
            let b = p[i];
            if b == b'\\' && i + 1 < p.len() {
                i += 2;
                continue;
            }
            if b == close {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            } else if b == d {
                depth += 1;
            }
            i += 1;
        }
    }
    let mut flags = Flags::empty();
    let mut study = false;
    for &m in &p[end + 1..] {
        match m {
            b'i' => flags |= Flags::CASELESS,
            b'm' => flags |= Flags::MULTILINE,
            b's' => flags |= Flags::DOTALL,
            b'x' => flags |= Flags::EXTENDED,
            b'n' => flags |= Flags::NO_AUTO_CAPTURE,
            b'A' => flags |= Flags::ANCHORED,
            b'D' => flags |= Flags::DOLLAR_ENDONLY,
            b'S' => study = true,
            b'U' => flags |= Flags::UNGREEDY,
            b'X' => {}
            b'J' => flags |= Flags::DUPNAMES,
            // The u modifier turns on UTF and UCP (observed on PHP 8.5:
            // /\w/u matches "é", (*UTF)\w does not).
            b'u' => flags |= Flags::UTF | Flags::UCP,
            b'r' => flags |= Flags::CASELESS_RESTRICT,
            b' ' | b'\n' | b'\r' => {}
            0 => return Err(PatternError::NulModifier),
            other => return Err(PatternError::UnknownModifier(other)),
        }
    }
    Ok(PhpPattern {
        regex: &p[start..end],
        flags,
        study,
    })
}

/// Compiles a PHP pattern string.
pub enum PhpCompileError {
    Pattern(PatternError),
    Compile(crate::CompileError),
}

impl PhpCompileError {
    /// The warning text PHP prints after "preg_match(): ".
    pub fn message(&self) -> String {
        match self {
            PhpCompileError::Pattern(p) => p.message(),
            PhpCompileError::Compile(c) => {
                format!("Compilation failed: {} at offset {}", c.message(), c.offset)
            }
        }
    }
}

pub fn compile(p: &[u8]) -> Result<Regex, PhpCompileError> {
    let pp = split_pattern(p).map_err(PhpCompileError::Pattern)?;
    Regex::with_config(
        pp.regex,
        &CompileConfig {
            flags: pp.flags,
            ..CompileConfig::default()
        },
    )
    .map_err(PhpCompileError::Compile)
}

/// PHP's PREG_*_ERROR codes for a match error.
pub fn preg_error_code(e: &MatchError) -> i32 {
    match e {
        MatchError::MatchLimit => 2,
        MatchError::DepthLimit => 3,
        MatchError::BadUtf8 { .. } => 4,
        MatchError::BadUtf8Offset => 5,
        MatchError::JitStackLimit => 6,
        _ => 1,
    }
}

/// The global matching loop PHP uses (preg_match_all, preg_replace,
/// preg_split): after an empty match, the next attempt at the same place
/// requires a non-empty anchored match; if that fails the position moves on
/// by one character.
pub struct GlobalMatcher<'r, 's> {
    re: &'r Regex,
    subject: &'s [u8],
    pos: usize,
    retry_nonempty: bool,
    done: bool,
    pub scratch: Scratch,
    pub caps: Captures,
    pub limits: Limits,
    searcher: Option<crate::exec::Searcher<'r, 's>>,
}

impl<'r, 's> GlobalMatcher<'r, 's> {
    pub fn new(
        re: &'r Regex,
        subject: &'s [u8],
        start: usize,
        limits: Limits,
    ) -> GlobalMatcher<'r, 's> {
        GlobalMatcher {
            re,
            subject,
            pos: start,
            retry_nonempty: false,
            done: false,
            scratch: Scratch::new(),
            caps: Captures::default(),
            limits,
            searcher: None,
        }
    }

    /// Finds the next match; on success the captures are in `self.caps`.
    #[inline]
    pub fn next_match(&mut self) -> Result<bool, MatchError> {
        if self.done {
            return Ok(false);
        }
        loop {
            let opts = MatchOptions {
                notempty_atstart: self.retry_nonempty,
                anchored: self.retry_nonempty,
                no_utf_check: self.searcher.is_some(),
                ..MatchOptions::default()
            };
            let (re, subject, limits) = (self.re, self.subject, self.limits);
            let searcher = self
                .searcher
                .get_or_insert_with(|| crate::exec::Searcher::new(re.prog(), subject, limits));
            let r = searcher.find(&mut self.scratch, self.pos, opts, &mut self.caps.slots);
            let found = match r {
                Ok(Some(m)) => {
                    self.caps.mark = m.mark;
                    true
                }
                Ok(None) => false,
                Err(e) => {
                    self.done = true;
                    return Err(e);
                }
            };
            if found {
                let (s, e) = self.caps.get(0).unwrap();
                self.pos = e;
                self.retry_nonempty = s == e;
                return Ok(true);
            }
            if !self.retry_nonempty || self.pos >= self.subject.len() {
                self.done = true;
                return Ok(false);
            }
            // Advance one character and try again normally.
            let step = if self.re.is_utf() {
                utf8::len_from_lead(self.subject[self.pos])
            } else {
                1
            };
            self.pos = (self.pos + step).min(self.subject.len());
            self.retry_nonempty = false;
        }
    }
}

/// Expands a preg_replace() replacement string: `\n`, `$n` and `${n}` with
/// n from 0 to 99 insert group n (empty if unset or nonexistent); `\\`
/// inserts a backslash (PHP manual preg_replace(); observed on PHP 8.5).
pub fn expand_replacement(repl: &[u8], subject: &[u8], caps: &Captures, out: &mut Vec<u8>) {
    let n = repl.len();
    let mut i = 0;
    let group = |g: usize, out: &mut Vec<u8>| {
        if let Some((s, e)) = caps.get(g)
            && s <= e
        {
            out.extend_from_slice(&subject[s..e]);
        }
    };
    while i < n {
        let c = repl[i];
        if (c == b'\\' || c == b'$') && i + 1 < n {
            if c == b'\\' && (repl[i + 1] == b'\\' || repl[i + 1] == b'$') {
                out.push(repl[i + 1]);
                i += 2;
                continue;
            }
            // \n, $n: one or two digits.
            if repl[i + 1].is_ascii_digit() {
                let mut g = (repl[i + 1] - b'0') as usize;
                let mut j = i + 2;
                if j < n && repl[j].is_ascii_digit() {
                    g = g * 10 + (repl[j] - b'0') as usize;
                    j += 1;
                }
                group(g, out);
                i = j;
                continue;
            }
            if c == b'$' && repl[i + 1] == b'{' {
                // ${n} or ${nn}
                let mut j = i + 2;
                let ds = j;
                while j < n && j - ds < 2 && repl[j].is_ascii_digit() {
                    j += 1;
                }
                if j > ds && j < n && repl[j] == b'}' {
                    let g: usize = std::str::from_utf8(&repl[ds..j]).unwrap().parse().unwrap();
                    group(g, out);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiters() {
        assert_eq!(split_pattern(b"/a/i").unwrap().regex, b"a");
        assert_eq!(split_pattern(b"  (a(b)c)").unwrap().regex, b"a(b)c");
        assert_eq!(split_pattern(b"{a{1}}").unwrap().regex, b"a{1}");
        assert_eq!(split_pattern(br"/a\/b/").unwrap().regex, br"a\/b");
        assert_eq!(split_pattern(b""), Err(PatternError::Empty));
        assert_eq!(split_pattern(b"a"), Err(PatternError::BadDelimiter));
        assert_eq!(
            split_pattern(b"/a"),
            Err(PatternError::NoEndingDelimiter(b'/'))
        );
        assert_eq!(
            split_pattern(b"(a"),
            Err(PatternError::NoEndingMatchingDelimiter(b')'))
        );
        assert_eq!(
            split_pattern(b"/a/e"),
            Err(PatternError::UnknownModifier(b'e'))
        );
        assert_eq!(split_pattern(b"/a/\0"), Err(PatternError::NulModifier));
        assert!(split_pattern(b"/a/ i\n").is_ok());
    }

    fn replace(pat: &[u8], repl: &[u8], s: &[u8]) -> Vec<u8> {
        let re = compile(pat).ok().unwrap();
        let mut g = GlobalMatcher::new(&re, s, 0, Limits::default());
        let mut out = Vec::new();
        let mut last = 0;
        while g.next_match().unwrap() {
            let (ms, me) = g.caps.get(0).unwrap();
            out.extend_from_slice(&s[last..ms]);
            expand_replacement(repl, s, &g.caps, &mut out);
            last = me;
        }
        out.extend_from_slice(&s[last..]);
        out
    }

    #[test]
    fn replacement() {
        // Observed on PHP 8.5.
        assert_eq!(replace(b"/a/", br"\\x", b"a"), br"\x");
        assert_eq!(replace(b"/(a)/", br"\1\\1", b"a"), br"a\1");
        assert_eq!(
            replace(
                b"/(a)/",
                b"[$1][${1}][\\1][$01][${01}][$2][\\2][$10][${10}]",
                b"a"
            ),
            b"[a][a][a][a][a][][][][]"
        );
        assert_eq!(
            replace(
                b"/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)/",
                b"[$11][${11}][\\11][$111][$1a][\\1a][$]",
                b"abcdefghijk"
            ),
            b"[k][k][k][k1][aa][aa][$]"
        );
        assert_eq!(
            replace(b"/(a)/", b"$\\1 \\$1 $$1 \\\\$1 \\", b"a"),
            b"$a $1 $a \\a \\"
        );
        assert_eq!(replace(b"/(a)/", b"${1 ${a} ${", b"a"), b"${1 ${a} ${");
    }

    #[test]
    fn global_empty_matches() {
        // preg_match_all('/a*?/', 'baa') gives "", "", "a", "", "a", "".
        let re = compile(b"/a*?/").ok().unwrap();
        let mut g = GlobalMatcher::new(&re, b"baa", 0, Limits::default());
        let mut v = Vec::new();
        while g.next_match().unwrap() {
            v.push(g.caps.get(0).unwrap());
        }
        assert_eq!(v, vec![(0, 0), (1, 1), (1, 2), (2, 2), (2, 3), (3, 3)]);
    }
}
