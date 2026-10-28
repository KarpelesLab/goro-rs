//! Comparison of a test's output with its EXPECT, EXPECTF or EXPECTREGEX
//! section.
//!
//! Sources:
//! - The "PHPT Test File Layout" documentation lists the EXPECTF placeholders:
//!   `%e` directory separator, `%s` one or more characters up to the end of
//!   the line, `%S` zero or more of those, `%a` one or more characters
//!   including line ends, `%A` zero or more of those, `%w` zero or more white
//!   space characters, `%i` a signed integer, `%d` an unsigned integer, `%x`
//!   one or more hexadecimal digits, `%f` a floating point number, `%c` a
//!   single character, and `%r...%r` a regular expression.
//! - `%0` is not in the documentation, but the corpus uses it for a NUL byte
//!   (Zend/tests/gh13142.phpt prints `"test\0test"` and expects
//!   `$test%0test`).
//! - Any other `%` is a literal: ext/standard/tests/strings/printf.phpt
//!   expects the literal output `'%%%.2f'`.
//! - The PHP Internals Book (tests/phpt_file_structure): the runner trims
//!   trailing new lines of the expected and the actual output. The corpus
//!   shows leading white space is trimmed too:
//!   Zend/tests/foreach/foreach_by_ref_repacking_insert.phpt prints a blank
//!   line before its first expected line. Both sides are trimmed with PHP's
//!   trim() character set.
//! - CRLF is normalized to LF on both sides: ext/calendar/tests/
//!   easter_date_64bit.phpt is stored with CRLF line ends while the script
//!   prints "\n".
//! - EXPECTREGEX must match the whole output, and `.` crosses line ends
//!   (tests/run-test/test006.phpt expects `.*Division by zero.*` of a
//!   multi-line fatal error).

use crate::section::trim_ws;

/// Which kind of expectation a test has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Exact,
    Format,
    Regex,
}

/// Normalizes output or expected text for comparison: CRLF becomes LF and
/// the result is trimmed.
pub fn normalize(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    let t = trim_ws(&out);
    if t.len() == out.len() {
        out
    } else {
        t.to_vec()
    }
}

/// A compiled expectation.
pub enum Matcher {
    Exact(Vec<u8>),
    Bytes(regex::bytes::Regex),
    /// Fallback for PCRE features the `regex` crate lacks (look-around,
    /// back-references), matched against the output decoded as UTF-8.
    Fancy(Box<fancy_regex::Regex>),
}

impl Matcher {
    /// Compiles an expectation. `expected` must already be normalized.
    pub fn new(kind: Kind, expected: &[u8]) -> Result<Matcher, String> {
        match kind {
            Kind::Exact => Ok(Matcher::Exact(expected.to_vec())),
            Kind::Format => {
                if !expected.contains(&b'%') {
                    return Ok(Matcher::Exact(expected.to_vec()));
                }
                compile(&expectf_to_regex(expected, Flavor::Bytes), || {
                    expectf_to_regex(expected, Flavor::Str)
                })
            }
            Kind::Regex => compile(&regex_body(expected, Flavor::Bytes), || {
                regex_body(expected, Flavor::Str)
            }),
        }
    }

    /// Whether the (normalized) output satisfies the expectation.
    pub fn is_match(&self, output: &[u8]) -> bool {
        match self {
            Matcher::Exact(e) => e == output,
            Matcher::Bytes(r) => r.is_match(output),
            Matcher::Fancy(r) => r
                .is_match(String::from_utf8_lossy(output).as_ref())
                .unwrap_or(false),
        }
    }
}

fn compile(pattern: &str, str_pattern: impl FnOnce() -> String) -> Result<Matcher, String> {
    let anchored = format!(r"\A(?:{pattern})\z");
    match regex::bytes::RegexBuilder::new(&anchored)
        .unicode(false)
        .dot_matches_new_line(true)
        .octal(true)
        .size_limit(1 << 29)
        .dfa_size_limit(1 << 26)
        .build()
    {
        Ok(r) => Ok(Matcher::Bytes(r)),
        Err(e1) => {
            let anchored = format!(r"(?s)\A(?:{})\z", str_pattern());
            match fancy_regex::RegexBuilder::new(&anchored)
                .backtrack_limit(10_000_000)
                .build()
            {
                Ok(r) => Ok(Matcher::Fancy(Box::new(r))),
                Err(e2) => Err(format!("bad expectation regex: {e1}; {e2}")),
            }
        }
    }
}

/// How literal bytes are written into a pattern.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor {
    /// For `regex::bytes` with Unicode off: any non-ASCII byte as `\xHH`.
    Bytes,
    /// For a UTF-8 regex: text is decoded lossily first.
    Str,
}

fn push_literal(out: &mut String, lit: &[u8], flavor: Flavor) {
    match flavor {
        Flavor::Bytes => {
            for &b in lit {
                if b.is_ascii_alphanumeric() || b == b' ' || b == b'\n' || b == b'_' {
                    out.push(b as char);
                } else if b.is_ascii_graphic() {
                    out.push_str(&regex::escape(&(b as char).to_string()));
                } else {
                    out.push_str(&format!(r"\x{b:02X}"));
                }
            }
        }
        Flavor::Str => out.push_str(&regex::escape(&String::from_utf8_lossy(lit))),
    }
}

/// Copies a user regex (EXPECTREGEX or a `%r...%r` body). In the bytes flavor
/// raw non-ASCII bytes become `\xHH` escapes, which match those bytes.
fn push_regex(out: &mut String, re: &[u8], flavor: Flavor) {
    let re = pcre_compat(re);
    match flavor {
        Flavor::Bytes => {
            for &b in &re {
                if b.is_ascii() && b != 0 {
                    out.push(b as char);
                } else {
                    out.push_str(&format!(r"\x{b:02X}"));
                }
            }
        }
        Flavor::Str => out.push_str(&String::from_utf8_lossy(&re)),
    }
}

/// Length of a `{n}`, `{n,}` or `{n,m}` quantifier at the start of `s`.
fn quantifier_len(s: &[u8]) -> Option<usize> {
    let digits = |i: &mut usize| {
        let st = *i;
        while *i < s.len() && s[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > st
    };
    let mut i = 1;
    if !digits(&mut i) {
        return None;
    }
    if s.get(i) == Some(&b',') {
        i += 1;
        digits(&mut i);
    }
    (s.get(i) == Some(&b'}')).then_some(i + 1)
}

/// Rewrites the PCRE syntax the `regex` crate reads differently. PCRE takes a
/// `{` that does not start a quantifier as a literal (ext/standard/tests/
/// array/array_rand_variation4.phpt expects `array\(2\) {` in a regex), and
/// inside a character class `[` (other than `[:name:]`), `&&`, `--` and `~~`
/// are literal characters.
fn pcre_compat(re: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(re.len() + 8);
    let mut i = 0;
    while i < re.len() {
        let b = re[i];
        match b {
            b'\\' => {
                out.extend_from_slice(&re[i..(i + 2).min(re.len())]);
                i += 2;
            }
            b'{' => match quantifier_len(&re[i..]) {
                Some(n) => {
                    out.extend_from_slice(&re[i..i + n]);
                    i += n;
                }
                None => {
                    out.extend_from_slice(br"\{");
                    i += 1;
                }
            },
            b'}' => {
                out.extend_from_slice(br"\}");
                i += 1;
            }
            b'[' => {
                out.push(b'[');
                i += 1;
                if re.get(i) == Some(&b'^') {
                    out.push(b'^');
                    i += 1;
                }
                if re.get(i) == Some(&b']') {
                    out.extend_from_slice(br"\]");
                    i += 1;
                }
                while i < re.len() && re[i] != b']' {
                    let c = re[i];
                    if c == b'\\' {
                        out.extend_from_slice(&re[i..(i + 2).min(re.len())]);
                        i += 2;
                    } else if c == b'[' && re.get(i + 1) == Some(&b':') {
                        let end = re[i..]
                            .windows(2)
                            .position(|w| w == b":]")
                            .map_or(re.len(), |p| i + p + 2);
                        out.extend_from_slice(&re[i..end]);
                        i = end;
                    } else if matches!(c, b'[' | b'&' | b'~') || (c == b'-' && re.get(i + 1) == Some(&b'-')) {
                        out.push(b'\\');
                        out.push(c);
                        i += 1;
                    } else {
                        out.push(c);
                        i += 1;
                    }
                }
                if i < re.len() {
                    out.push(b']');
                    i += 1;
                }
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn regex_body(expected: &[u8], flavor: Flavor) -> String {
    let mut s = String::with_capacity(expected.len());
    push_regex(&mut s, expected, flavor);
    s
}

/// The regex for one EXPECTF placeholder letter, if it is one.
fn placeholder(c: u8) -> Option<&'static str> {
    Some(match c {
        b'e' => "/",
        b's' => r"[^\r\n]+",
        b'S' => r"[^\r\n]*",
        b'a' => r"(?s:.+)",
        b'A' => r"(?s:.*)",
        b'w' => r"\s*",
        b'i' => r"[+-]?[0-9]+",
        b'd' => r"[0-9]+",
        b'x' => r"[0-9a-fA-F]+",
        b'f' => r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?",
        b'c' => r"(?s:.)",
        b'0' => r"\x00",
        _ => return None,
    })
}

fn expectf_to_regex(expected: &[u8], flavor: Flavor) -> String {
    let mut out = String::with_capacity(expected.len() * 2);
    let mut lit_start = 0;
    let mut i = 0;
    while i < expected.len() {
        if expected[i] != b'%' || i + 1 >= expected.len() {
            i += 1;
            continue;
        }
        let c = expected[i + 1];
        if c == b'r' {
            // %r...%r: find the closing marker.
            if let Some(end) = find(&expected[i + 2..], b"%r") {
                push_literal(&mut out, &expected[lit_start..i], flavor);
                out.push_str("(?:");
                push_regex(&mut out, &expected[i + 2..i + 2 + end], flavor);
                out.push(')');
                i = i + 2 + end + 2;
                lit_start = i;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(re) = placeholder(c) {
            push_literal(&mut out, &expected[lit_start..i], flavor);
            out.push_str(re);
            i += 2;
            lit_start = i;
            continue;
        }
        // Not a placeholder: the % is literal; rescan from the next byte.
        i += 1;
    }
    push_literal(&mut out, &expected[lit_start..], flavor);
    out
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Whether a single expected line matches a single output line. Used only to
/// make failure diffs readable.
pub fn line_matches(kind: Kind, expected: &[u8], actual: &[u8]) -> bool {
    if expected == actual {
        return true;
    }
    match kind {
        Kind::Exact => false,
        Kind::Format | Kind::Regex => {
            if kind == Kind::Format && !expected.contains(&b'%') {
                return false;
            }
            Matcher::new(kind, expected).is_ok_and(|m| m.is_match(actual))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmatch(pat: &str, out: &str) -> bool {
        Matcher::new(Kind::Format, &normalize(pat.as_bytes()))
            .unwrap()
            .is_match(&normalize(out.as_bytes()))
    }

    #[test]
    fn documented_placeholders() {
        // tests/run-test/test002.phpt
        assert!(fmatch(
            "%d\n%i\n%i\n%f\n%s\n%x\n%c\n",
            "123\n-123\n+123\n+1.1\nabc\n0abc\nx\n"
        ));
        assert!(fmatch("a%eb", "a/b"));
        assert!(!fmatch("a%sb", "ab"));
        assert!(fmatch("a%Sb", "ab"));
        assert!(!fmatch("a%sb", "a\nb"));
        assert!(fmatch("a%ab", "a\nxx\nb"));
        assert!(!fmatch("a%ab", "ab"));
        assert!(fmatch("a%Ab", "ab"));
        assert!(fmatch("a%wb", "a \t b"));
        assert!(fmatch("a%wb", "ab"));
        assert!(!fmatch("%d", "-1"));
        assert!(fmatch("%i", "-1"));
        assert!(fmatch("%x", "DeadBeef"));
        assert!(!fmatch("%x", "xyz"));
        for f in ["1", "1.5", "-1.5", ".5", "1.", "1.5E+20", "-3e-7", "+2"] {
            assert!(fmatch("%f", f), "{f}");
        }
        assert!(!fmatch("%f", "abc"));
        assert!(fmatch("%c%c", "ab"));
        assert!(!fmatch("%c", "ab"));
    }

    #[test]
    fn regex_placeholder() {
        // ext/standard/tests/strings/printf.phpt: `[2] => %r%%r05s`
        assert!(fmatch("[2] => %r%%r05s", "[2] => %05s"));
        assert!(fmatch("x %r(foo|bar)+%r y", "x foobar y"));
        assert!(!fmatch("x %r(foo|bar)+%r y", "x baz y"));
        // Regex metacharacters outside %r are literal.
        assert!(fmatch("a.b(c)*[d]", "a.b(c)*[d]"));
        assert!(!fmatch("a.b%d", "aXb1"));
    }

    #[test]
    fn nul_and_literal_percent() {
        // Zend/tests/gh13142.phpt
        assert!(fmatch(
            "Warning: Undefined variable $test%0test in %s on line %d",
            "Warning: Undefined variable $test\0test in /x/gh13142.php on line 4"
        ));
        // ext/standard/tests/strings/printf.phpt
        assert!(fmatch("'%%%.2f'", "'%%%.2f'"));
        assert!(fmatch("100%", "100%"));
        assert!(fmatch("%z%", "%z%"));
    }

    #[test]
    fn typical_warning() {
        let pat = "\nWarning: Undefined variable $x in %s on line %d\nNULL\n";
        let out = "\nWarning: Undefined variable $x in /path/to/t.php on line 3\nNULL\n";
        assert!(fmatch(pat, out));
        assert!(!fmatch(pat, "Warning: Undefined variable $y in /p on line 3\nNULL"));
    }

    #[test]
    fn binary_and_utf8() {
        assert!(fmatch("caf\u{e9} %s", "caf\u{e9} ok"));
        let m = Matcher::new(Kind::Format, b"\xff\xfe %d").unwrap();
        assert!(m.is_match(b"\xff\xfe 12"));
        assert!(!m.is_match(b"\xff\xff 12"));
    }

    #[test]
    fn trimming_and_crlf() {
        assert_eq!(normalize(b"\n\nabc\r\ndef\r\n\n"), b"abc\ndef");
        assert_eq!(normalize(b"  \n"), b"");
        // Zend/tests/foreach/foreach_by_ref_repacking_insert.phpt
        let m = Matcher::new(Kind::Exact, &normalize(b"int(1)\n")).unwrap();
        assert!(m.is_match(&normalize(b"\nint(1)\n")));
    }

    #[test]
    fn expectregex() {
        // tests/run-test/test003.phpt and test006.phpt
        let m = Matcher::new(Kind::Regex, b"[abcde]+[0-5]*xyz[2-8]+abcde").unwrap();
        assert!(m.is_match(b"abcde12314235xyz34264768286abcde"));
        let m = Matcher::new(Kind::Regex, b".*Division by zero.*").unwrap();
        assert!(m.is_match(b"PHP Fatal error:  Uncaught\nDivision by zero in x\nStack"));
        // Anchored: a partial match is not enough.
        let m = Matcher::new(Kind::Regex, b"abc").unwrap();
        assert!(!m.is_match(b"xabcx"));
        // ext/date/tests/timezone_version_get_basic1.phpt
        let m = Matcher::new(Kind::Regex, b"^[12][0-9]{3}.[0-9]+$").unwrap();
        assert!(m.is_match(b"2025.2"));
        // \0 inside a class: ext/standard/tests/array/array_rand_variation4.phpt
        let m = Matcher::new(Kind::Regex, b"\"[a\\0]*\"").unwrap();
        assert!(m.is_match(b"\"a\0a\""));
    }

    #[test]
    fn pcre_literal_braces_and_classes() {
        // ext/standard/tests/array/array_rand_variation4.phpt
        let m = Matcher::new(Kind::Regex, b"array\\(2\\) {\n  \\[0\\]=>\n}").unwrap();
        assert!(m.is_match(b"array(2) {\n  [0]=>\n}"));
        let m = Matcher::new(Kind::Regex, b"a{2}b{1,}c{1,2}").unwrap();
        assert!(m.is_match(b"aabbbcc"));
        let m = Matcher::new(Kind::Regex, b"[[a]+[]x]*[^]]-[&&]-[[:digit:]]").unwrap();
        assert!(m.is_match(b"[a]x]z-&-7"));
    }

    #[test]
    fn lookahead_fallback() {
        // ext/zip/tests/bug76524.phpt uses a negative look-ahead.
        let m = Matcher::new(Kind::Format, b"ok%r((?!memory leaks detected).)*%r").unwrap();
        assert!(matches!(m, Matcher::Fancy(_)));
        assert!(m.is_match(b"ok fine"));
        assert!(!m.is_match(b"ok memory leaks detected"));
    }

    #[test]
    fn line_match_helper() {
        assert!(line_matches(Kind::Format, b"int(%d)", b"int(5)"));
        assert!(!line_matches(Kind::Exact, b"int(%d)", b"int(5)"));
    }
}
