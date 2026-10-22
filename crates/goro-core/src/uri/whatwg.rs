//! WHATWG URLs: a port of lexbor's URL parser (ext/lexbor/lexbor/url/url.c)
//! as bundled with php-src: the basic URL parser with its state overrides
//! (used by the API setters), host parsing (domains, IPv4, IPv6, opaque
//! hosts), percent-encoding, serialization and the validation error log.
//!
//! lexbor strings distinguish NULL from empty; that is kept where it is
//! observable (path, query, fragment). Positions are offsets into the
//! buffer being parsed; a logged error keeps lexbor's "context" (the
//! NUL-terminated text from the error position on, which ext/uri exposes).

#![allow(clippy::result_unit_err)]

use super::idna;

/// lxb_url_error_type_t
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorType {
    DomainToAscii,
    DomainToUnicode,
    DomainInvalidCodePoint,
    HostInvalidCodePoint,
    Ipv4EmptyPart,
    Ipv4TooManyParts,
    Ipv4NonNumericPart,
    Ipv4NonDecimalPart,
    Ipv4OutOfRangePart,
    Ipv6Unclosed,
    Ipv6InvalidCompression,
    Ipv6TooManyPieces,
    Ipv6MultipleCompression,
    Ipv6InvalidCodePoint,
    Ipv6TooFewPieces,
    Ipv4InIpv6TooManyPieces,
    Ipv4InIpv6InvalidCodePoint,
    Ipv4InIpv6OutOfRangePart,
    Ipv4InIpv6TooFewParts,
    InvalidUrlUnit,
    SpecialSchemeMissingFollowingSolidus,
    MissingSchemeNonRelativeUrl,
    InvalidReverseSolidus,
    InvalidCredentials,
    HostMissing,
    PortOutOfRange,
    PortInvalid,
    FileInvalidWindowsDriveLetter,
    FileInvalidWindowsDriveLetterHost,
}

impl ErrorType {
    /// The Uri\WhatWg\UrlValidationErrorType case and the `failure` flag
    /// (uri_parser_whatwg.c fill_errors()).
    pub fn php_case(self) -> (&'static str, bool) {
        use ErrorType::*;
        match self {
            DomainToAscii => ("DomainToAscii", true),
            DomainToUnicode => ("DomainToUnicode", false),
            DomainInvalidCodePoint => ("DomainInvalidCodePoint", true),
            HostInvalidCodePoint => ("HostInvalidCodePoint", true),
            Ipv4EmptyPart => ("Ipv4EmptyPart", false),
            Ipv4TooManyParts => ("Ipv4TooManyParts", true),
            Ipv4NonNumericPart => ("Ipv4NonNumericPart", true),
            Ipv4NonDecimalPart => ("Ipv4NonDecimalPart", false),
            Ipv4OutOfRangePart => ("Ipv4OutOfRangePart", true),
            Ipv6Unclosed => ("Ipv6Unclosed", true),
            Ipv6InvalidCompression => ("Ipv6InvalidCompression", true),
            Ipv6TooManyPieces => ("Ipv6TooManyPieces", true),
            Ipv6MultipleCompression => ("Ipv6MultipleCompression", true),
            Ipv6InvalidCodePoint => ("Ipv6InvalidCodePoint", true),
            Ipv6TooFewPieces => ("Ipv6TooFewPieces", true),
            Ipv4InIpv6TooManyPieces => ("Ipv4InIpv6TooManyPieces", true),
            Ipv4InIpv6InvalidCodePoint => ("Ipv4InIpv6InvalidCodePoint", true),
            Ipv4InIpv6OutOfRangePart => ("Ipv4InIpv6OutOfRangePart", true),
            Ipv4InIpv6TooFewParts => ("Ipv4InIpv6TooFewParts", true),
            InvalidUrlUnit => ("InvalidUrlUnit", false),
            SpecialSchemeMissingFollowingSolidus => ("SpecialSchemeMissingFollowingSolidus", false),
            MissingSchemeNonRelativeUrl => ("MissingSchemeNonRelativeUrl", true),
            InvalidReverseSolidus => ("InvalidReverseSoldius", false),
            InvalidCredentials => ("InvalidCredentials", false),
            HostMissing => ("HostMissing", true),
            PortOutOfRange => ("PortOutOfRange", true),
            PortInvalid => ("PortInvalid", true),
            FileInvalidWindowsDriveLetter => ("FileInvalidWindowsDriveLetter", false),
            FileInvalidWindowsDriveLetterHost => ("FileInvalidWindowsDriveLetterHost", false),
        }
    }
}

/// One entry of the parser log (lexbor_plog_entry_t): the error and the
/// text at its position.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub ty: ErrorType,
    pub context: Vec<u8>,
}

/// lxb_url_scheme_type_t
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SchemeType {
    #[default]
    Undef,
    Unknown,
    Http,
    Https,
    Ws,
    Wss,
    Ftp,
    File,
}

impl SchemeType {
    fn default_port(self) -> u16 {
        match self {
            SchemeType::Http | SchemeType::Ws => 80,
            SchemeType::Https | SchemeType::Wss => 443,
            SchemeType::Ftp => 21,
            _ => 0,
        }
    }
    /// lxb_url_scheme_is_special(): everything but UNKNOWN (UNDEF included)
    fn is_special(self) -> bool {
        self != SchemeType::Unknown
    }
}

const SCHEMES: [(&[u8], SchemeType); 6] = [
    (b"http", SchemeType::Http),
    (b"https", SchemeType::Https),
    (b"ws", SchemeType::Ws),
    (b"wss", SchemeType::Wss),
    (b"ftp", SchemeType::Ftp),
    (b"file", SchemeType::File),
];

/// lxb_url_scheme_find()
fn scheme_find(s: &[u8]) -> SchemeType {
    for (name, t) in SCHEMES {
        if name.eq_ignore_ascii_case(s) {
            return t;
        }
    }
    SchemeType::Unknown
}

/// lxb_url_host_t
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Host {
    #[default]
    Undef,
    Domain(Vec<u8>),
    Opaque(Vec<u8>),
    Ipv4(u32),
    Ipv6([u16; 8]),
    Empty,
}

/// lxb_url_path_t
#[derive(Clone, Debug, Default)]
pub struct Path {
    /// None: NULL data
    pub str: Option<Vec<u8>>,
    /// The number of segments
    pub length: usize,
    pub opaque: bool,
}

impl Path {
    pub fn bytes(&self) -> &[u8] {
        self.str.as_deref().unwrap_or(b"")
    }
    fn len(&self) -> usize {
        self.str.as_ref().map_or(0, |s| s.len())
    }
}

/// lxb_url_t
#[derive(Clone, Debug, Default)]
pub struct Url {
    pub scheme_name: Vec<u8>,
    pub scheme_type: SchemeType,
    pub host: Host,
    pub username: Vec<u8>,
    pub password: Vec<u8>,
    pub port: u16,
    pub has_port: bool,
    pub path: Path,
    pub query: Option<Vec<u8>>,
    pub fragment: Option<Vec<u8>>,
}

impl Url {
    fn is_special(&self) -> bool {
        self.scheme_type.is_special()
    }
    /// lxb_url_includes_credentials()
    pub fn includes_credentials(&self) -> bool {
        !self.username.is_empty() || !self.password.is_empty()
    }
    /// lxb_url_cannot_have_user_pass_port()
    fn cannot_have_user_pass_port(&self) -> bool {
        matches!(self.host, Host::Empty | Host::Undef) || self.scheme_type == SchemeType::File
    }

    /// lxb_url_path_set_null()
    fn path_set_null(&mut self) {
        if self.path.str.is_none() {
            return;
        }
        self.path.str = None;
        self.path.length = 0;
        self.path.opaque = false;
    }

    /// lxb_url_path_copy()
    fn path_copy_from(&mut self, src: &Url) {
        if self.path.str.is_some() {
            self.path_set_null();
        }
        self.path.opaque = src.path.opaque;
        if src.path.str.is_none() {
            return;
        }
        self.path.length = src.path.length;
        self.path.str = src.path.str.clone();
    }

    /// lxb_url_path_shorten()
    fn path_shorten(&mut self) {
        if self.scheme_type == SchemeType::File && self.path.length == 1 {
            let s = self.path.bytes();
            if s.len() >= 3 && s[1].is_ascii_alphabetic() && s[2] == b':' {
                return;
            }
        }
        if let Some(s) = &mut self.path.str {
            self.path.length = self.path.length.wrapping_sub(1);
            let mut p = s.len();
            while p > 0 {
                p -= 1;
                if s[p] == b'/' {
                    break;
                }
            }
            s.truncate(p);
        }
    }

    /// lxb_url_path_append_wo_slash()
    fn path_append_wo_slash(&mut self, data: &[u8]) {
        self.path
            .str
            .get_or_insert_with(Vec::new)
            .extend_from_slice(data);
    }

    /// lxb_url_path_append(): "/" and the segment.
    fn path_append(&mut self, data: &[u8]) {
        let s = self.path.str.get_or_insert_with(Vec::new);
        s.push(b'/');
        s.extend_from_slice(data);
    }

    /// lxb_url_scheme_copy() etc. for the relative states
    fn copy_scheme_from(&mut self, base: &Url) {
        self.scheme_type = base.scheme_type;
        self.scheme_name = base.scheme_name.clone();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Undef,
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

const OPT_NOT_SPECIAL: u8 = 1;
const OPT_DECODE: u8 = 2;
const OPT_IDNA: u8 = 4;

// lxb_url_map_type_t
const MAP_C0: u8 = 0x01;
const MAP_FRAGMENT: u8 = 0x02;
const MAP_QUERY: u8 = 0x04;
const MAP_SPECIAL_QUERY: u8 = 0x08;
const MAP_PATH: u8 = 0x10;
const MAP_USERINFO: u8 = 0x20;
const MAP_COMPONENT: u8 = 0x40;
const MAP_X_WWW_FORM: u8 = 0x80;

/// lxb_url_map[]: the percent-encode sets each byte belongs to.
fn url_map(c: u8) -> u8 {
    const UFQSPCX: u8 = MAP_USERINFO
        | MAP_FRAGMENT
        | MAP_QUERY
        | MAP_SPECIAL_QUERY
        | MAP_PATH
        | MAP_COMPONENT
        | MAP_X_WWW_FORM;
    const UCX: u8 = MAP_USERINFO | MAP_COMPONENT | MAP_X_WWW_FORM;
    match c {
        0x00..=0x1f | 0x7f..=0xff => 0xff,
        b' ' | b'"' | b'<' | b'>' => UFQSPCX,
        b'#' => UFQSPCX & !MAP_FRAGMENT,
        b'!' | b'%' | b'(' | b')' | b'~' => MAP_X_WWW_FORM,
        b'$' | b'&' | b'+' | b',' => MAP_COMPONENT | MAP_X_WWW_FORM,
        b'\'' => MAP_SPECIAL_QUERY | MAP_X_WWW_FORM,
        b'/' | b':' | b';' | b'=' | b'@' | b'[' | b'\\' | b']' | b'^' | b'|' => UCX,
        b'?' => MAP_PATH | UCX,
        b'`' => MAP_PATH | MAP_FRAGMENT | UCX,
        b'{' | b'}' => MAP_PATH | UCX,
        _ => 0,
    }
}

/// lxb_url_path_map[]: 0 = plain, else the fast path stops there.
fn path_map(c: u8) -> u8 {
    match c {
        0x00..=0x1f | 0x7f..=0xff => 2,
        b'[' | b']' | b'^' | b'|' => 2,
        b' ' | b'"' | b'#' | b'.' | b'/' | b'<' | b'>' | b'?' | b'\\' | b'`' | b'{' | b'}' => 1,
        b'%' => 4,
        _ => 0,
    }
}

/// lxb_url_map_forbidden_domain_cp[]
fn forbidden_domain_cp(c: u8) -> bool {
    c <= 0x20
        || matches!(
            c,
            b'#' | b'%'
                | b'/'
                | b':'
                | b'<'
                | b'>'
                | b'?'
                | b'@'
                | b'['
                | b'\\'
                | b']'
                | b'^'
                | b'|'
                | 0x7f
        )
}

/// lxb_url_map_forbidden_host_cp[]
fn forbidden_host_cp(c: u8) -> bool {
    matches!(
        c,
        0x00 | 0x09
            | 0x0a
            | 0x0d
            | b' '
            | b'#'
            | b'/'
            | b':'
            | b'<'
            | b'>'
            | b'?'
            | b'@'
            | b'['
            | b'\\'
            | b']'
            | b'^'
            | b'|'
    )
}

/// lxb_url_codepoint_alphanumeric[] (code points below U+00A0)
fn codepoint_alphanumeric(c: u32) -> bool {
    let c = c as u8;
    if c >= 0x80 {
        return false;
    }
    c.is_ascii_alphanumeric()
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
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn is_hex(c: u8) -> bool {
    hex_val(c).is_some()
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

fn push_pct(out: &mut Vec<u8>, c: u8) {
    out.push(b'%');
    out.push(HEX_UPPER[(c >> 4) as usize]);
    out.push(HEX_UPPER[(c & 15) as usize]);
}

/// lxb_url_is_noncharacter()
fn is_noncharacter(cp: u32) -> bool {
    (0xFDD0..=0xFDEF).contains(&cp) || (cp & 0xFFFE) == 0xFFFE && cp <= 0x10FFFF
}

/// lxb_url_is_url_codepoint()
fn is_url_codepoint(cp: u32) -> bool {
    if (0x00A0..=0x1FFFFF).contains(&cp) {
        if (0xD800..=0xDFFF).contains(&cp) {
            return false;
        }
        return !(cp > 0x10FFFF || is_noncharacter(cp));
    }
    codepoint_alphanumeric(cp)
}

/// LXB_ENCODING_DECODE_ERROR
pub const DECODE_ERROR: u32 = 0x1FFFFF;

/// lxb_encoding_decode_valid_utf_8_single(): one code point at `*p`
/// (advancing it), DECODE_ERROR on a truncated or invalid lead byte.
pub fn decode_valid_utf8_single(data: &[u8], p: &mut usize, end: usize) -> u32 {
    let b = data[*p];
    let need = if b < 0x80 {
        1
    } else if b & 0xe0 == 0xc0 {
        2
    } else if b & 0xf0 == 0xe0 {
        3
    } else if b & 0xf8 == 0xf0 {
        4
    } else {
        *p += 1;
        return DECODE_ERROR;
    };
    if end - *p < need {
        *p = end;
        return DECODE_ERROR;
    }
    let s = &data[*p..*p + need];
    let cp = match need {
        1 => s[0] as u32,
        2 => ((s[0] as u32 & 0x1f) << 6) | (s[1] as u32 & 0x3f),
        3 => ((s[0] as u32 & 0x0f) << 12) | ((s[1] as u32 & 0x3f) << 6) | (s[2] as u32 & 0x3f),
        _ => {
            ((s[0] as u32 & 0x07) << 18)
                | ((s[1] as u32 & 0x3f) << 12)
                | ((s[2] as u32 & 0x3f) << 6)
                | (s[3] as u32 & 0x3f)
        }
    };
    *p += need;
    cp
}

/// lxb_encoding_decode_valid_utf_8_single_reverse(): the code point ending
/// at `*end` (moving it back), DECODE_ERROR when malformed.
fn decode_valid_utf8_single_reverse(data: &[u8], end: &mut usize, begin: usize) -> u32 {
    let mut p = *end;
    while p > begin {
        p -= 1;
        let b = data[p];
        let need = if b < 0x80 {
            *end = p;
            return b as u32;
        } else if b & 0xe0 == 0xc0 {
            2
        } else if b & 0xf0 == 0xe0 {
            3
        } else if b & 0xf8 == 0xf0 {
            4
        } else if *end - p >= 4 {
            break;
        } else {
            continue;
        };
        if *end - p < need {
            *end = p;
            return DECODE_ERROR;
        }
        let mut q = p;
        let cp = decode_valid_utf8_single(data, &mut q, data.len());
        *end = p;
        return cp;
    }
    *end = p;
    DECODE_ERROR
}

/// The text a log entry shows: the buffer from `pos` to its NUL.
fn context_of(buf: &[u8], pos: usize) -> Vec<u8> {
    let rest = buf.get(pos..).unwrap_or(&[]);
    match rest.iter().position(|&b| b == 0) {
        Some(n) => rest[..n].to_vec(),
        None => rest.to_vec(),
    }
}

/// lxb_url_percent_encode_after_utf_8(): append the percent-encoded bytes.
fn percent_encode_after_utf8(data: &[u8], out: &mut Vec<u8>, set: u8) {
    for &c in data {
        if url_map(c) & set != 0 {
            push_pct(out, c);
        } else {
            out.push(c);
        }
    }
}

fn is_windows_letter(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

/// lxb_url_start_windows_drive_letter()
fn starts_windows_drive_letter(d: &[u8]) -> bool {
    if d.len() < 2 {
        return false;
    }
    if !is_windows_letter(d[0]) || (d[1] != b':' && d[1] != b'|') {
        return false;
    }
    if d.len() > 2 && !matches!(d[2], b'/' | b'\\' | b'?' | b'#') {
        return false;
    }
    true
}

/// lxb_url_windows_drive_letter()
fn windows_drive_letter(d: &[u8]) -> bool {
    d.len() >= 2 && is_windows_letter(d[0]) && (d[1] == b':' || d[1] == b'|')
}

/// lxb_url_normalized_windows_drive_letter()
fn normalized_windows_drive_letter(d: &[u8]) -> bool {
    d.len() >= 2 && is_windows_letter(d[0]) && d[1] == b':'
}

/// lxb_url_parser_t: the error log.
#[derive(Default)]
pub struct Parser {
    pub log: Vec<LogEntry>,
}

type St = Result<(), ()>;

impl Parser {
    fn log(&mut self, buf: &[u8], pos: usize, ty: ErrorType) {
        self.log.push(LogEntry {
            ty,
            context: context_of(buf, pos),
        });
    }

    /// lxb_url_parse(): None on failure.
    pub fn parse(&mut self, base: Option<&Url>, input: &[u8]) -> Option<Url> {
        let mut url = Url::default();
        self.parse_basic_h(&mut url, true, base, input, State::Undef)
            .ok()?;
        Some(url)
    }

    /// lxb_url_leading_trailing(): the range without leading and trailing
    /// C0 controls and spaces.
    fn leading_trailing(&mut self, data: &[u8]) -> (usize, usize) {
        let mut p = 0usize;
        let mut end = data.len();
        while p < end {
            let c = data[p];
            if c > 0x1f && c < 0x80 && c != 0x20 {
                break;
            } else if c >= 0x80 {
                let tmp = p;
                let cp = decode_valid_utf8_single(data, &mut p, end);
                if cp > 0x1f && cp != DECODE_ERROR {
                    p = tmp;
                    break;
                }
                continue;
            }
            p += 1;
        }
        if p != 0 {
            self.log(data, 0, ErrorType::InvalidUrlUnit);
        }
        while end > p {
            let tmp = end;
            let cp = decode_valid_utf8_single_reverse(data, &mut end, p);
            if cp > 0x1f && cp != 0x20 && cp != DECODE_ERROR {
                end = tmp;
                break;
            }
        }
        if end != data.len() {
            self.log(data, end, ErrorType::InvalidUrlUnit);
        }
        (p, end)
    }

    /// lxb_url_parse_basic_h(). `fresh`: `url` is new (lxb_url_parse()),
    /// so leading and trailing C0 controls and spaces are trimmed.
    pub fn parse_basic_h(
        &mut self,
        url: &mut Url,
        fresh: bool,
        base: Option<&Url>,
        input: &[u8],
        override_state: State,
    ) -> St {
        let (start, end) = if fresh {
            self.leading_trailing(input)
        } else {
            (0, input.len())
        };
        // lxb_url_remove_tab_newline()
        let owned;
        let (buf, start, end): (&[u8], usize, usize) = match input[start..end]
            .iter()
            .position(|&c| c == b'\n' || c == b'\r' || c == b'\t')
        {
            Some(i) => {
                self.log(input, start + i, ErrorType::InvalidUrlUnit);
                owned = input[start..end]
                    .iter()
                    .copied()
                    .filter(|&c| c != b'\n' && c != b'\r' && c != b'\t')
                    .collect::<Vec<u8>>();
                (&owned, 0, owned.len())
            }
            None => (input, start, end),
        };
        let has_override = override_state != State::Undef;
        let mut state = if has_override {
            override_state
        } else {
            State::SchemeStart
        };
        let mut inside_bracket = false;
        let mut p = start;
        let data = start;
        let c_at = |i: usize| -> u8 { if i < end { buf[i] } else { 0 } };

        loop {
            match state {
                State::SchemeStart | State::Scheme => {
                    if state == State::SchemeStart && (p >= end || !buf[p].is_ascii_alphabetic()) {
                        if !has_override {
                            state = State::NoScheme;
                            continue;
                        }
                        return Err(());
                    }
                    loop {
                        p += 1;
                        if !(p < end
                            && (buf[p].is_ascii_alphanumeric()
                                || matches!(buf[p], b'+' | b'-' | b'.')))
                        {
                            break;
                        }
                    }
                    if p >= end || buf[p] != b':' {
                        if !has_override {
                            p = data;
                            state = State::NoScheme;
                            continue;
                        } else if p < end || override_state != State::SchemeStart {
                            return Err(());
                        }
                    }
                    let schm = scheme_find(&buf[data..p.min(end)]);
                    if has_override {
                        if url.is_special() != schm.is_special() {
                            return Ok(());
                        }
                        if (url.has_port || url.includes_credentials()) && schm == SchemeType::File
                        {
                            return Ok(());
                        }
                        if url.scheme_type == SchemeType::File && url.host == Host::Empty {
                            return Ok(());
                        }
                    }
                    url.scheme_type = schm;
                    url.scheme_name = buf[data..p.min(end)].to_ascii_lowercase();
                    p += 1;
                    if has_override {
                        if url.has_port && url.port == schm.default_port() {
                            url.port = 0;
                            url.has_port = false;
                        }
                        return Ok(());
                    }
                    if schm == SchemeType::File {
                        if end - p < 2 || buf[p] != b'/' || buf[p + 1] != b'/' {
                            self.log(buf, p, ErrorType::SpecialSchemeMissingFollowingSolidus);
                        }
                        state = State::File;
                        continue;
                    }
                    if schm.is_special() {
                        state = match base {
                            Some(b) if scheme_is_equal(url, b) => State::SpecialRelativeOrAuthority,
                            _ => State::SpecialAuthoritySlashes,
                        };
                        continue;
                    }
                    if p < end && buf[p] == b'/' {
                        p += 1;
                        state = State::PathOrAuthority;
                        continue;
                    }
                    url.path_set_null();
                    state = State::OpaquePath;
                }
                State::NoScheme => {
                    let Some(b) = base else {
                        self.log(buf, p, ErrorType::MissingSchemeNonRelativeUrl);
                        return Err(());
                    };
                    if b.path.opaque {
                        if p >= end || buf[p] != b'#' {
                            self.log(buf, p, ErrorType::MissingSchemeNonRelativeUrl);
                            return Err(());
                        }
                        p += 1;
                        url.copy_scheme_from(b);
                        url.path_copy_from(b);
                        url.query = b.query.clone();
                        url.fragment = None;
                        state = State::Fragment;
                        continue;
                    }
                    state = if b.scheme_type != SchemeType::File {
                        State::Relative
                    } else {
                        State::File
                    };
                }
                State::SpecialRelativeOrAuthority => {
                    if end - p > 1 && buf[p] == b'/' && buf[p + 1] == b'/' {
                        p += 2;
                        state = State::SpecialAuthorityIgnoreSlashes;
                        continue;
                    }
                    self.log(buf, p, ErrorType::SpecialSchemeMissingFollowingSolidus);
                    state = State::Relative;
                }
                State::PathOrAuthority => {
                    if p < end && buf[p] == b'/' {
                        p += 1;
                        state = State::Authority;
                    } else {
                        state = State::Path;
                    }
                }
                State::Relative => {
                    let b = base.expect("relative state without a base URL");
                    url.copy_scheme_from(b);
                    if end - p >= 1 {
                        if buf[p] == b'/' {
                            p += 1;
                            state = State::RelativeSlash;
                            continue;
                        }
                        if url.is_special() && buf[p] == b'\\' {
                            p += 1;
                            self.log(buf, p, ErrorType::InvalidReverseSolidus);
                            state = State::RelativeSlash;
                            continue;
                        }
                    }
                    url.username = b.username.clone();
                    url.password = b.password.clone();
                    url.host = b.host.clone();
                    if b.has_port {
                        url.port = b.port;
                        url.has_port = true;
                    }
                    url.path_copy_from(b);
                    url.query = b.query.clone();
                    if end - p == 0 {
                        return Ok(());
                    }
                    if buf[p] == b'?' {
                        p += 1;
                        url.query = None;
                        state = State::Query;
                        continue;
                    }
                    if buf[p] == b'#' {
                        p += 1;
                        url.fragment = None;
                        state = State::Fragment;
                        continue;
                    }
                    url.query = None;
                    url.path_shorten();
                    state = State::Path;
                }
                State::RelativeSlash => {
                    let c = if end - p >= 1 { buf[p] } else { 0 };
                    if url.is_special() && (c == b'/' || c == b'\\') {
                        if c == b'\\' {
                            self.log(buf, p, ErrorType::InvalidReverseSolidus);
                        }
                        p += 1;
                        state = State::SpecialAuthorityIgnoreSlashes;
                        continue;
                    }
                    if c == b'/' {
                        p += 1;
                        state = State::Authority;
                        continue;
                    }
                    let b = base.expect("relative slash state without a base URL");
                    url.username = b.username.clone();
                    url.password = b.password.clone();
                    url.host = b.host.clone();
                    if b.has_port {
                        url.port = b.port;
                        url.has_port = true;
                    }
                    state = State::Path;
                }
                State::SpecialAuthoritySlashes => {
                    state = State::SpecialAuthorityIgnoreSlashes;
                    if end - p > 1 && buf[p] == b'/' && buf[p + 1] == b'/' {
                        p += 2;
                    } else {
                        self.log(buf, p, ErrorType::SpecialSchemeMissingFollowingSolidus);
                    }
                }
                State::SpecialAuthorityIgnoreSlashes => {
                    if p >= end || (buf[p] != b'/' && buf[p] != b'\\') {
                        state = State::Authority;
                        continue;
                    }
                    self.log(buf, p, ErrorType::SpecialSchemeMissingFollowingSolidus);
                    p += 1;
                }
                State::Authority => {
                    let mut begin = p;
                    let mut pswd: Option<usize> = None;
                    let mut at_sign = false;
                    while p < end {
                        let c = buf[p];
                        match c {
                            b'@' => {
                                self.log(buf, p, ErrorType::InvalidCredentials);
                                if p == begin {
                                    at_sign = true;
                                    p += 1;
                                    continue;
                                }
                                if pswd.is_none() || !at_sign {
                                    let tmp = match pswd {
                                        Some(pw) => pw - 1,
                                        None => p,
                                    };
                                    if tmp > begin {
                                        percent_encode_after_utf8(
                                            &buf[begin..tmp],
                                            &mut url.username,
                                            MAP_USERINFO,
                                        );
                                    }
                                }
                                if let Some(pw) = pswd {
                                    if p > pw {
                                        percent_encode_after_utf8(
                                            &buf[pw..p],
                                            &mut url.password,
                                            MAP_USERINFO,
                                        );
                                        pswd = Some(p);
                                    }
                                }
                                at_sign = true;
                                begin = p;
                            }
                            b':' => {
                                if pswd.is_none() {
                                    pswd = Some(p + 1);
                                }
                            }
                            b'/' | b'\\' | b'?' | b'#' => {
                                if c == b'\\' && !url.is_special() {
                                    p += 1;
                                    continue;
                                }
                                break;
                            }
                            _ => {}
                        }
                        p += 1;
                    }
                    if at_sign {
                        if begin == p || begin + 1 == p {
                            self.log(buf, p, ErrorType::InvalidCredentials);
                            return Err(());
                        }
                        begin += 1;
                    }
                    p = begin;
                    state = State::Host;
                }
                State::Host | State::Hostname => {
                    let begin = p;
                    if has_override && url.scheme_type == SchemeType::File {
                        state = State::FileHost;
                        continue;
                    }
                    let mut opt = if !url.is_special() {
                        OPT_NOT_SPECIAL
                    } else {
                        0
                    };
                    let mut to_port = false;
                    while p < end {
                        let c = buf[p];
                        match c {
                            b'/' | b'?' | b'#' => break,
                            b'\\' => {
                                if url.is_special() {
                                    break;
                                }
                            }
                            b':' => {
                                if !inside_bracket {
                                    if p == begin {
                                        self.log(buf, p, ErrorType::HostMissing);
                                        return Err(());
                                    }
                                    if override_state == State::Hostname {
                                        return Err(());
                                    }
                                    let host = self.host_parse(buf, begin, p, opt)?;
                                    url.host = host;
                                    p += 1;
                                    to_port = true;
                                    break;
                                }
                            }
                            b'[' => inside_bracket = true,
                            b']' => inside_bracket = false,
                            b'%' => opt |= OPT_DECODE,
                            b'X' | b'x' => {
                                if p + 4 <= end
                                    && (buf[p + 1] == b'n' || buf[p + 1] == b'N')
                                    && buf[p + 2] == b'-'
                                    && buf[p + 3] == b'-'
                                {
                                    opt |= OPT_IDNA;
                                    p += 3;
                                }
                            }
                            _ => {
                                if c >= 0x80 {
                                    opt |= OPT_IDNA;
                                }
                            }
                        }
                        p += 1;
                    }
                    if to_port {
                        state = State::Port;
                        continue;
                    }
                    if begin == p && url.is_special() {
                        self.log(buf, p, ErrorType::HostMissing);
                        return Err(());
                    }
                    if has_override && begin == p && (url.includes_credentials() || url.has_port) {
                        return Ok(());
                    }
                    let host = self.host_parse(buf, begin, p, opt)?;
                    url.host = host;
                    if has_override {
                        return Ok(());
                    }
                    state = State::PathStart;
                }
                State::Port => {
                    let mut begin = p;
                    loop {
                        let c = c_at(p);
                        if p < end && c.is_ascii_digit() {
                            p += 1;
                            continue;
                        }
                        if p >= end
                            || c == b'/'
                            || c == b'?'
                            || c == b'#'
                            || (url.is_special() && c == b'\\')
                            || has_override
                        {
                            if begin == p {
                                if has_override {
                                    return Err(());
                                }
                                state = State::PathStart;
                                break;
                            }
                            let mut port: u32 = 0;
                            while begin < p {
                                port = (buf[begin] - b'0') as u32 + port * 10;
                                begin += 1;
                                if port > 65535 {
                                    self.log(buf, p, ErrorType::PortOutOfRange);
                                    return Err(());
                                }
                            }
                            if !url.is_special() || url.scheme_type.default_port() != port as u16 {
                                url.port = port as u16;
                                url.has_port = true;
                            } else {
                                url.port = 0;
                                url.has_port = false;
                            }
                            if has_override {
                                return Ok(());
                            }
                            state = State::PathStart;
                            break;
                        }
                        self.log(buf, p, ErrorType::PortInvalid);
                        return Err(());
                    }
                }
                State::File => {
                    url.scheme_type = SchemeType::File;
                    url.scheme_name = b"file".to_vec();
                    url.host = Host::Empty;
                    let c = c_at(p);
                    if c == b'/' || c == b'\\' {
                        if c == b'\\' {
                            self.log(buf, p, ErrorType::InvalidReverseSolidus);
                        }
                        p += 1;
                        state = State::FileSlash;
                        continue;
                    }
                    if let Some(b) = base.filter(|b| b.scheme_type == SchemeType::File) {
                        url.host = b.host.clone();
                        url.path_copy_from(b);
                        url.query = b.query.clone();
                        if c == b'?' && p < end {
                            p += 1;
                            url.query = None;
                            state = State::Query;
                            continue;
                        }
                        if c == b'#' && p < end {
                            p += 1;
                            url.fragment = None;
                            state = State::Fragment;
                            continue;
                        }
                        if p >= end {
                            return Ok(());
                        }
                        url.query = None;
                        if !starts_windows_drive_letter(&buf[p..end]) {
                            url.path_shorten();
                        } else {
                            self.log(buf, p, ErrorType::FileInvalidWindowsDriveLetter);
                            url.path_set_null();
                            url.path.opaque = true;
                        }
                    }
                    state = State::Path;
                }
                State::FileSlash => {
                    let c = c_at(p);
                    if c == b'/' || c == b'\\' {
                        if c == b'\\' {
                            self.log(buf, p, ErrorType::InvalidReverseSolidus);
                        }
                        p += 1;
                        state = State::FileHost;
                        continue;
                    }
                    if let Some(b) = base.filter(|b| b.scheme_type == SchemeType::File) {
                        url.host = b.host.clone();
                        if !starts_windows_drive_letter(&buf[p..end])
                            && !b.path.opaque
                            && b.path.length >= 1
                        {
                            let bp = b.path.bytes();
                            let idx = (bp.first() == Some(&b'/')) as usize;
                            if let Some((off, len)) = path_part_by_index(bp, idx) {
                                if normalized_windows_drive_letter(&bp[off..off + len]) {
                                    let l = off + len;
                                    url.path_append_wo_slash(&bp[..l]);
                                }
                            }
                        }
                    }
                    state = State::Path;
                }
                State::FileHost => {
                    let begin = p;
                    let mut opt = if !url.is_special() {
                        OPT_NOT_SPECIAL
                    } else {
                        0
                    };
                    while p < end {
                        match buf[p] {
                            b'/' | b'\\' | b'?' | b'#' => break,
                            b'%' => {
                                p += 1;
                                opt |= OPT_DECODE;
                            }
                            b'X' | b'x' => {
                                if p + 4 <= end
                                    && (buf[p + 1] == b'n' || buf[p + 1] == b'N')
                                    && buf[p + 2] == b'-'
                                    && buf[p + 3] == b'-'
                                {
                                    opt |= OPT_IDNA;
                                    p += 3;
                                }
                                p += 1;
                            }
                            c => {
                                if c >= 0x80 {
                                    opt |= OPT_IDNA;
                                }
                                p += 1;
                            }
                        }
                    }
                    let p_end = p.min(end);
                    if !has_override
                        && p_end - begin == 2
                        && windows_drive_letter(&buf[begin..p_end])
                    {
                        self.log(buf, begin, ErrorType::FileInvalidWindowsDriveLetterHost);
                        p = begin;
                        state = State::Path;
                        continue;
                    }
                    if p == begin {
                        url.host = Host::Empty;
                        if has_override {
                            return Ok(());
                        }
                        state = State::PathStart;
                        continue;
                    }
                    let host = self.host_parse(buf, begin, p_end, opt)?;
                    url.host = host;
                    if let Host::Domain(d) | Host::Opaque(d) = &url.host {
                        if d.as_slice() == b"localhost" {
                            url.host = Host::Empty;
                        }
                    }
                    if has_override {
                        return Ok(());
                    }
                    p = p_end;
                    state = State::PathStart;
                }
                State::PathStart => {
                    let c = c_at(p);
                    if url.is_special() {
                        if c == b'\\' && p < end {
                            self.log(buf, p, ErrorType::InvalidReverseSolidus);
                            p += 1;
                        } else if c == b'/' && p < end {
                            p += 1;
                        }
                        state = State::Path;
                        continue;
                    }
                    if !has_override {
                        if c == b'?' && p < end {
                            p += 1;
                            state = State::Query;
                            continue;
                        }
                        if c == b'#' && p < end {
                            p += 1;
                            state = State::Fragment;
                            continue;
                        }
                    }
                    if p < end {
                        if c == b'/' {
                            p += 1;
                        }
                        state = State::Path;
                        continue;
                    }
                    if has_override && url.host == Host::Undef {
                        url.path_append(b"");
                    }
                    return Ok(());
                }
                State::Path => {
                    p = self.path_fast_path(url, buf, p, end, !has_override);
                    if p >= end {
                        return Ok(());
                    }
                    match buf[p] {
                        b'?' => {
                            p += 1;
                            state = State::Query;
                        }
                        b'#' => {
                            p += 1;
                            state = State::Fragment;
                        }
                        _ => {}
                    }
                }
                State::OpaquePath => {
                    let begin = p;
                    url.path.opaque = true;
                    loop {
                        if p >= end {
                            let mut tmp = Vec::new();
                            percent_encode_after_utf8(&buf[begin..end], &mut tmp, MAP_C0);
                            url.path_append_wo_slash(&tmp);
                            return Ok(());
                        }
                        let c = buf[p];
                        if c == b'#' || c == b'?' {
                            let mut tmp = Vec::new();
                            percent_encode_after_utf8(&buf[begin..p], &mut tmp, MAP_C0);
                            url.path_append_wo_slash(&tmp);
                            p += 1;
                            state = if c == b'#' {
                                State::Fragment
                            } else {
                                State::Query
                            };
                            break;
                        }
                        self.check_url_unit(buf, &mut p, end);
                    }
                }
                State::Query => {
                    let begin = p;
                    loop {
                        if p >= end || (!has_override && buf[p] == b'#') {
                            let set = if url.is_special() {
                                MAP_SPECIAL_QUERY
                            } else {
                                MAP_QUERY
                            };
                            let q = url.query.get_or_insert_with(Vec::new);
                            percent_encode_after_utf8(&buf[begin..p.min(end)], q, set);
                            if p < end {
                                p += 1;
                                state = State::Fragment;
                                break;
                            }
                            return Ok(());
                        }
                        self.check_url_unit(buf, &mut p, end);
                    }
                }
                State::Fragment => {
                    let begin = p;
                    while p < end {
                        self.check_url_unit(buf, &mut p, end);
                    }
                    let f = url.fragment.get_or_insert_with(Vec::new);
                    percent_encode_after_utf8(&buf[begin..end.max(begin)], f, MAP_FRAGMENT);
                    return Ok(());
                }
                State::Undef => return Err(()),
            }
        }
    }

    /// The code point check of the opaque path, query and fragment states:
    /// decode one code point at `*p` and log an invalid URL unit.
    fn check_url_unit(&mut self, buf: &[u8], p: &mut usize, end: usize) {
        let tmp = *p;
        let cp = decode_valid_utf8_single(buf, p, end);
        let q = *p;
        if (!is_url_codepoint(cp) && cp != b'%' as u32)
            || (cp == b'%' as u32 && (end - q < 2 || !is_hex(buf[q]) || !is_hex(buf[q + 1])))
        {
            self.log(buf, tmp, ErrorType::InvalidUrlUnit);
        }
    }

    /// lxb_url_path_fast_path()
    fn path_fast_path(
        &mut self,
        url: &mut Url,
        buf: &[u8],
        mut p: usize,
        end: usize,
        bqs: bool,
    ) -> usize {
        let mut begin = p;
        let mut last = p;
        let mut count = url.path.length;
        let special = url.is_special();
        while p < end {
            let c = buf[p];
            let x = path_map(c);
            if x != 0 {
                if c == b'/' {
                    count += 1;
                    last = p + 1;
                } else if c == b'%' {
                    if end - p < 3 || !is_hex(buf[p + 1]) || !is_hex(buf[p + 2]) {
                        self.log(buf, p, ErrorType::InvalidUrlUnit);
                        p = if end - p < 3 { end - 1 } else { p + 2 };
                    } else if buf[p + 1] == b'2'
                        && (buf[p + 2] == b'e' || buf[p + 2] == b'E')
                        && (p == begin || buf[p - 1] == b'/' || (buf[p - 1] == b'\\' && special))
                    {
                        url.path.length = count;
                        path_try_dot(url, buf, &mut begin, &mut last, &mut p, end, bqs);
                        count = url.path.length;
                    } else {
                        p += 2;
                    }
                } else if (c == b'?' || c == b'#') && bqs {
                    break;
                } else if c == b'\\' && special {
                    count += 1;
                    self.log(buf, p, ErrorType::InvalidReverseSolidus);
                    url.path_append(&buf[begin..p]);
                    begin = p + 1;
                    last = p + 1;
                } else if c == b'.' {
                    if p == begin || buf[p - 1] == b'/' || (buf[p - 1] == b'\\' && special) {
                        url.path.length = count;
                        path_try_dot(url, buf, &mut begin, &mut last, &mut p, end, bqs);
                        count = url.path.length;
                    }
                } else {
                    url.path.length = count;
                    if last > begin + 1 {
                        url.path_append(&buf[begin..last - 1]);
                    }
                    return self.path_slow_path(url, buf, last, end, bqs);
                }
            }
            p += 1;
        }
        let pe = p.min(end);
        url.path_append(&buf[begin.min(pe)..pe]);
        if count == 0 || p != begin {
            count += 1;
        }
        url.path.length = count;
        p
    }

    /// lxb_url_path_slow_path()
    fn path_slow_path(
        &mut self,
        url: &mut Url,
        buf: &[u8],
        data: usize,
        end: usize,
        bqs: bool,
    ) -> usize {
        let special = url.is_special();
        let mut p = data;
        let mut sbuf: Vec<u8> = Vec::new();
        if url.path.len() != 0 {
            if let Some(s) = &mut url.path.str {
                sbuf.extend_from_slice(s);
                s.clear();
            }
        }
        sbuf.push(b'/');
        let mut last = sbuf.len();
        let mut count = url.path.length;
        while p < end {
            let c = buf[p];
            if c >= 0x80 {
                let tmp = p;
                let cp = decode_valid_utf8_single(buf, &mut p, end);
                if !is_url_codepoint(cp) {
                    self.log(buf, tmp, ErrorType::InvalidUrlUnit);
                }
                for &b in &buf[tmp..p] {
                    push_pct(&mut sbuf, b);
                }
                continue;
            }
            if c == b'/' || (c == b'\\' && special) {
                if c == b'\\' {
                    self.log(buf, p, ErrorType::InvalidReverseSolidus);
                }
                sbuf.push(b'/');
                path_fix_windows_drive(url, &mut sbuf, last, count);
                count += 1;
                last = sbuf.len();
                if p + 1 >= end {
                    count += 1;
                }
            } else if (c == b'?' || c == b'#') && bqs {
                path_fix_windows_drive(url, &mut sbuf, last, count);
                count += 1;
                last = sbuf.len();
                break;
            } else if url_map(c) & MAP_PATH != 0 {
                push_pct(&mut sbuf, c);
                self.log(buf, p, ErrorType::InvalidUrlUnit);
            } else if c == b'.' {
                if last == sbuf.len() {
                    let tmp =
                        path_dot_count(url, buf, p, end, &mut sbuf, &mut last, &mut count, bqs);
                    if tmp != p {
                        p = tmp + 1;
                        continue;
                    }
                }
                sbuf.push(b'.');
            } else if c == b'%' {
                if end - p < 3 || !is_hex(buf[p + 1]) || !is_hex(buf[p + 2]) {
                    self.log(buf, p, ErrorType::InvalidUrlUnit);
                } else if buf[p + 1] == b'2'
                    && (buf[p + 2] == b'e' || buf[p + 2] == b'E')
                    && last == sbuf.len()
                {
                    let tmp =
                        path_dot_count(url, buf, p, end, &mut sbuf, &mut last, &mut count, bqs);
                    if tmp != p {
                        p = tmp + 1;
                        continue;
                    }
                }
                sbuf.push(b'%');
            } else {
                if !codepoint_alphanumeric(c as u32) {
                    self.log(buf, p, ErrorType::InvalidUrlUnit);
                }
                sbuf.push(c);
            }
            p += 1;
        }
        if count == 0 || last < sbuf.len() {
            path_fix_windows_drive(url, &mut sbuf, last, count);
            count += 1;
        }
        url.path.length = count;
        url.path_append_wo_slash(&sbuf);
        p
    }

    // ------------------------------------------------------------------
    // Hosts

    /// lxb_url_host_parse()
    fn host_parse(
        &mut self,
        buf: &[u8],
        begin: usize,
        end: usize,
        mut opt: u8,
    ) -> Result<Host, ()> {
        if begin < end && buf[begin] == b'[' {
            if buf[end - 1] != b']' {
                self.log(buf, end - 1, ErrorType::Ipv6Unclosed);
                return Err(());
            }
            if end - 1 <= begin {
                // "[" alone: lexbor parses the empty range [begin + 1, end - 1)
                let mut ip = [0u16; 8];
                self.ipv6_parse(buf, begin + 1, begin + 1, &mut ip)?;
                return Ok(Host::Ipv6(ip));
            }
            let mut ip = [0u16; 8];
            self.ipv6_parse(buf, begin + 1, end - 1, &mut ip)?;
            return Ok(Host::Ipv6(ip));
        }
        if opt & OPT_NOT_SPECIAL != 0 {
            return self.opaque_host_parse(buf, begin, end);
        }
        let input = &buf[begin..end];
        let mut domain: Vec<u8> = if opt & OPT_DECODE != 0 {
            percent_decode(input, &mut opt)
        } else if opt & OPT_IDNA != 0 {
            input.to_vec()
        } else {
            input.to_ascii_lowercase()
        };
        if opt & OPT_IDNA != 0 {
            domain = idna::to_ascii(&domain).map_err(|_| ())?;
        }
        if domain.is_empty() {
            return Err(());
        }
        if let Some(i) = domain
            .iter()
            .position(|&c| c < 128 && forbidden_domain_cp(c))
        {
            self.log(&domain, i, ErrorType::DomainInvalidCodePoint);
            return Err(());
        }
        if is_ipv4(&domain) {
            let ip = self.ipv4_parse(&domain)?;
            return Ok(Host::Ipv4(ip));
        }
        Ok(Host::Domain(domain))
    }

    /// lxb_url_opaque_host_parse()
    fn opaque_host_parse(&mut self, buf: &[u8], begin: usize, end: usize) -> Result<Host, ()> {
        for p in begin..end {
            let c = buf[p];
            if c < 128 && forbidden_host_cp(c) {
                self.log(buf, p, ErrorType::HostInvalidCodePoint);
                return Err(());
            }
        }
        let mut out = Vec::new();
        percent_encode_after_utf8(&buf[begin..end], &mut out, MAP_C0);
        Ok(Host::Opaque(out))
    }

    /// lxb_url_ipv4_append()
    fn ipv4_append(
        &mut self,
        d: &[u8],
        begin: usize,
        end: usize,
        parts: &mut [u64; 5],
        i: usize,
    ) -> St {
        let (n, st) = ipv4_number_parse(&d[begin..end]);
        parts[i] = n;
        match st {
            NumSt::Error => {
                self.log(d, begin, ErrorType::Ipv4NonNumericPart);
                return Err(());
            }
            NumSt::Warning => self.log(d, begin, ErrorType::Ipv4NonDecimalPart),
            NumSt::Ok => {}
        }
        if parts[i] > 255 {
            self.log(d, begin, ErrorType::Ipv4OutOfRangePart);
        }
        Ok(())
    }

    /// lxb_url_ipv4_parse()
    fn ipv4_parse(&mut self, d: &[u8]) -> Result<u32, ()> {
        const ST: [u64; 6] = [0, 256, 65536, 16777216, 4294967296, 0];
        if d.is_empty() {
            return Err(());
        }
        let mut i = 0usize;
        let mut begin = 0usize;
        let mut parts = [0u64; 5];
        let mut p = 0usize;
        while p < d.len() {
            if d[p] == b'.' {
                if i == 4 {
                    self.log(d, begin, ErrorType::Ipv4TooManyParts);
                    return Err(());
                }
                i += 1;
                self.ipv4_append(d, begin, p, &mut parts, i)?;
                begin = p + 1;
            }
            p += 1;
        }
        if begin < p {
            if i == 4 {
                self.log(d, begin, ErrorType::Ipv4TooManyParts);
                return Err(());
            }
            i += 1;
            self.ipv4_append(d, begin, p, &mut parts, i)?;
        } else if d[p - 1] == b'.' {
            self.log(d, begin, ErrorType::Ipv4EmptyPart);
        }
        if parts[i] >= ST[5 - i] {
            return Err(());
        }
        let mut ip = parts[i] as u32;
        i -= 1;
        for (j, part) in parts.iter().enumerate().take(i + 1).skip(1) {
            if *part > 255 {
                return Err(());
            }
            ip = ip.wrapping_add((*part * ST[3 - (j - 1)]) as u32);
        }
        Ok(ip)
    }

    /// lxb_url_ipv6_parse()
    fn ipv6_parse(&mut self, buf: &[u8], data: usize, end: usize, ipv6: &mut [u16; 8]) -> St {
        let mut piece = 0usize;
        let mut compress: Option<usize> = None;
        let mut p = data;
        let err: ErrorType;
        'parse: {
            if p >= end {
                break 'parse;
            }
            if buf[p] == b':' {
                if p + 1 >= end || buf[p + 1] != b':' {
                    if p + 1 < end {
                        p += 1;
                    }
                    err = ErrorType::Ipv6InvalidCompression;
                    self.log(buf, p, err);
                    return Err(());
                }
                p += 2;
                piece += 1;
                compress = Some(piece);
            }
            while p < end {
                if piece == 8 {
                    self.log(buf, p, ErrorType::Ipv6TooManyPieces);
                    return Err(());
                }
                if buf[p] == b':' {
                    if compress.is_some() {
                        self.log(buf, p, ErrorType::Ipv6MultipleCompression);
                        return Err(());
                    }
                    p += 1;
                    piece += 1;
                    compress = Some(piece);
                    continue;
                }
                let mut num: u16 = 0;
                let mut i = 0usize;
                while i < 4 && p < end {
                    let Some(c) = hex_val(buf[p]) else {
                        break;
                    };
                    num = (num << 4) | c as u16;
                    p += 1;
                    i += 1;
                }
                if p >= end {
                    ipv6[piece] = num;
                    piece += 1;
                    break;
                }
                if buf[p] == b'.' {
                    if i == 0 {
                        self.log(buf, p, ErrorType::Ipv4InIpv6InvalidCodePoint);
                        return Err(());
                    }
                    p -= i;
                    if piece > 6 {
                        self.log(buf, p, ErrorType::Ipv4InIpv6TooManyPieces);
                        return Err(());
                    }
                    self.ipv4_in_ipv6_parse(buf, &mut p, end, ipv6, &mut piece)?;
                    break;
                }
                if buf[p] == b':' {
                    p += 1;
                    if p >= end {
                        self.log(buf, p, ErrorType::Ipv6InvalidCodePoint);
                        return Err(());
                    }
                } else if p < end {
                    self.log(buf, p, ErrorType::Ipv6InvalidCodePoint);
                    return Err(());
                }
                ipv6[piece] = num;
                piece += 1;
            }
        }
        // done:
        if let Some(c) = compress {
            let mut num = piece - c;
            let mut i = 7usize;
            while i != 0 && num > 0 {
                let idx = c + num - 1;
                ipv6.swap(idx, i);
                i -= 1;
                num -= 1;
            }
        } else if piece != 8 {
            self.log(buf, p, ErrorType::Ipv6TooFewPieces);
            return Err(());
        }
        Ok(())
    }

    /// lxb_url_ipv4_in_ipv6_parse()
    fn ipv4_in_ipv6_parse(
        &mut self,
        buf: &[u8],
        pp: &mut usize,
        end: usize,
        ipv6: &mut [u16; 8],
        piece: &mut usize,
    ) -> St {
        let mut seen = 0u32;
        let mut p = *pp;
        while p < end {
            let mut ipv4: i32 = -1;
            if seen > 0 {
                if buf[p] == b'.' && seen < 4 {
                    p += 1;
                    if p >= end {
                        break;
                    }
                } else {
                    self.log(buf, p, ErrorType::Ipv4InIpv6InvalidCodePoint);
                    return Err(());
                }
            }
            loop {
                let c = buf[p];
                if !c.is_ascii_digit() {
                    if ipv4 == -1 {
                        self.log(buf, p, ErrorType::Ipv4InIpv6InvalidCodePoint);
                        return Err(());
                    }
                    break;
                }
                if ipv4 == -1 {
                    ipv4 = (c - b'0') as i32;
                } else if ipv4 == 0 {
                    self.log(buf, p, ErrorType::Ipv4InIpv6InvalidCodePoint);
                    return Err(());
                } else {
                    ipv4 = ipv4 * 10 + (c - b'0') as i32;
                }
                if ipv4 > 255 {
                    self.log(buf, p, ErrorType::Ipv4InIpv6OutOfRangePart);
                    return Err(());
                }
                p += 1;
                if p >= end {
                    break;
                }
            }
            ipv6[*piece] = ipv6[*piece].wrapping_mul(0x100).wrapping_add(ipv4 as u16);
            seen += 1;
            if seen == 2 || seen == 4 {
                *piece += 1;
            }
        }
        if seen != 4 {
            self.log(buf, p, ErrorType::Ipv4InIpv6TooFewParts);
            return Err(());
        }
        *pp = p;
        Ok(())
    }
}

/// lxb_url_scheme_is_equal()
fn scheme_is_equal(a: &Url, b: &Url) -> bool {
    if a.scheme_type != b.scheme_type {
        return false;
    }
    if a.scheme_type == SchemeType::Unknown {
        return a.scheme_name == b.scheme_name;
    }
    true
}

/// lxb_url_path_part_by_index(): (offset, length) of a slash-terminated part.
fn path_part_by_index(data: &[u8], mut index: usize) -> Option<(usize, usize)> {
    let mut begin = 0usize;
    for (i, &c) in data.iter().enumerate() {
        if c == b'/' {
            if index == 0 {
                return Some((begin, i - begin));
            }
            index -= 1;
            begin = i + 1;
        }
    }
    None
}

/// lxb_url_path_try_dot()
fn path_try_dot(
    url: &mut Url,
    buf: &[u8],
    begin: &mut usize,
    last: &mut usize,
    start: &mut usize,
    end: usize,
    bqs: bool,
) {
    let special = url.is_special();
    let mut p = *start;
    let mut count = 0u32;
    while p < end {
        let c = buf[p];
        if c == b'/' || (c == b'\\' && special) || ((c == b'?' || c == b'#') && bqs) {
            break;
        } else if c == b'.' {
            count += 1;
        } else if c == b'%' {
            if p + 3 <= end && buf[p + 1] == b'2' && (buf[p + 2] == b'e' || buf[p + 2] == b'E') {
                count += 1;
            } else {
                return;
            }
            p += 2;
        } else {
            return;
        }
        p += 1;
    }
    if count == 0 || count > 2 {
        return;
    }
    if *start > *begin {
        url.path_append(&buf[*begin..*start - 1]);
    }
    if p < end {
        *start = p;
        *begin = p + 1;
        *last = *begin;
    } else {
        *start = end - 1;
        *begin = end;
        *last = end;
    }
    if count == 2 {
        url.path_shorten();
    } else if count == 1 {
        if let Some(s) = &mut url.path.str {
            if s.last() == Some(&b'/') {
                s.pop();
            }
        }
    }
}

/// lxb_url_path_dot_count(): the end of a "." / ".." segment at `p` (p
/// itself when it is not one).
#[allow(clippy::too_many_arguments)]
fn path_dot_count(
    url: &Url,
    buf: &[u8],
    mut p: usize,
    end: usize,
    sbuf: &mut Vec<u8>,
    last: &mut usize,
    path_count: &mut usize,
    bqs: bool,
) -> usize {
    let special = url.is_special();
    let mut count = 0u32;
    let begin = p;
    while p < end {
        let c = buf[p];
        if c == b'/' || (c == b'\\' && special) || ((c == b'?' || c == b'#') && bqs) {
            break;
        } else if c == b'.' {
            count += 1;
        } else if c == b'%' {
            if p + 3 <= end && buf[p + 1] == b'2' && (buf[p + 2] == b'e' || buf[p + 2] == b'E') {
                count += 1;
            } else {
                return begin;
            }
            p += 2;
        } else {
            return begin;
        }
        p += 1;
    }
    if count == 0 || count > 2 {
        return begin;
    }
    if url.scheme_type == SchemeType::File
        && *path_count == 1
        && *last >= 1
        && normalized_windows_drive_letter(&sbuf[1.min(sbuf.len())..(*last - 1).max(1)])
    {
        return p;
    }
    if count == 2 && *path_count > 0 {
        *path_count -= 1;
        let mut last_p = *last - 1;
        while last_p > 0 {
            last_p -= 1;
            if sbuf[last_p] == b'/' {
                sbuf.truncate(last_p + 1);
                break;
            }
        }
        *last = sbuf.len();
    }
    p
}

/// lxb_url_path_fix_windows_drive(): `sbuf[seg..]` is the segment just ended.
fn path_fix_windows_drive(url: &Url, sbuf: &mut [u8], seg: usize, count: usize) {
    let last = sbuf.len();
    if url.scheme_type == SchemeType::File
        && count == 0
        && ((last - seg == 3 && sbuf[last - 1] == b'/') || last - seg == 2)
        && windows_drive_letter(&sbuf[seg..last])
    {
        sbuf[seg + 1] = b':';
    }
}

/// lxb_url_percent_decode() (sets OPT_IDNA for decoded non-ASCII bytes)
fn percent_decode(data: &[u8], opt: &mut u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut p = 0usize;
    while p < data.len() {
        let mut c = data[p];
        p += 1;
        if c != b'%' {
            out.push(c);
            continue;
        }
        if p + 2 <= data.len() {
            if let (Some(h), Some(l)) = (hex_val(data[p]), hex_val(data[p + 1])) {
                c = (h << 4) | l;
                p += 2;
                if c >= 0x80 {
                    *opt |= OPT_IDNA;
                }
            }
        }
        out.push(c);
    }
    out
}

enum NumSt {
    Ok,
    Warning,
    Error,
}

/// lxb_url_ipv4_number_parse()
fn ipv4_number_parse(d: &[u8]) -> (u64, NumSt) {
    if d.is_empty() {
        return (0, NumSt::Error);
    }
    let mut r = 10u64;
    let mut i = 0usize;
    let mut status = NumSt::Ok;
    if d.len() > 1 && d[0] == b'0' {
        if d[1] == b'x' || d[1] == b'X' {
            i = 2;
            r = 16;
        } else {
            i = 1;
            r = 8;
        }
        if i >= d.len() {
            return (0, NumSt::Warning);
        }
        status = NumSt::Warning;
    }
    let mut n: u64 = 0;
    while i < d.len() {
        let c = d[i];
        i += 1;
        let v = match r {
            16 => hex_val(c).map(|v| v as u64),
            8 => (b'0'..=b'7').contains(&c).then(|| (c - b'0') as u64),
            _ => c.is_ascii_digit().then(|| (c - b'0') as u64),
        };
        let Some(v) = v else {
            return (0, NumSt::Error);
        };
        n = v + n * r;
        if n > u32::MAX as u64 {
            break;
        }
    }
    (n, status)
}

/// lxb_url_is_ipv4(): whether the domain ends in a number.
fn is_ipv4(d: &[u8]) -> bool {
    if d.is_empty() {
        return false;
    }
    let mut end = d.len();
    let mut isit = true;
    let mut first = true;
    let mut p = end;
    loop {
        p -= 1;
        let c = d[p];
        let mut skip_digit_check = false;
        if !is_hex(c) {
            if c == b'.' {
                if p == end - 1 {
                    if first {
                        end = p;
                        first = false;
                        skip_digit_check = true;
                    } else {
                        return false;
                    }
                } else {
                    p += 1;
                    break;
                }
            } else if c != b'x' && c != b'X' {
                return false;
            }
        }
        if !skip_digit_check && !c.is_ascii_digit() {
            isit = false;
        }
        if p == 0 {
            break;
        }
    }
    if p == end {
        return false;
    }
    if isit {
        return true;
    }
    !matches!(ipv4_number_parse(&d[p..end]).1, NumSt::Error)
}

// ----------------------------------------------------------------------
// Serialization

/// lxb_url_serialize_host_ipv4()
pub fn serialize_ipv4(ip: u32) -> Vec<u8> {
    format!(
        "{}.{}.{}.{}",
        (ip >> 24) as u8,
        (ip >> 16) as u8,
        (ip >> 8) as u8,
        ip as u8
    )
    .into_bytes()
}

/// lxb_url_serialize_host_ipv6()
pub fn serialize_ipv6(ipv6: &[u16; 8]) -> Vec<u8> {
    let mut count = 0usize;
    let mut tmp_count = (ipv6[0] == 0) as usize;
    let mut compress: Option<usize> = None;
    let mut tmp_compress = 0usize;
    for i in 1..8 {
        if ipv6[i] == 0 {
            if ipv6[i - 1] == 0 {
                tmp_count += 1;
            } else {
                tmp_count = 1;
                tmp_compress = i;
            }
        } else if tmp_count > count {
            compress = Some(tmp_compress);
            count = tmp_count;
        }
    }
    if tmp_count > count {
        compress = Some(tmp_compress);
        count = tmp_count;
    }
    if compress == Some(1) && ipv6[0] == 0 {
        compress = Some(0);
    }
    if count < 2 {
        compress = None;
    }
    let mut out = Vec::new();
    let mut ignore = false;
    let mut i = 0usize;
    while i < 8 {
        if ignore {
            if ipv6[i] == 0 {
                i += 1;
                continue;
            }
            ignore = false;
        }
        if compress == Some(i) {
            out.push(b':');
            if i == 0 {
                out.push(b':');
            }
            i += count;
            ignore = true;
            continue;
        }
        out.extend_from_slice(format!("{:x}", ipv6[i]).as_bytes());
        if i != 7 {
            out.push(b':');
        }
        i += 1;
    }
    out
}

/// lxb_url_serialize_host()
pub fn serialize_host(host: &Host, out: &mut Vec<u8>) {
    match host {
        Host::Domain(d) | Host::Opaque(d) => out.extend_from_slice(d),
        Host::Ipv4(ip) => out.extend_from_slice(&serialize_ipv4(*ip)),
        Host::Ipv6(ip) => {
            out.push(b'[');
            out.extend_from_slice(&serialize_ipv6(ip));
            out.push(b']');
        }
        _ => {}
    }
}

/// lxb_url_serialize_host_unicode(): Err when domain to Unicode failed
/// (nothing is written then).
pub fn serialize_host_unicode(host: &Host, out: &mut Vec<u8>) -> Result<(), ()> {
    match host {
        Host::Domain(d) | Host::Opaque(d) => {
            let u = idna::to_unicode(d)?;
            out.extend_from_slice(&u);
            Ok(())
        }
        h => {
            serialize_host(h, out);
            Ok(())
        }
    }
}

/// lxb_url_serialize() / lxb_url_serialize_idna()
pub fn serialize(url: &Url, unicode: bool, exclude_fragment: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&url.scheme_name);
    out.push(b':');
    if url.host != Host::Undef {
        out.extend_from_slice(b"//");
        if url.includes_credentials() {
            out.extend_from_slice(&url.username);
            if !url.password.is_empty() {
                out.push(b':');
                out.extend_from_slice(&url.password);
            }
            out.push(b'@');
        }
        if unicode {
            if serialize_host_unicode(&url.host, &mut out).is_err() {
                // lxb_url_serialize_body() returns the error: the rest is dropped
                return out;
            }
        } else {
            serialize_host(&url.host, &mut out);
        }
        if url.has_port {
            out.push(b':');
            out.extend_from_slice(url.port.to_string().as_bytes());
        }
    } else if !url.path.opaque && url.path.len() > 1 {
        let s = url.path.bytes();
        if s[0] == b'/' && s[1] == b'/' {
            out.extend_from_slice(b"/.");
        }
    }
    out.extend_from_slice(url.path.bytes());
    if let Some(q) = &url.query {
        out.push(b'?');
        out.extend_from_slice(q);
    }
    if !exclude_fragment {
        if let Some(f) = &url.fragment {
            out.push(b'#');
            out.extend_from_slice(f);
        }
    }
    out
}

// ----------------------------------------------------------------------
// API setters (lxb_url_api_*_set)

impl Parser {
    /// lxb_url_api_protocol_set()
    pub fn protocol_set(&mut self, url: &mut Url, v: &[u8]) -> St {
        self.parse_basic_h(url, false, None, v, State::SchemeStart)
    }

    /// lxb_url_api_hostname_set() (lxb_url_host_set_h())
    pub fn hostname_set(&mut self, url: &mut Url, v: &[u8]) -> St {
        if url.path.opaque {
            return Ok(());
        }
        let old = std::mem::take(&mut url.host);
        let st = self.parse_basic_h(url, false, None, v, State::Hostname);
        if st.is_err() || url.host == Host::Undef {
            url.host = old;
        }
        st
    }

    /// lxb_url_api_port_set()
    pub fn port_set(&mut self, url: &mut Url, v: &[u8]) -> St {
        if url.cannot_have_user_pass_port() {
            return Ok(());
        }
        if v.is_empty() {
            url.port = 0;
            url.has_port = false;
            return Ok(());
        }
        self.parse_basic_h(url, false, None, v, State::Port)
    }

    /// lxb_url_api_pathname_set()
    pub fn pathname_set(&mut self, url: &mut Url, v: &[u8]) -> St {
        if url.path.opaque {
            return Ok(());
        }
        url.path.length = 0;
        if let Some(s) = &mut url.path.str {
            s.clear();
        }
        self.parse_basic_h(url, false, None, v, State::PathStart)
    }

    /// lxb_url_api_search_set()
    pub fn search_set(&mut self, url: &mut Url, v: &[u8]) -> St {
        url.query = None;
        if v.is_empty() {
            return Ok(());
        }
        let v = v.strip_prefix(b"?").unwrap_or(v);
        self.parse_basic_h(url, false, None, v, State::Query)
    }

    /// lxb_url_api_hash_set()
    pub fn hash_set(&mut self, url: &mut Url, v: &[u8]) -> St {
        url.fragment = None;
        if v.is_empty() {
            return Ok(());
        }
        let v = v.strip_prefix(b"#").unwrap_or(v);
        self.parse_basic_h(url, false, None, v, State::Fragment)
    }
}

/// lxb_url_api_username_set()
pub fn username_set(url: &mut Url, v: &[u8]) {
    if url.cannot_have_user_pass_port() {
        return;
    }
    url.username.clear();
    percent_encode_after_utf8(v, &mut url.username, MAP_USERINFO);
}

/// lxb_url_api_password_set()
pub fn password_set(url: &mut Url, v: &[u8]) {
    if url.cannot_have_user_pass_port() {
        return;
    }
    url.password.clear();
    percent_encode_after_utf8(v, &mut url.password, MAP_USERINFO);
}
