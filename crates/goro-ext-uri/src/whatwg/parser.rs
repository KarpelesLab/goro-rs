//! The basic URL parser (URL Standard, "URL parsing"), as PHP 8.5 runs it.
//!
//! The state machine is the standard's. Where PHP 8.5 behaves otherwise
//! (all observed on PHP 8.5, mostly visible in the WPT urltestdata expected
//! results and in the validation-error lists PHP returns):
//!
//! - errors are recorded with the input from the parser's position (C
//!   string: cut at a NUL byte); the leading/trailing C0-control-or-space
//!   and the tab/newline errors quote the original input, the others the
//!   cleaned one;
//! - the relative state reports a `\` with the context starting after it;
//! - an authority ending right after `@` is reported as InvalidCredentials
//!   (not HostMissing) before failing;
//! - a `.`/`..` path segment followed by `?` or `#` takes that character
//!   as a segment separator (`https://h/a/..?q` is `https://h/q`);
//! - `^` is not in the path percent-encode set, and spaces at the end of an
//!   opaque path are kept as they are (the standard as of 2023);
//! - input that is not UTF-8 is read the way PHP reads it: a lead byte
//!   takes the bytes its length says without checking them, a lone
//!   continuation or invalid byte is one invalid code point, and a sequence
//!   cut by the end of the input is dropped (percent-encoding then writes
//!   the bytes of the sequence).

use super::errors::{ErrorType, Errors};
use super::host::{self, Host, HostKind, Source};

/// Special schemes and their default ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheme {
    NotSpecial,
    Http,
    Https,
    Ws,
    Wss,
    Ftp,
    File,
}

impl Scheme {
    pub(crate) fn of(s: &[u8]) -> Scheme {
        match s {
            b"http" => Scheme::Http,
            b"https" => Scheme::Https,
            b"ws" => Scheme::Ws,
            b"wss" => Scheme::Wss,
            b"ftp" => Scheme::Ftp,
            b"file" => Scheme::File,
            _ => Scheme::NotSpecial,
        }
    }
    #[inline]
    pub(crate) fn special(self) -> bool {
        self != Scheme::NotSpecial
    }
    pub(crate) fn default_port(self) -> Option<u16> {
        match self {
            Scheme::Http | Scheme::Ws => Some(80),
            Scheme::Https | Scheme::Wss => Some(443),
            Scheme::Ftp => Some(21),
            _ => None,
        }
    }
}

/// A URL record being parsed or modified.
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub scheme: Vec<u8>,
    pub kind: Scheme,
    pub username: Vec<u8>,
    pub password: Vec<u8>,
    pub host: Option<Host>,
    pub port: Option<u16>,
    /// Serialized path: an opaque path as is, a list as "/seg/seg".
    pub path: Vec<u8>,
    /// For a list path: the offset of each segment's '/'.
    pub segs: Vec<u32>,
    pub opaque: bool,
    pub query: Option<Vec<u8>>,
    pub fragment: Option<Vec<u8>>,
}

impl Record {
    pub(crate) fn new() -> Record {
        Record {
            scheme: Vec::new(),
            kind: Scheme::NotSpecial,
            username: Vec::new(),
            password: Vec::new(),
            host: None,
            port: None,
            path: Vec::new(),
            segs: Vec::new(),
            opaque: false,
            query: None,
            fragment: None,
        }
    }

    pub(crate) fn special(&self) -> bool {
        self.kind.special()
    }

    /// Empties the record, keeping its buffers.
    pub(crate) fn reset(&mut self) {
        self.scheme.clear();
        self.kind = Scheme::NotSpecial;
        self.username.clear();
        self.password.clear();
        self.host = None;
        self.port = None;
        self.path.clear();
        self.segs.clear();
        self.opaque = false;
        self.query = None;
        self.fragment = None;
    }

    pub(crate) fn includes_credentials(&self) -> bool {
        !self.username.is_empty() || !self.password.is_empty()
    }

    /// PHP allows credentials and a port with an empty non-special host
    /// (observed on PHP 8.5: `sc:///` with username "x" is `sc://x@/`).
    pub(crate) fn cannot_have_credentials_or_port(&self) -> bool {
        self.host.is_none() || self.kind == Scheme::File
    }

    pub(crate) fn push_seg(&mut self, seg: &[u8]) {
        self.segs.push(self.path.len() as u32);
        self.path.push(b'/');
        self.path.extend_from_slice(seg);
    }

    pub(crate) fn clear_path(&mut self) {
        self.path.clear();
        self.segs.clear();
    }

    pub(crate) fn seg(&self, i: usize) -> &[u8] {
        let start = self.segs[i] as usize + 1;
        let end = self
            .segs
            .get(i + 1)
            .map_or(self.path.len(), |&e| e as usize);
        &self.path[start..end]
    }

    /// "Shorten a URL's path".
    pub(crate) fn shorten(&mut self) {
        if self.kind == Scheme::File
            && self.segs.len() == 1
            && starts_with_normalized_drive(self.seg(0))
        {
            return;
        }
        if let Some(last) = self.segs.pop() {
            self.path.truncate(last as usize);
        }
    }

    pub(crate) fn set_scheme(&mut self, s: &[u8]) {
        self.scheme.clear();
        self.scheme.extend_from_slice(s);
        self.kind = Scheme::of(s);
    }
}

pub(crate) fn is_drive(s: &[u8]) -> bool {
    s.len() == 2 && s[0].is_ascii_alphabetic() && (s[1] == b':' || s[1] == b'|')
}

/// PHP's test for "a normalized Windows drive letter" where a path is
/// shortened or a file URL takes its base's drive: only the first two bytes
/// are looked at (observed on PHP 8.5: `file:///u:p/..` keeps `u:p`).
fn starts_with_normalized_drive(s: &[u8]) -> bool {
    s.len() >= 2 && s[0].is_ascii_alphabetic() && s[1] == b':'
}

/// "Starts with a Windows drive letter".
fn starts_with_drive(s: &[u8]) -> bool {
    s.len() >= 2
        && is_drive(&s[..2])
        && (s.len() == 2 || matches!(s[2], b'/' | b'\\' | b'?' | b'#'))
}

/// The outcome of [`Parser::scan_path`].
enum Scan {
    /// The path text ends there and is taken as it is; the listed `\`
    /// were passed over after a `%` and are not separators.
    Clean(usize, Vec<usize>),
    /// Read again from after this separator (or from the start).
    Dirty(Option<usize>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    SchemeStart,
    Scheme,
    NoScheme,
    SpecialRelativeOrAuthority,
    PathOrAuthority,
    Relative,
    RelativeSlash,
    SpecialAuthoritySlashes,
    SpecialAuthorityIgnoreSlashes,
    Authority,
    Host,
    Hostname,
    Port,
    File,
    FileSlash,
    FileHost,
    PathStart,
    Path,
    OpaquePath,
    Query,
    Fragment,
}

// ---- percent-encode sets ----

pub(crate) const C0: u8 = 1;
pub(crate) const FRAG: u8 = 2;
pub(crate) const QUERY: u8 = 4;
pub(crate) const SQUERY: u8 = 8;
pub(crate) const PATH: u8 = 16;
pub(crate) const USERINFO: u8 = 32;
pub(crate) const URLCP: u8 = 64;

pub(crate) static SETS: [u8; 128] = {
    let mut t = [0u8; 128];
    let mut i = 0;
    while i < 128 {
        let c = i as u8;
        let mut v = 0;
        let c0 = c <= 0x1f || c == 0x7f;
        if c0 {
            v |= C0;
        }
        let frag = c0 || matches!(c, b' ' | b'"' | b'<' | b'>' | b'`');
        if frag {
            v |= FRAG;
        }
        let query = c0 || matches!(c, b' ' | b'"' | b'#' | b'<' | b'>');
        if query {
            v |= QUERY;
        }
        if query || c == b'\'' {
            v |= SQUERY;
        }
        let path = query || matches!(c, b'?' | b'`' | b'{' | b'}');
        if path {
            v |= PATH;
        }
        if path
            || matches!(
                c,
                b'/' | b':' | b';' | b'=' | b'@' | b'[' | b'\\' | b']' | b'^' | b'|'
            )
        {
            v |= USERINFO;
        }
        if c.is_ascii_alphanumeric()
            || matches!(
                c,
                b'!' | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b'-'
                    | b'.'
                    | b'/'
                    | b':'
                    | b';'
                    | b'='
                    | b'?'
                    | b'@'
                    | b'_'
                    | b'~'
            )
        {
            v |= URLCP;
        }
        t[i] = v;
        i += 1;
    }
    t
};

/// A URL code point outside ASCII (U+00A0..U+10FFFD, no surrogates or
/// noncharacters).
fn is_url_code_point_wide(cp: u32) -> bool {
    (0xA0..=0x10FFFD).contains(&cp)
        && !(0xD800..=0xDFFF).contains(&cp)
        && !(0xFDD0..=0xFDEF).contains(&cp)
        && (cp & 0xFFFE) != 0xFFFE
}

#[inline]
pub(crate) fn push_pct(out: &mut Vec<u8>, b: u8) {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    out.push(b'%');
    out.push(H[(b >> 4) as usize]);
    out.push(H[(b & 15) as usize]);
}

/// Appends the bytes of a unit, percent-encoding them with `set` (bytes
/// outside ASCII are always encoded).
#[inline]
pub(crate) fn push_encoded(out: &mut Vec<u8>, bytes: &[u8], set: u8) {
    for &b in bytes {
        if b >= 0x80 || SETS[b as usize] & set != 0 {
            push_pct(out, b);
        } else {
            out.push(b);
        }
    }
}

pub(crate) fn encode_into(out: &mut Vec<u8>, s: &[u8], set: Set) {
    push_encoded(out, s, set.bits());
}

/// The percent-encode sets the API setters use.
#[derive(Clone, Copy)]
pub(crate) enum Set {
    Userinfo,
}

impl Set {
    fn bits(self) -> u8 {
        match self {
            Set::Userinfo => USERINFO,
        }
    }
}

const EOF: u32 = u32::MAX;
/// A byte that does not start a UTF-8 sequence.
const INVALID: u32 = u32::MAX - 1;
/// A sequence cut by the end of the input.
const TRUNCATED: u32 = u32::MAX - 2;

/// Reads the code point at `p` the way PHP 8.5 does (see the module docs):
/// (code point, byte length).
#[inline]
fn unit(s: &[u8], p: usize) -> (u32, usize) {
    let Some(&b) = s.get(p) else {
        return (EOF, 0);
    };
    if b < 0x80 {
        return (b as u32, 1);
    }
    let need = match b {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => return (INVALID, 1),
    };
    if p + need > s.len() {
        return (TRUNCATED, s.len() - p);
    }
    let cp = match need {
        2 => ((b as u32 & 0x1F) << 6) | (s[p + 1] as u32 & 0x3F),
        3 => ((b as u32 & 0x0F) << 12) | ((s[p + 1] as u32 & 0x3F) << 6) | (s[p + 2] as u32 & 0x3F),
        _ => {
            ((b as u32 & 0x07) << 18)
                | ((s[p + 1] as u32 & 0x3F) << 12)
                | ((s[p + 2] as u32 & 0x3F) << 6)
                | (s[p + 3] as u32 & 0x3F)
        }
    };
    (cp, need)
}

/// The base URL as the parser needs it.
pub(crate) type Base<'a> = Option<&'a Record>;

pub(crate) struct Parser<'a> {
    /// The text being parsed: a prefix of `ctx` (the trailing
    /// C0-control-or-space is not parsed, but PHP's error contexts run to
    /// the end of the string).
    pub input: &'a [u8],
    pub ctx: &'a [u8],
    pub base: Base<'a>,
    pub url: &'a mut Record,
    pub errs: &'a mut Errors,
    pub over: Option<State>,
}

/// The result of a parse: `Ok(())` or failure.
pub(crate) type Outcome = Result<(), ()>;

/// Strips the leading/trailing C0-control-or-space and removes tabs and
/// newlines, recording PHP's errors for them; returns the text for the
/// error contexts and the length to parse.
pub(crate) fn clean<'b>(
    orig: &'b [u8],
    errs: &mut Errors,
    trim: bool,
) -> (std::borrow::Cow<'b, [u8]>, usize) {
    let (mut start, mut end) = (0, orig.len());
    if trim {
        // PHP trims bytes that do not start a UTF-8 sequence (and cut
        // sequences) along with the leading C0 controls and spaces
        // (observed on PHP 8.5: "\xFFhttp://h/" is "http://h/", and
        // "\r\xFF\t#x" reports one error).
        loop {
            if start < end && orig[start] <= 0x20 {
                start += 1;
                continue;
            }
            match unit(&orig[..end], start) {
                (INVALID | TRUNCATED, l) if start < end => start += l,
                _ => break,
            }
        }
        while end > start && orig[end - 1] <= 0x20 {
            end -= 1;
        }
        if start > 0 {
            errs.push(ErrorType::InvalidUrlUnit, orig, 0);
        }
        if end < orig.len() && end > start {
            errs.push(ErrorType::InvalidUrlUnit, orig, end);
        }
    }
    let s = &orig[start..];
    match s[..end - start]
        .iter()
        .position(|&c| c == b'\t' || c == b'\n' || c == b'\r')
    {
        None => (std::borrow::Cow::Borrowed(s), end - start),
        Some(p) => {
            // The copy without tabs and newlines holds only the trimmed
            // text (observed on PHP 8.5: the contexts then stop there).
            errs.push(ErrorType::InvalidUrlUnit, orig, start + p);
            let is_tab = |c: &u8| *c == b'\t' || *c == b'\n' || *c == b'\r';
            let v: Vec<u8> = s[..end - start]
                .iter()
                .copied()
                .filter(|c| !is_tab(c))
                .collect();
            let len = v.len();
            (std::borrow::Cow::Owned(v), len)
        }
    }
}

impl Parser<'_> {
    #[inline]
    fn err(&mut self, t: ErrorType, p: usize) {
        self.errs.push(t, self.ctx, p);
    }

    #[inline]
    fn byte(&self, p: usize) -> Option<u8> {
        self.input.get(p).copied()
    }

    /// Whether the input after `p` starts with two hex digits.
    #[inline]
    fn two_hex_after(&self, p: usize) -> bool {
        self.byte(p + 1).is_some_and(|c| c.is_ascii_hexdigit())
            && self.byte(p + 2).is_some_and(|c| c.is_ascii_hexdigit())
    }

    /// The URL-code-point and percent checks of the path, query, fragment
    /// and opaque path states.
    #[inline]
    fn check_unit(&mut self, cp: u32, p: usize) {
        if cp < 0x80 {
            if cp == b'%' as u32 {
                if !self.two_hex_after(p) {
                    self.err(ErrorType::InvalidUrlUnit, p);
                }
            } else if SETS[cp as usize] & URLCP == 0 {
                self.err(ErrorType::InvalidUrlUnit, p);
            }
        } else if cp == TRUNCATED || cp == INVALID || !is_url_code_point_wide(cp) {
            self.err(ErrorType::InvalidUrlUnit, p);
        }
    }

    /// PHP reads a path with a fast scan first (observed on PHP 8.5). The
    /// scan runs to the end of the path (`?`, `#` or the end); a `%` not
    /// followed by two hex digits is reported and the two bytes after it
    /// are passed over unseen, whatever they are (a `?` or `#` there does
    /// not end the path); a special URL's `\` is reported as it is met.
    /// When the scan meets nothing that needs percent-encoding or is not a
    /// URL code point, the path text is taken as it is ([`Scan::Clean`]).
    /// Otherwise PHP reads the path again the standard way from the start
    /// of the segment holding that byte (reporting such a `%` there again),
    /// and takes the segments before it from the scanned text
    /// ([`Scan::Dirty`]).
    fn scan_path(&mut self, start: usize) -> Scan {
        let s = self.input;
        let special = self.url.special();
        let mut i = start;
        let mut last_sep: Option<usize> = None;
        let mut plain: Vec<usize> = Vec::new();
        loop {
            let Some(&b) = s.get(i) else {
                return Scan::Clean(s.len(), plain);
            };
            if self.over.is_none() && (b == b'?' || b == b'#') {
                return Scan::Clean(i, plain);
            }
            if b == b'/' || (special && b == b'\\') {
                if b == b'\\' {
                    self.err(ErrorType::InvalidReverseSoldius, i);
                }
                last_sep = Some(i);
                i += 1;
                continue;
            }
            if b == b'%' {
                if !self.two_hex_after(i) {
                    self.err(ErrorType::InvalidUrlUnit, i);
                }
                let window = i + 1..(i + 3).min(s.len());
                for (j, &c) in s[window.clone()].iter().enumerate() {
                    if special && c == b'\\' {
                        plain.push(window.start + j);
                    }
                }
                i = (i + 3).min(s.len());
                continue;
            }
            if b < 0x80 && SETS[b as usize] & URLCP != 0 && SETS[b as usize] & PATH == 0 {
                i += 1;
                continue;
            }
            return Scan::Dirty(last_sep);
        }
    }

    /// Whether the text from `from` to the next separator (or `end`) is a
    /// `.` segment.
    fn next_is_single_dot(&self, from: usize, end: usize) -> bool {
        let special = self.url.special();
        let s = &self.input[from.min(end)..end];
        let n = s
            .iter()
            .position(|&c| c == b'/' || (special && c == b'\\'))
            .unwrap_or(s.len());
        is_single_dot(&s[..n])
    }

    /// The segments PHP takes from the scanned text before a restart (see
    /// [`Parser::scan_path`]): the text up to the last separator, split at
    /// separators; an empty text gives none, and an empty segment next to a
    /// `.` or `..` one is dropped.
    fn prefix_segments(&mut self, text: &[u8]) {
        if text.is_empty() {
            return;
        }
        let special = self.url.special();
        let pieces: Vec<&[u8]> = text
            .split(|&c| c == b'/' || (special && c == b'\\'))
            .collect();
        let is_dot = |p: &[u8]| is_single_dot(p) || is_double_dot(p);
        for (i, &piece) in pieces.iter().enumerate() {
            let dropped = is_single_dot(piece)
                || (piece.is_empty()
                    && ((i > 0 && is_dot(pieces[i - 1]))
                        || pieces.get(i + 1).is_some_and(|n| is_dot(n))));
            if is_double_dot(piece) {
                self.url.shorten();
            } else if !dropped {
                let mut seg = piece.to_vec();
                if self.url.kind == Scheme::File && self.url.segs.is_empty() && is_drive(&seg) {
                    seg[1] = b':';
                }
                self.url.push_seg(&seg);
            }
        }
    }

    pub(crate) fn run(&mut self, start_state: State) -> Outcome {
        let input = self.input;
        let n = input.len();
        let mut state = start_state;
        let mut p: usize = 0;
        let mut buffer: Vec<u8> = Vec::with_capacity(n);
        let mut at_sign_seen = false;
        let mut inside_brackets = false;
        let mut password_token_seen = false;
        // Start of the buffered text in the input (authority, host, file
        // host and port states).
        let mut buf_start: usize = 0;
        // The path is read the standard way from there on (see
        // `scan_path`); before `raw_until`, bytes are taken as they are.
        let mut slow_path = false;
        let mut raw_until: usize = 0;
        let mut raw_plain: Vec<usize> = Vec::new();
        loop {
            let (cp, mut len) = unit(input, p);
            // The authority states look at bytes (observed on PHP 8.5: a
            // lead byte does not take a following '/' with it there).
            if cp >= 0x80
                && cp != EOF
                && matches!(
                    state,
                    State::Authority | State::Host | State::Hostname | State::FileHost
                )
            {
                len = 1;
            }
            let c = if cp < 0x80 { cp as u8 } else { 0x80 };
            let is_eof = cp == EOF;
            // `advance` false re-runs the next state on the same position.
            let mut advance = true;
            match state {
                State::SchemeStart => {
                    if !is_eof && cp < 0x80 && c.is_ascii_alphabetic() {
                        buffer.push(c.to_ascii_lowercase());
                        state = State::Scheme;
                    } else if self.over.is_none() {
                        state = State::NoScheme;
                        advance = false;
                    } else {
                        return Err(());
                    }
                }
                State::Scheme => {
                    if !is_eof
                        && cp < 0x80
                        && (c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
                    {
                        buffer.push(c.to_ascii_lowercase());
                    } else if !is_eof && c == b':' {
                        if self.over.is_some() {
                            let new_special = Scheme::of(&buffer).special();
                            if self.url.special() != new_special {
                                return Ok(());
                            }
                            if (self.url.includes_credentials() || self.url.port.is_some())
                                && buffer == b"file"
                            {
                                return Ok(());
                            }
                            if self.url.kind == Scheme::File
                                && self
                                    .url
                                    .host
                                    .as_ref()
                                    .is_some_and(|h| h.kind == HostKind::Empty)
                            {
                                return Ok(());
                            }
                        }
                        self.url.set_scheme(&buffer);
                        if self.over.is_some() {
                            if self.url.port.is_some()
                                && self.url.port == self.url.kind.default_port()
                            {
                                self.url.port = None;
                            }
                            return Ok(());
                        }
                        buffer.clear();
                        if self.url.kind == Scheme::File {
                            if !input[p + 1..].starts_with(b"//") {
                                self.err(ErrorType::SpecialSchemeMissingFollowingSolidus, p + 1);
                            }
                            state = State::File;
                        } else if self.url.special()
                            && self.base.is_some_and(|b| b.scheme == self.url.scheme)
                        {
                            state = State::SpecialRelativeOrAuthority;
                        } else if self.url.special() {
                            state = State::SpecialAuthoritySlashes;
                        } else if self.byte(p + 1) == Some(b'/') {
                            state = State::PathOrAuthority;
                            p += 1;
                        } else {
                            self.url.opaque = true;
                            self.url.clear_path();
                            state = State::OpaquePath;
                        }
                    } else if self.over.is_none() {
                        buffer.clear();
                        state = State::NoScheme;
                        p = 0;
                        continue;
                    } else {
                        return Err(());
                    }
                }
                State::NoScheme => {
                    let base = self.base;
                    match base {
                        None => {
                            self.err(ErrorType::MissingSchemeNonRelativeUrl, p);
                            return Err(());
                        }
                        Some(b) if b.opaque && c != b'#' => {
                            self.err(ErrorType::MissingSchemeNonRelativeUrl, p);
                            return Err(());
                        }
                        Some(b) if b.opaque => {
                            self.url.set_scheme(&b.scheme.clone());
                            self.url.path = b.path.clone();
                            self.url.segs = b.segs.clone();
                            self.url.opaque = true;
                            self.url.query = b.query.clone();
                            self.url.fragment = Some(Vec::new());
                            state = State::Fragment;
                        }
                        Some(b) if b.kind != Scheme::File => {
                            state = State::Relative;
                            advance = false;
                        }
                        Some(_) => {
                            state = State::File;
                            advance = false;
                        }
                    }
                }
                State::SpecialRelativeOrAuthority => {
                    if c == b'/' && self.byte(p + 1) == Some(b'/') {
                        state = State::SpecialAuthorityIgnoreSlashes;
                        p += 1;
                    } else {
                        self.err(ErrorType::SpecialSchemeMissingFollowingSolidus, p);
                        state = State::Relative;
                        advance = false;
                    }
                }
                State::PathOrAuthority => {
                    if c == b'/' {
                        state = State::Authority;
                        buf_start = p + 1;
                    } else {
                        state = State::Path;
                        advance = false;
                    }
                }
                State::Relative => {
                    let b = self.base.unwrap();
                    self.url.set_scheme(&b.scheme.clone());
                    if c == b'/' && !is_eof {
                        state = State::RelativeSlash;
                    } else if self.url.special() && c == b'\\' && !is_eof {
                        self.err(ErrorType::InvalidReverseSoldius, p + 1);
                        state = State::RelativeSlash;
                    } else {
                        self.url.username = b.username.clone();
                        self.url.password = b.password.clone();
                        self.url.host = b.host.clone();
                        self.url.port = b.port;
                        self.url.path = b.path.clone();
                        self.url.segs = b.segs.clone();
                        self.url.query = b.query.clone();
                        if !is_eof && c == b'?' {
                            self.url.query = Some(Vec::new());
                            state = State::Query;
                        } else if !is_eof && c == b'#' {
                            self.url.fragment = Some(Vec::new());
                            state = State::Fragment;
                        } else if !is_eof {
                            self.url.query = None;
                            self.url.shorten();
                            state = State::Path;
                            advance = false;
                        }
                    }
                }
                State::RelativeSlash => {
                    if self.url.special() && !is_eof && (c == b'/' || c == b'\\') {
                        if c == b'\\' {
                            self.err(ErrorType::InvalidReverseSoldius, p);
                        }
                        state = State::SpecialAuthorityIgnoreSlashes;
                    } else if !is_eof && c == b'/' {
                        state = State::Authority;
                        buf_start = p + 1;
                    } else {
                        let b = self.base.unwrap();
                        self.url.username = b.username.clone();
                        self.url.password = b.password.clone();
                        self.url.host = b.host.clone();
                        self.url.port = b.port;
                        state = State::Path;
                        advance = false;
                    }
                }
                State::SpecialAuthoritySlashes => {
                    if c == b'/' && !is_eof && self.byte(p + 1) == Some(b'/') {
                        state = State::SpecialAuthorityIgnoreSlashes;
                        p += 1;
                    } else {
                        self.err(ErrorType::SpecialSchemeMissingFollowingSolidus, p);
                        state = State::SpecialAuthorityIgnoreSlashes;
                        advance = false;
                    }
                }
                State::SpecialAuthorityIgnoreSlashes => {
                    if is_eof || (c != b'/' && c != b'\\') {
                        state = State::Authority;
                        buf_start = p;
                        advance = false;
                    } else {
                        self.err(ErrorType::SpecialSchemeMissingFollowingSolidus, p);
                    }
                }
                State::Authority => {
                    if !is_eof && c == b'@' {
                        self.err(ErrorType::InvalidCredentials, p);
                        if at_sign_seen && !password_token_seen {
                            if let Some(colon) = buffer.iter().position(|&b| b == b':') {
                                // PHP drops what comes before the ':' of a
                                // later chunk (observed on PHP 8.5:
                                // "http://u@a:b@h/" has username "u" and
                                // password "b").
                                buffer.drain(..colon);
                            } else {
                                let mut nb = b"%40".to_vec();
                                nb.extend_from_slice(&buffer);
                                buffer = nb;
                            }
                        } else if at_sign_seen {
                            let mut nb = b"%40".to_vec();
                            nb.extend_from_slice(&buffer);
                            buffer = nb;
                        }
                        at_sign_seen = true;
                        let mut i = 0;
                        while i < buffer.len() {
                            let (ucp, ulen) = unit(&buffer, i);
                            let ulen = ulen.max(1);
                            if ucp == b':' as u32 && !password_token_seen {
                                password_token_seen = true;
                                i += 1;
                                continue;
                            }
                            let target = if password_token_seen {
                                &mut self.url.password
                            } else {
                                &mut self.url.username
                            };
                            push_encoded(target, &buffer[i..i + ulen], USERINFO);
                            i += ulen;
                        }
                        buffer.clear();
                        buf_start = p + 1;
                    } else if is_eof
                        || c == b'/'
                        || c == b'?'
                        || c == b'#'
                        || (self.url.special() && c == b'\\')
                    {
                        if at_sign_seen && buffer.is_empty() {
                            self.err(ErrorType::InvalidCredentials, p);
                            return Err(());
                        }
                        p = buf_start;
                        buffer.clear();
                        state = State::Host;
                        continue;
                    } else {
                        // Takes the run of bytes that are none of the above.
                        let q = run_end(input, p + len, |c| {
                            !matches!(c, b'@' | b'/' | b'?' | b'#' | b'\\')
                        });
                        buffer.extend_from_slice(&input[p..q]);
                        p = q;
                        continue;
                    }
                }
                State::Host | State::Hostname => {
                    if self.over.is_some() && self.url.kind == Scheme::File {
                        state = State::FileHost;
                        buf_start = p;
                        advance = false;
                    } else if !is_eof && c == b':' && !inside_brackets {
                        if buffer.is_empty() {
                            self.err(ErrorType::HostMissing, p);
                            return Err(());
                        }
                        if self.over == Some(State::Hostname) {
                            return Err(());
                        }
                        let h = host::parse_host(
                            &buffer,
                            &Source {
                                input: self.ctx,
                                start: p - buffer.len(),
                            },
                            !self.url.special(),
                            self.errs,
                        )
                        .ok_or(())?;
                        self.url.host = Some(h);
                        buffer.clear();
                        state = State::Port;
                        buf_start = p + 1;
                    } else if is_eof
                        || c == b'/'
                        || c == b'?'
                        || c == b'#'
                        || (self.url.special() && c == b'\\')
                    {
                        if self.url.special() && buffer.is_empty() {
                            self.err(ErrorType::HostMissing, p);
                            return Err(());
                        } else if self.over.is_some()
                            && buffer.is_empty()
                            && (self.url.includes_credentials() || self.url.port.is_some())
                        {
                            return Ok(());
                        }
                        let h = host::parse_host(
                            &buffer,
                            &Source {
                                input: self.ctx,
                                start: p - buffer.len(),
                            },
                            !self.url.special(),
                            self.errs,
                        )
                        .ok_or(())?;
                        self.url.host = Some(h);
                        buffer.clear();
                        state = State::PathStart;
                        if self.over.is_some() {
                            return Ok(());
                        }
                        advance = false;
                    } else {
                        if c == b'[' {
                            inside_brackets = true;
                        }
                        if c == b']' {
                            inside_brackets = false;
                        }
                        let q = run_end(input, p + len, |c| {
                            !matches!(c, b':' | b'/' | b'?' | b'#' | b'\\' | b'[' | b']')
                        });
                        buffer.extend_from_slice(&input[p..q]);
                        p = q;
                        continue;
                    }
                }
                State::Port => {
                    if !is_eof && c.is_ascii_digit() && cp < 0x80 {
                        buffer.push(c);
                    } else if is_eof
                        || c == b'/'
                        || c == b'?'
                        || c == b'#'
                        || (self.url.special() && c == b'\\')
                        || self.over.is_some()
                    {
                        if !buffer.is_empty() {
                            let mut v: u32 = 0;
                            for &d in &buffer {
                                v = v * 10 + (d - b'0') as u32;
                                if v > 65535 {
                                    break;
                                }
                            }
                            if v > 65535 {
                                self.err(ErrorType::PortOutOfRange, p);
                                return Err(());
                            }
                            let v = v as u16;
                            self.url.port = if Some(v) == self.url.kind.default_port() {
                                None
                            } else {
                                Some(v)
                            };
                            buffer.clear();
                        }
                        if self.over.is_some() {
                            return Ok(());
                        }
                        state = State::PathStart;
                        advance = false;
                    } else {
                        self.err(ErrorType::PortInvalid, p);
                        return Err(());
                    }
                }
                State::File => {
                    self.url.set_scheme(b"file");
                    self.url.host = Some(Host::empty());
                    if !is_eof && (c == b'/' || c == b'\\') {
                        if c == b'\\' {
                            self.err(ErrorType::InvalidReverseSoldius, p);
                        }
                        state = State::FileSlash;
                    } else if let Some(b) = self.base.filter(|b| b.kind == Scheme::File) {
                        self.url.host = b.host.clone();
                        self.url.path = b.path.clone();
                        self.url.segs = b.segs.clone();
                        self.url.query = b.query.clone();
                        if !is_eof && c == b'?' {
                            self.url.query = Some(Vec::new());
                            state = State::Query;
                        } else if !is_eof && c == b'#' {
                            self.url.fragment = Some(Vec::new());
                            state = State::Fragment;
                        } else if !is_eof {
                            self.url.query = None;
                            if !starts_with_drive(&input[p..]) {
                                self.url.shorten();
                            } else {
                                self.err(ErrorType::FileInvalidWindowsDriveLetter, p);
                                self.url.clear_path();
                            }
                            state = State::Path;
                            advance = false;
                        }
                    } else {
                        state = State::Path;
                        advance = false;
                    }
                }
                State::FileSlash => {
                    if !is_eof && (c == b'/' || c == b'\\') {
                        if c == b'\\' {
                            self.err(ErrorType::InvalidReverseSoldius, p);
                        }
                        state = State::FileHost;
                        buf_start = p + 1;
                    } else {
                        if let Some(b) = self.base.filter(|b| b.kind == Scheme::File) {
                            self.url.host = b.host.clone();
                            if !starts_with_drive(&input[p..])
                                && !b.segs.is_empty()
                                && starts_with_normalized_drive(b.seg(0))
                            {
                                let s0 = b.seg(0).to_vec();
                                self.url.push_seg(&s0);
                            }
                        }
                        state = State::Path;
                        advance = false;
                    }
                }
                State::FileHost => {
                    if is_eof || matches!(c, b'/' | b'\\' | b'?' | b'#') {
                        if self.over.is_none() && is_drive(&buffer) {
                            // The drive letter is read again as the start of
                            // the path (PHP reports a '|' there as an
                            // invalid URL unit; WPT `//C|/foo/bar`).
                            self.err(ErrorType::FileInvalidWindowsDriveLetterHost, buf_start);
                            state = State::Path;
                            p = buf_start;
                            buffer.clear();
                            continue;
                        } else if buffer.is_empty() {
                            self.url.host = Some(Host::empty());
                            if self.over.is_some() {
                                return Ok(());
                            }
                            state = State::PathStart;
                            advance = false;
                        } else {
                            let mut h = host::parse_host(
                                &buffer,
                                &Source {
                                    input: self.ctx,
                                    start: buf_start,
                                },
                                !self.url.special(),
                                self.errs,
                            )
                            .ok_or(())?;
                            if h.text == b"localhost" {
                                h = Host::empty();
                            }
                            self.url.host = Some(h);
                            if self.over.is_some() {
                                return Ok(());
                            }
                            buffer.clear();
                            state = State::PathStart;
                            advance = false;
                        }
                    } else {
                        buffer.extend_from_slice(&input[p..p + len]);
                    }
                }
                State::PathStart => {
                    if self.url.special() {
                        if c == b'\\' && !is_eof {
                            self.err(ErrorType::InvalidReverseSoldius, p);
                        }
                        state = State::Path;
                        if is_eof || (c != b'/' && c != b'\\') {
                            advance = false;
                        }
                    } else if self.over.is_none() && !is_eof && c == b'?' {
                        self.url.query = Some(Vec::new());
                        state = State::Query;
                    } else if self.over.is_none() && !is_eof && c == b'#' {
                        self.url.fragment = Some(Vec::new());
                        state = State::Fragment;
                    } else if !is_eof {
                        state = State::Path;
                        if c != b'/' {
                            advance = false;
                        }
                    } else if self.over.is_some() && self.url.host.is_none() {
                        self.url.push_seg(b"");
                    }
                }
                State::Path => {
                    let special = self.url.special();
                    if !slow_path && p >= raw_until && buffer.is_empty() {
                        match self.scan_path(p) {
                            Scan::Clean(end, plain) => {
                                raw_until = end;
                                raw_plain = plain;
                            }
                            Scan::Dirty(last_sep) => {
                                slow_path = true;
                                if let Some(ls) = last_sep {
                                    if ls == p && input[ls] == b'\\' {
                                        // A leading '\' keeps its empty
                                        // segment, a '/' does not (observed
                                        // on PHP 8.5).
                                        self.url.push_seg(b"");
                                    }
                                    self.prefix_segments(&input[p..ls]);
                                    p = ls + 1;
                                    continue;
                                }
                            }
                        }
                    }
                    let raw = p < raw_until;
                    let sep = !is_eof
                        && (c == b'/'
                            || (special && c == b'\\' && !(raw && raw_plain.contains(&p))));
                    let qf = !raw && self.over.is_none() && !is_eof && (c == b'?' || c == b'#');
                    if is_eof || sep || qf {
                        if special && c == b'\\' && !is_eof && !raw {
                            self.err(ErrorType::InvalidReverseSoldius, p);
                        }
                        let dd = is_double_dot(&buffer);
                        let sd = !dd && is_single_dot(&buffer);
                        // PHP: '?' or '#' right after a dot segment acts as
                        // a separator.
                        let sep = sep || (qf && (dd || sd));
                        if dd {
                            self.url.shorten();
                            if !sep {
                                self.url.push_seg(b"");
                            }
                        } else if sd {
                            if !sep {
                                self.url.push_seg(b"");
                            }
                        } else if raw
                            && sep
                            && buffer.is_empty()
                            && self.next_is_single_dot(p + 1, raw_until)
                        {
                            // An empty segment right before a "." segment is
                            // dropped (observed on PHP 8.5: "http://h//./x"
                            // is "http://h/x"; "/.//x" keeps its empty
                            // segment, as WPT expects).
                        } else {
                            if self.url.kind == Scheme::File
                                && self.url.segs.is_empty()
                                && is_drive(&buffer)
                            {
                                buffer[1] = b':';
                            }
                            let seg = std::mem::take(&mut buffer);
                            self.url.push_seg(&seg);
                            buffer = seg;
                        }

                        buffer.clear();
                        if !sep && !is_eof && c == b'?' {
                            self.url.query = Some(Vec::new());
                            state = State::Query;
                        } else if !sep && !is_eof && c == b'#' {
                            self.url.fragment = Some(Vec::new());
                            state = State::Fragment;
                        }
                    } else if raw {
                        // Units up to the next separator (a lead byte takes
                        // the bytes after it, whatever they are).
                        let mut q = (p + len).min(raw_until);
                        while q < raw_until {
                            let b = input[q];
                            if b == b'/' || (special && b == b'\\' && !raw_plain.contains(&q)) {
                                break;
                            }
                            q += if b < 0x80 { 1 } else { unit(input, q).1.max(1) };
                        }
                        let q = q.min(raw_until);
                        buffer.extend_from_slice(&input[p..q]);
                        p = q;
                        continue;
                    } else {
                        self.check_unit(cp, p);
                        if cp != TRUNCATED {
                            push_encoded(&mut buffer, &input[p..p + len], PATH);
                        }
                    }
                }
                State::OpaquePath => {
                    if !is_eof && c == b'?' {
                        self.url.query = Some(Vec::new());
                        state = State::Query;
                    } else if !is_eof && c == b'#' {
                        self.url.fragment = Some(Vec::new());
                        state = State::Fragment;
                    } else if !is_eof && is_urlcp(c) && c != b'?' {
                        let q = run_end(input, p, |c| is_urlcp(c) && c != b'?');
                        self.url.path.extend_from_slice(&input[p..q]);
                        p = q;
                        continue;
                    } else if !is_eof {
                        self.check_unit(cp, p);
                        if cp != TRUNCATED {
                            push_encoded(&mut self.url.path, &input[p..p + len], C0);
                        }
                    }
                }
                State::Query => {
                    if (self.over.is_none() && !is_eof && c == b'#') || is_eof {
                        let set = if self.url.special() { SQUERY } else { QUERY };
                        let q = self.url.query.get_or_insert_with(Vec::new);
                        push_encoded(q, &buffer, set);
                        buffer.clear();
                        if !is_eof {
                            self.url.fragment = Some(Vec::new());
                            state = State::Fragment;
                        }
                    } else if is_urlcp(c) {
                        let q = run_end(input, p, is_urlcp);
                        buffer.extend_from_slice(&input[p..q]);
                        p = q;
                        continue;
                    } else {
                        self.check_unit(cp, p);
                        if cp != TRUNCATED {
                            buffer.extend_from_slice(&input[p..p + len]);
                        }
                    }
                }
                State::Fragment => {
                    if !is_eof && is_urlcp(c) {
                        let q = run_end(input, p, is_urlcp);
                        let f = self.url.fragment.get_or_insert_with(Vec::new);
                        f.extend_from_slice(&input[p..q]);
                        p = q;
                        continue;
                    } else if !is_eof {
                        self.check_unit(cp, p);
                        if cp != TRUNCATED {
                            let f = self.url.fragment.get_or_insert_with(Vec::new);
                            push_encoded(f, &input[p..p + len], FRAG);
                        }
                    }
                }
            }
            if is_eof && advance {
                break;
            }
            if advance {
                p += len.max(1);
                if p > n {
                    p = n;
                }
            }
        }
        Ok(())
    }
}

fn is_single_dot(b: &[u8]) -> bool {
    b == b"." || b.eq_ignore_ascii_case(b"%2e")
}

fn is_double_dot(b: &[u8]) -> bool {
    match b.len() {
        2 => b == b"..",
        4 => {
            (b[0] == b'.' && b[1..].eq_ignore_ascii_case(b"%2e"))
                || (b[..3].eq_ignore_ascii_case(b"%2e") && b[3] == b'.')
        }
        6 => b.eq_ignore_ascii_case(b"%2e%2e"),
        _ => false,
    }
}

/// The end of the run of bytes from `from` that `keep` accepts.
#[inline]
fn run_end(s: &[u8], from: usize, keep: impl Fn(u8) -> bool) -> usize {
    s[from..]
        .iter()
        .position(|&c| !keep(c))
        .map_or(s.len(), |i| from + i)
}

/// An ASCII URL code point (these need no check, and no percent-encoding
/// in the fragment and opaque path sets).
#[inline]
pub(crate) fn is_urlcp(c: u8) -> bool {
    c < 0x80 && SETS[c as usize] & URLCP != 0
}
