//! Parser for the .phpt file layout.
//!
//! Source: the "PHPT Test File Layout" documentation (qa.php.net/phpt_details.php,
//! now at php.github.io/php-src/miscellaneous/writing-tests.html). A file is a
//! sequence of sections, each introduced by a line `--NAME--`. The first line
//! must be `--TEST--`. Section bodies are kept as raw bytes: some tests embed
//! binary data and a few files use CRLF line endings.

use std::fmt;

/// Every section name the documentation describes, plus the ones that occur
/// in php-src 8.5's test corpus (found by scanning for `^--[A-Z_]+--$`).
/// A line `--NAME--` only starts a section when NAME is in this list, so a
/// test body may contain other `--WORD--` lines.
pub const KNOWN_SECTIONS: &[&str] = &[
    "TEST",
    "DESCRIPTION",
    "CREDITS",
    "SKIPIF",
    "CONFLICTS",
    "WHITESPACE_SENSITIVE",
    "CAPTURE_STDIO",
    "EXTENSIONS",
    "INI",
    "ENV",
    "ARGS",
    "STDIN",
    "FILE",
    "FILEEOF",
    "FILE_EXTERNAL",
    "REDIRECTTEST",
    "PHPDBG",
    "XFAIL",
    "XLEAK",
    "FLAKY",
    "CGI",
    "GET",
    "POST",
    "POST_RAW",
    "PUT",
    "GZIP_POST",
    "DEFLATE_POST",
    "COOKIE",
    "EXPECT",
    "EXPECTF",
    "EXPECTREGEX",
    "EXPECT_EXTERNAL",
    "EXPECTF_EXTERNAL",
    "EXPECTREGEX_EXTERNAL",
    "EXPECTHEADERS",
    "CLEAN",
];

/// The sections that hold the test script. Exactly one is required.
pub const FILE_SECTIONS: &[&str] = &["FILE", "FILEEOF", "FILE_EXTERNAL", "REDIRECTTEST"];

/// The sections that hold the expected output. Exactly one is required,
/// except for REDIRECTTEST, which has none.
pub const EXPECT_SECTIONS: &[&str] = &[
    "EXPECT",
    "EXPECTF",
    "EXPECTREGEX",
    "EXPECT_EXTERNAL",
    "EXPECTF_EXTERNAL",
    "EXPECTREGEX_EXTERNAL",
];

/// Sections that only make sense under a web SAPI (CGI). The documentation
/// says GET, POST, POST_RAW, PUT, GZIP_POST, DEFLATE_POST and COOKIE force the
/// CGI binary, CGI does so explicitly, and EXPECTHEADERS compares HTTP headers.
pub const CGI_SECTIONS: &[&str] = &[
    "CGI",
    "GET",
    "POST",
    "POST_RAW",
    "PUT",
    "GZIP_POST",
    "DEFLATE_POST",
    "COOKIE",
    "EXPECTHEADERS",
];

/// One section of a .phpt file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: &'static str,
    /// The bytes between the header line and the next header (or the end of
    /// the file), including the final line break.
    pub body: Vec<u8>,
    /// 1-based line number of the header.
    pub line: usize,
}

/// A parsed .phpt file.
#[derive(Debug, Clone, Default)]
pub struct Phpt {
    pub sections: Vec<Section>,
}

/// Why a file is not a valid .phpt (the runner reports these as BORK).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    MissingTest,
    Duplicate(&'static str),
    NoFile,
    ManyFiles,
    NoExpect,
    ManyExpects,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingTest => write!(f, "first line is not --TEST--"),
            ParseError::Duplicate(n) => write!(f, "duplicate --{n}-- section"),
            ParseError::NoFile => write!(f, "no FILE, FILEEOF, FILE_EXTERNAL or REDIRECTTEST section"),
            ParseError::ManyFiles => write!(f, "more than one FILE-type section"),
            ParseError::NoExpect => write!(f, "no EXPECT-type section"),
            ParseError::ManyExpects => write!(f, "more than one EXPECT-type section"),
        }
    }
}

/// Returns the known section name if `line` (without its line break) is a
/// section header: `--NAME--`, where anything after the closing dashes is
/// ignored. The corpus has first lines `--TEST---`
/// (ext/soap/tests/bugs/gh18640.phpt), `--TEST--n`
/// (ext/standard/tests/array/bug24198.phpt) and `--TEST--Bug #64506`
/// (ext/standard/tests/file/windows_mb_path/bug64506.phpt), each followed by
/// the title on the next line.
fn header_name(line: &[u8]) -> Option<&'static str> {
    let rest = line.strip_prefix(b"--")?;
    let len = rest
        .iter()
        .position(|&b| !(b.is_ascii_uppercase() || b == b'_'))
        .unwrap_or(rest.len());
    if len == 0 || !rest[len..].starts_with(b"--") {
        return None;
    }
    let inner = &rest[..len];
    KNOWN_SECTIONS.iter().copied().find(|n| n.as_bytes() == inner)
}

/// Splits `data` into lines, each including its `\n` (the last one may lack it).
fn lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    data.split_inclusive(|&b| b == b'\n')
}

fn strip_eol(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

impl Phpt {
    /// Parses and validates a .phpt file.
    pub fn parse(data: &[u8]) -> Result<Phpt, ParseError> {
        let p = Phpt::parse_sections(data)?;
        p.validate()?;
        Ok(p)
    }

    /// Splits a file into sections without checking which ones are present.
    pub fn parse_sections(data: &[u8]) -> Result<Phpt, ParseError> {
        let mut sections: Vec<Section> = Vec::new();
        for (idx, line) in lines(data).enumerate() {
            if let Some(name) = header_name(strip_eol(line)) {
                if idx == 0 && name != "TEST" {
                    return Err(ParseError::MissingTest);
                }
                if sections.iter().any(|s| s.name == name) {
                    return Err(ParseError::Duplicate(name));
                }
                sections.push(Section {
                    name,
                    body: Vec::new(),
                    line: idx + 1,
                });
            } else if let Some(cur) = sections.last_mut() {
                cur.body.extend_from_slice(line);
            } else {
                return Err(ParseError::MissingTest);
            }
        }
        if sections.is_empty() {
            return Err(ParseError::MissingTest);
        }
        Ok(Phpt { sections })
    }

    fn validate(&self) -> Result<(), ParseError> {
        let files = FILE_SECTIONS.iter().filter(|n| self.has(n)).count();
        match files {
            // A PHPDBG test may give only phpdbg commands
            // (sapi/phpdbg/tests/basic_run.phpt).
            0 if self.has("PHPDBG") => {}
            0 => return Err(ParseError::NoFile),
            1 => {}
            _ => return Err(ParseError::ManyFiles),
        }
        let expects = EXPECT_SECTIONS.iter().filter(|n| self.has(n)).count();
        if self.has("REDIRECTTEST") {
            // The documentation: a redirect test may hold SKIPIF, ENV and ARGS
            // but no EXPECT.
            return Ok(());
        }
        match expects {
            0 => Err(ParseError::NoExpect),
            1 => Ok(()),
            _ => Err(ParseError::ManyExpects),
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.sections.iter().any(|s| s.name == name)
    }

    /// The raw body of a section.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.sections
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.body.as_slice())
    }

    /// A single-line section (TEST, ARGS, FILE_EXTERNAL, ...) with surrounding
    /// whitespace removed.
    pub fn get_line(&self, name: &str) -> Option<String> {
        self.get(name)
            .map(|b| String::from_utf8_lossy(trim_ws(b)).into_owned())
    }

    /// The non-empty lines of a list section (INI, ENV, EXTENSIONS,
    /// CONFLICTS), trimmed, without `#` comments when `hash_comments`.
    pub fn get_list(&self, name: &str, hash_comments: bool) -> Vec<String> {
        let Some(body) = self.get(name) else {
            return Vec::new();
        };
        list_lines(body, hash_comments)
    }

    /// The name of the FILE-type section present.
    pub fn file_section(&self) -> Option<&'static str> {
        FILE_SECTIONS.iter().copied().find(|n| self.has(n))
    }

    /// The name of the EXPECT-type section present.
    pub fn expect_section(&self) -> Option<&'static str> {
        EXPECT_SECTIONS.iter().copied().find(|n| self.has(n))
    }

    /// The first web-SAPI section present, if any.
    pub fn cgi_section(&self) -> Option<&'static str> {
        CGI_SECTIONS.iter().copied().find(|n| self.has(n))
    }

    /// The test title (the TEST section).
    pub fn title(&self) -> String {
        self.get_line("TEST").unwrap_or_default()
    }
}

/// Splits a list body into trimmed non-empty lines.
pub fn list_lines(body: &[u8], hash_comments: bool) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines(body) {
        let mut line = strip_eol(line);
        if hash_comments && let Some(p) = line.iter().position(|&b| b == b'#') {
            line = &line[..p];
        }
        let line = trim_ws(line);
        if !line.is_empty() {
            out.push(String::from_utf8_lossy(line).into_owned());
        }
    }
    out
}

/// The characters PHP's trim() removes by default: " \t\n\r\0\x0B".
pub fn is_trim_char(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0b)
}

/// Removes leading and trailing trim() characters.
pub fn trim_ws(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|&c| !is_trim_char(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|&c| !is_trim_char(c)).map_or(start, |e| e + 1);
    &b[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_layout() {
        // tests/run-test/test001.phpt
        let p = Phpt::parse(b"--TEST--\nEXPECT\n--FILE--\nabc\n--EXPECT--\nabc\n").unwrap();
        assert_eq!(p.title(), "EXPECT");
        assert_eq!(p.get("FILE"), Some(&b"abc\n"[..]));
        assert_eq!(p.get("EXPECT"), Some(&b"abc\n"[..]));
        assert_eq!(p.sections[1].line, 3);
        assert_eq!(p.file_section(), Some("FILE"));
        assert_eq!(p.expect_section(), Some("EXPECT"));
    }

    #[test]
    fn crlf_headers() {
        // ext/pcre/tests/bug81424a.phpt uses CRLF line endings throughout.
        let p = Phpt::parse(b"--TEST--\r\nt\r\n--INI--\r\ndate.timezone=UTC\r\n--FILE--\r\nx\r\n--EXPECT--\r\nx\r\n")
            .unwrap();
        assert_eq!(p.title(), "t");
        assert_eq!(p.get_list("INI", false), vec!["date.timezone=UTC"]);
        assert_eq!(p.get("FILE"), Some(&b"x\r\n"[..]));
    }

    #[test]
    fn first_line_must_be_test() {
        assert_eq!(
            Phpt::parse(b"\n--TEST--\nx\n--FILE--\n--EXPECT--\n").unwrap_err(),
            ParseError::MissingTest
        );
        assert_eq!(
            Phpt::parse(b"--FILE--\nx\n--EXPECT--\n").unwrap_err(),
            ParseError::MissingTest
        );
        assert_eq!(Phpt::parse(b"").unwrap_err(), ParseError::MissingTest);
    }

    #[test]
    fn header_with_trailing_text() {
        let p = Phpt::parse(b"--TEST--n\nBug #24198\n--FILE--\nx\n--EXPECT--\nx\n").unwrap();
        assert_eq!(p.title(), "Bug #24198");
        let p = Phpt::parse(b"--TEST---\nGH-18640\n--FILE--\nx\n--EXPECT--\nx\n").unwrap();
        assert_eq!(p.title(), "GH-18640");
    }

    #[test]
    fn phpdbg_without_file() {
        assert!(Phpt::parse(b"--TEST--\nt\n--PHPDBG--\nr\nq\n--EXPECTF--\nprompt> \n").is_ok());
    }

    #[test]
    fn unknown_headers_are_body_text() {
        let p = Phpt::parse(b"--TEST--\nt\n--FILE--\n--DONE--\n--\n----\n--EXPECT--\n--DONE--\n").unwrap();
        assert_eq!(p.get("FILE"), Some(&b"--DONE--\n--\n----\n"[..]));
        assert_eq!(p.get("EXPECT"), Some(&b"--DONE--\n"[..]));
    }

    #[test]
    fn required_sections() {
        assert_eq!(
            Phpt::parse(b"--TEST--\nt\n--EXPECT--\n").unwrap_err(),
            ParseError::NoFile
        );
        assert_eq!(
            Phpt::parse(b"--TEST--\nt\n--FILE--\n").unwrap_err(),
            ParseError::NoExpect
        );
        assert_eq!(
            Phpt::parse(b"--TEST--\nt\n--FILE--\n--FILEEOF--\n--EXPECT--\n").unwrap_err(),
            ParseError::ManyFiles
        );
        assert_eq!(
            Phpt::parse(b"--TEST--\nt\n--FILE--\n--EXPECT--\n--EXPECTF--\n").unwrap_err(),
            ParseError::ManyExpects
        );
        assert_eq!(
            Phpt::parse(b"--TEST--\nt\n--FILE--\n--EXPECT--\n--FILE--\n").unwrap_err(),
            ParseError::Duplicate("FILE")
        );
        // REDIRECTTEST has no EXPECT (ext/pdo_sqlite/tests/common.phpt).
        assert!(Phpt::parse(b"--TEST--\nt\n--REDIRECTTEST--\nreturn [];\n").is_ok());
    }

    #[test]
    fn lists_and_lines() {
        // CONFLICTS allows # comments (documentation; sapi/fpm/tests/CONFLICTS).
        let p = Phpt::parse(
            b"--TEST--\n  spaced title  \n--CONFLICTS--\n# why\nserver\n\n all # x\n--ARGS--\n-a 1 -b 2\n--FILE--\n--EXPECT--\n",
        )
        .unwrap();
        assert_eq!(p.title(), "spaced title");
        assert_eq!(p.get_list("CONFLICTS", true), vec!["server", "all"]);
        assert_eq!(p.get_line("ARGS").unwrap(), "-a 1 -b 2");
    }

    #[test]
    fn web_sections() {
        let p = Phpt::parse(b"--TEST--\nt\n--POST--\na=1\n--FILE--\n--EXPECT--\n").unwrap();
        assert_eq!(p.cgi_section(), Some("POST"));
    }

    #[test]
    fn trim() {
        assert_eq!(trim_ws(b"\n\0 a b\t\x0b\r\n"), b"a b");
        assert_eq!(trim_ws(b" \n "), b"");
    }
}
