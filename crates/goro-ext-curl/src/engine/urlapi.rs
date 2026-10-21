//! Pure-Rust port of libcurl 8.14.1's URL API (`lib/urlapi.c`), the URL
//! escaping helpers (`lib/escape.c`) and the parts of `lib/url.c` /
//! `lib/http.c` that turn a URL into connection parameters
//! (`parseurlandfillconn()`) and resolve redirects (`Curl_http_follow()`).
//!
//! Behaviour is meant to be byte-exact with libcurl 8.14.1 built for a
//! non-Windows platform without libidn2. All strings are byte strings; like
//! the C API, input is treated as a C string (it ends at the first NUL byte).

// ---------------------------------------------------------------------------
// Public constants (include/curl/urlapi.h)
// ---------------------------------------------------------------------------

/// return default port number
pub const CURLU_DEFAULT_PORT: u32 = 1 << 0;
/// act as if no port number was set, if the port number matches the default
/// for the scheme
pub const CURLU_NO_DEFAULT_PORT: u32 = 1 << 1;
/// return default scheme if missing
pub const CURLU_DEFAULT_SCHEME: u32 = 1 << 2;
/// allow non-supported scheme
pub const CURLU_NON_SUPPORT_SCHEME: u32 = 1 << 3;
/// leave dot sequences
pub const CURLU_PATH_AS_IS: u32 = 1 << 4;
/// no user+password allowed
pub const CURLU_DISALLOW_USER: u32 = 1 << 5;
/// URL decode on get
pub const CURLU_URLDECODE: u32 = 1 << 6;
/// URL encode on set
pub const CURLU_URLENCODE: u32 = 1 << 7;
/// append a form style part
pub const CURLU_APPENDQUERY: u32 = 1 << 8;
/// legacy curl-style guessing
pub const CURLU_GUESS_SCHEME: u32 = 1 << 9;
/// Allow empty authority when the scheme is unknown.
pub const CURLU_NO_AUTHORITY: u32 = 1 << 10;
/// Allow spaces in the URL
pub const CURLU_ALLOW_SPACE: u32 = 1 << 11;
/// get the hostname in punycode
pub const CURLU_PUNYCODE: u32 = 1 << 12;
/// punycode => IDN conversion
pub const CURLU_PUNY2IDN: u32 = 1 << 13;
/// allow empty queries and fragments when extracting the URL or the
/// components
pub const CURLU_GET_EMPTY: u32 = 1 << 14;
/// for get, do not accept a guess
pub const CURLU_NO_GUESS_SCHEME: u32 = 1 << 15;

/// Longest input string accepted (lib/urldata.h CURL_MAX_INPUT_LENGTH).
pub const CURL_MAX_INPUT_LENGTH: usize = 8_000_000;

// CURLcode values used by the helpers below.
pub const CURLE_OK: u32 = 0;
pub const CURLE_UNSUPPORTED_PROTOCOL: u32 = 1;
pub const CURLE_URL_MALFORMAT: u32 = 3;
pub const CURLE_OUT_OF_MEMORY: u32 = 27;
pub const CURLE_TOO_MANY_REDIRECTS: u32 = 47;
pub const CURLE_LOGIN_DENIED: u32 = 67;

// CURLPROTO_* bits (include/curl/curl.h, lib/urldata.h)
pub const CURLPROTO_HTTP: u64 = 1 << 0;
pub const CURLPROTO_HTTPS: u64 = 1 << 1;
pub const CURLPROTO_FTP: u64 = 1 << 2;
pub const CURLPROTO_FTPS: u64 = 1 << 3;
pub const CURLPROTO_SCP: u64 = 1 << 4;
pub const CURLPROTO_SFTP: u64 = 1 << 5;
pub const CURLPROTO_TELNET: u64 = 1 << 6;
pub const CURLPROTO_LDAP: u64 = 1 << 7;
pub const CURLPROTO_LDAPS: u64 = 1 << 8;
pub const CURLPROTO_DICT: u64 = 1 << 9;
pub const CURLPROTO_FILE: u64 = 1 << 10;
pub const CURLPROTO_TFTP: u64 = 1 << 11;
pub const CURLPROTO_IMAP: u64 = 1 << 12;
pub const CURLPROTO_IMAPS: u64 = 1 << 13;
pub const CURLPROTO_POP3: u64 = 1 << 14;
pub const CURLPROTO_POP3S: u64 = 1 << 15;
pub const CURLPROTO_SMTP: u64 = 1 << 16;
pub const CURLPROTO_SMTPS: u64 = 1 << 17;
pub const CURLPROTO_RTSP: u64 = 1 << 18;
pub const CURLPROTO_RTMP: u64 = 1 << 19;
pub const CURLPROTO_RTMPT: u64 = 1 << 20;
pub const CURLPROTO_RTMPE: u64 = 1 << 21;
pub const CURLPROTO_RTMPTE: u64 = 1 << 22;
pub const CURLPROTO_RTMPS: u64 = 1 << 23;
pub const CURLPROTO_RTMPTS: u64 = 1 << 24;
pub const CURLPROTO_GOPHER: u64 = 1 << 25;
pub const CURLPROTO_SMB: u64 = 1 << 26;
pub const CURLPROTO_SMBS: u64 = 1 << 27;
pub const CURLPROTO_MQTT: u64 = 1 << 28;
pub const CURLPROTO_GOPHERS: u64 = 1 << 29;
pub const CURLPROTO_WS: u64 = 1 << 30;
pub const CURLPROTO_WSS: u64 = 1 << 31;
/// CURLPROTO_ALL as stored in data->set.allowed_protocols by default.
pub const CURLPROTO_ALL: u64 = !0;
/// Default CURLOPT_REDIR_PROTOCOLS (lib/urldata.h CURLPROTO_REDIR).
pub const CURLPROTO_REDIR: u64 = CURLPROTO_HTTP | CURLPROTO_HTTPS | CURLPROTO_FTP | CURLPROTO_FTPS;

/// Maximum hostname length accepted by parseurlandfillconn() (url.c).
pub const MAX_URL_LEN: usize = 0xffff;

const MAX_SCHEME_LEN: usize = 40;
const DEFAULT_SCHEME: &[u8] = b"https";

/// CURLUcode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum UCode {
    Ok = 0,
    BadHandle = 1,
    BadPartpointer = 2,
    MalformedInput = 3,
    BadPortNumber = 4,
    UnsupportedScheme = 5,
    Urldecode = 6,
    OutOfMemory = 7,
    UserNotAllowed = 8,
    UnknownPart = 9,
    NoScheme = 10,
    NoUser = 11,
    NoPassword = 12,
    NoOptions = 13,
    NoHost = 14,
    NoPort = 15,
    NoQuery = 16,
    NoFragment = 17,
    NoZoneid = 18,
    BadFileUrl = 19,
    BadFragment = 20,
    BadHostname = 21,
    BadIpv6 = 22,
    BadLogin = 23,
    BadPassword = 24,
    BadPath = 25,
    BadQuery = 26,
    BadScheme = 27,
    BadSlashes = 28,
    BadUser = 29,
    LacksIdn = 30,
    TooLarge = 31,
}

impl UCode {
    /// The numeric CURLUcode value.
    pub fn code(self) -> u32 {
        self as u32
    }

    /// Map a numeric CURLUcode back to the enum (None for CURLUE_LAST and
    /// beyond).
    pub fn from_u32(n: u32) -> Option<UCode> {
        use UCode::*;
        const ALL: [UCode; 32] = [
            Ok,
            BadHandle,
            BadPartpointer,
            MalformedInput,
            BadPortNumber,
            UnsupportedScheme,
            Urldecode,
            OutOfMemory,
            UserNotAllowed,
            UnknownPart,
            NoScheme,
            NoUser,
            NoPassword,
            NoOptions,
            NoHost,
            NoPort,
            NoQuery,
            NoFragment,
            NoZoneid,
            BadFileUrl,
            BadFragment,
            BadHostname,
            BadIpv6,
            BadLogin,
            BadPassword,
            BadPath,
            BadQuery,
            BadScheme,
            BadSlashes,
            BadUser,
            LacksIdn,
            TooLarge,
        ];
        ALL.get(n as usize).copied()
    }
}

/// CURLUPart
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Part {
    Url = 0,
    Scheme = 1,
    User = 2,
    Password = 3,
    Options = 4,
    Host = 5,
    Port = 6,
    Path = 7,
    Query = 8,
    Fragment = 9,
    Zoneid = 10,
}

impl Part {
    /// Map a numeric CURLUPART_* value to the enum. libcurl answers unknown
    /// parts with CURLUE_UNKNOWN_PART.
    pub fn from_u32(n: u32) -> Result<Part, UCode> {
        use Part::*;
        Ok(match n {
            0 => Url,
            1 => Scheme,
            2 => User,
            3 => Password,
            4 => Options,
            5 => Host,
            6 => Port,
            7 => Path,
            8 => Query,
            9 => Fragment,
            10 => Zoneid,
            _ => return Err(UCode::UnknownPart),
        })
    }
}

/// curl_url_strerror()
pub fn url_strerror(code: UCode) -> &'static str {
    use UCode::*;
    match code {
        Ok => "No error",
        BadHandle => "An invalid CURLU pointer was passed as argument",
        BadPartpointer => "An invalid 'part' argument was passed as argument",
        MalformedInput => "Malformed input to a URL function",
        BadPortNumber => "Port number was not a decimal number between 0 and 65535",
        UnsupportedScheme => "Unsupported URL scheme",
        Urldecode => "URL decode error, most likely because of rubbish in the input",
        OutOfMemory => "A memory function failed",
        UserNotAllowed => "Credentials was passed in the URL when prohibited",
        UnknownPart => "An unknown part ID was passed to a URL API function",
        NoScheme => "No scheme part in the URL",
        NoUser => "No user part in the URL",
        NoPassword => "No password part in the URL",
        NoOptions => "No options part in the URL",
        NoHost => "No host part in the URL",
        NoPort => "No port part in the URL",
        NoQuery => "No query part in the URL",
        NoFragment => "No fragment part in the URL",
        NoZoneid => "No zoneid part in the URL",
        BadLogin => "Bad login part",
        BadIpv6 => "Bad IPv6 address",
        BadHostname => "Bad hostname",
        BadFileUrl => "Bad file:// URL",
        BadSlashes => "Unsupported number of slashes following scheme",
        BadScheme => "Bad scheme",
        BadPath => "Bad path",
        BadFragment => "Bad fragment",
        BadQuery => "Bad query",
        BadPassword => "Bad password",
        BadUser => "Bad user",
        LacksIdn => "libcurl lacks IDN support",
        TooLarge => "A value or data field is larger than allowed",
    }
}

/// curl_url_strerror() for a raw number ("CURLUcode unknown" for values
/// out of range).
pub fn url_strerror_u32(code: u32) -> &'static str {
    match UCode::from_u32(code) {
        Some(c) => url_strerror(c),
        None => "CURLUcode unknown",
    }
}

/// Curl_uc_to_curlcode() (url.c)
pub fn uc_to_curlcode(uc: UCode) -> u32 {
    match uc {
        UCode::UnsupportedScheme => CURLE_UNSUPPORTED_PROTOCOL,
        UCode::OutOfMemory => CURLE_OUT_OF_MEMORY,
        UCode::UserNotAllowed => CURLE_LOGIN_DENIED,
        _ => CURLE_URL_MALFORMAT,
    }
}

// ---------------------------------------------------------------------------
// Scheme table (Curl_get_scheme_handler)
// ---------------------------------------------------------------------------

/// What urlapi / url.c need to know about a protocol handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemeInfo {
    /// lowercase scheme name
    pub name: &'static str,
    /// Curl_handler.defport
    pub defport: u16,
    /// Curl_handler.protocol (CURLPROTO_* bit)
    pub protocol: u64,
    /// PROTOPT_URLOPTIONS: ";options" in the userinfo are parsed
    pub urloptions: bool,
    /// PROTOPT_USERPWDCTRL: control codes allowed in decoded user/password
    pub userpwdctrl: bool,
    /// Whether the handler is "built in". Only built-in entries are ever
    /// returned by [`get_scheme_handler`]; the rest of the table is just
    /// knowledge (see [`scheme_default_port`]).
    pub builtin: bool,
}

const fn scheme(
    name: &'static str,
    defport: u16,
    protocol: u64,
    urloptions: bool,
    userpwdctrl: bool,
    builtin: bool,
) -> SchemeInfo {
    SchemeInfo {
        name,
        defport,
        protocol,
        urloptions,
        userpwdctrl,
        builtin,
    }
}

/// The default scheme table: every handler libcurl 8.14.1 knows about, with
/// only http, https and file marked as built in. Edit the `builtin` flags
/// (or pass your own table to [`Url::with_schemes`]) to change what is
/// "supported".
pub static SCHEMES: &[SchemeInfo] = &[
    scheme("http", 80, CURLPROTO_HTTP, false, true, true),
    scheme("https", 443, CURLPROTO_HTTPS, false, true, true),
    scheme("file", 0, CURLPROTO_FILE, false, false, true),
    scheme("ftp", 21, CURLPROTO_FTP, false, false, false),
    scheme("ftps", 990, CURLPROTO_FTPS, false, false, false),
    scheme("ws", 80, CURLPROTO_WS, false, true, false),
    scheme("wss", 443, CURLPROTO_WSS, false, true, false),
    scheme("dict", 2628, CURLPROTO_DICT, false, false, false),
    scheme("ldap", 389, CURLPROTO_LDAP, false, false, false),
    scheme("ldaps", 636, CURLPROTO_LDAPS, false, false, false),
    scheme("imap", 143, CURLPROTO_IMAP, true, false, false),
    scheme("imaps", 993, CURLPROTO_IMAPS, true, false, false),
    scheme("pop3", 110, CURLPROTO_POP3, true, false, false),
    scheme("pop3s", 995, CURLPROTO_POP3S, true, false, false),
    scheme("smtp", 25, CURLPROTO_SMTP, true, false, false),
    scheme("smtps", 465, CURLPROTO_SMTPS, true, false, false),
    scheme("telnet", 23, CURLPROTO_TELNET, false, false, false),
    scheme("tftp", 69, CURLPROTO_TFTP, false, false, false),
    scheme("gopher", 70, CURLPROTO_GOPHER, false, false, false),
    scheme("gophers", 70, CURLPROTO_GOPHERS, false, false, false),
    scheme("rtsp", 554, CURLPROTO_RTSP, false, false, false),
    scheme("smb", 445, CURLPROTO_SMB, false, false, false),
    scheme("smbs", 445, CURLPROTO_SMBS, false, false, false),
    scheme("scp", 22, CURLPROTO_SCP, false, false, false),
    scheme("sftp", 22, CURLPROTO_SFTP, false, false, false),
    scheme("mqtt", 1883, CURLPROTO_MQTT, false, false, false),
    scheme("rtmp", 1935, CURLPROTO_RTMP, false, false, false),
    scheme("rtmpt", 80, CURLPROTO_RTMPT, false, false, false),
    scheme("rtmpe", 1935, CURLPROTO_RTMPE, false, false, false),
    scheme("rtmpte", 80, CURLPROTO_RTMPTE, false, false, false),
    scheme("rtmps", 443, CURLPROTO_RTMPS, false, false, false),
    scheme("rtmpts", 443, CURLPROTO_RTMPTS, false, false, false),
];

/// Curl_get_scheme_handler(): the built-in handler for `name`
/// (case-insensitive), if any.
pub fn get_scheme_handler(
    table: &'static [SchemeInfo],
    name: &[u8],
) -> Option<&'static SchemeInfo> {
    // libcurl's perfect hash only considers 1..=7 byte names; every name in
    // the table fits so a plain case-insensitive compare is equivalent.
    if name.is_empty() || name.len() > 7 {
        return None;
    }
    table
        .iter()
        .find(|s| s.builtin && s.name.as_bytes().eq_ignore_ascii_case(name))
}

/// The default port of any scheme in the table, built in or not.
pub fn scheme_default_port(table: &'static [SchemeInfo], name: &[u8]) -> Option<u16> {
    table
        .iter()
        .find(|s| s.name.as_bytes().eq_ignore_ascii_case(name))
        .map(|s| s.defport)
}

// ---------------------------------------------------------------------------
// Small C-string helpers
// ---------------------------------------------------------------------------

/// Byte at `i`, or NUL past the end (C string semantics).
#[inline]
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// The part of `s` before the first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&c| c == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

fn find(s: &[u8], b: u8) -> Option<usize> {
    s.iter().position(|&c| c == b)
}

fn rfind(s: &[u8], b: u8) -> Option<usize> {
    s.iter().rposition(|&c| c == b)
}

fn find_sub(s: &[u8], needle: &[u8]) -> Option<usize> {
    s.windows(needle.len()).position(|w| w == needle)
}

fn strspn(s: &[u8], accept: &[u8]) -> usize {
    s.iter().take_while(|c| accept.contains(c)).count()
}

fn strcspn(s: &[u8], reject: &[u8]) -> usize {
    s.iter()
        .take_while(|c| **c != 0 && !reject.contains(c))
        .count()
}

/// checkprefix(): case-insensitive prefix match
fn checkprefix(prefix: &[u8], s: &[u8]) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn is_unreserved(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'.' || c == b'_' || c == b'~'
}

fn push_hexbyte(out: &mut Vec<u8>, val: u8, lowercase: bool) {
    let t: &[u8; 16] = if lowercase {
        b"0123456789abcdef"
    } else {
        b"0123456789ABCDEF"
    };
    out.push(b'%');
    out.push(t[(val >> 4) as usize]);
    out.push(t[(val & 0x0f) as usize]);
}

fn hexval(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// Curl_is_ASCII_name()
fn is_ascii_name(s: &[u8]) -> bool {
    cstr(s).iter().all(|c| c & 0x80 == 0)
}

/// STARTS_WITH_URL_DRIVE_PREFIX
fn starts_with_url_drive_prefix(s: &[u8]) -> bool {
    at(s, 0).is_ascii_alphabetic()
        && (at(s, 1) == b':' || at(s, 1) == b'|')
        && (at(s, 2) == b'/' || at(s, 2) == b'\\' || at(s, 2) == 0)
}

/// curlx_str_number/hex/octal: parse digits of `base` at the start of `s`.
/// Returns (value, bytes consumed) or None (no digit / overflow).
fn str_num_base(s: &[u8], max: u64, base: u64) -> Option<(u64, usize)> {
    let valid = |c: u8| match base {
        10 => c.is_ascii_digit(),
        16 => c.is_ascii_hexdigit(),
        _ => (b'0'..=b'7').contains(&c),
    };
    let mut i = 0;
    if !valid(at(s, 0)) {
        return None;
    }
    let mut num: u64 = 0;
    while valid(at(s, i)) {
        let n = hexval(s[i]) as u64;
        if max < base {
            num = num * base + n;
            if num > max {
                return None;
            }
        } else {
            if num > (max - n) / base {
                return None;
            }
            num = num * base + n;
        }
        i += 1;
    }
    Some((num, i))
}

fn str_number(s: &[u8], max: u64) -> Option<(u64, usize)> {
    str_num_base(s, max, 10)
}

/// A dynbuf with a size limit ("toobig"), mirroring lib/curlx/dynbuf.c
/// closely enough for the URL code: it remembers whether anything was ever
/// appended (a never-appended dynbuf has a NULL pointer in C).
struct DynBuf {
    buf: Vec<u8>,
    allocated: bool,
    toobig: usize,
}

impl DynBuf {
    fn new(toobig: usize) -> DynBuf {
        DynBuf {
            buf: Vec::new(),
            allocated: false,
            toobig,
        }
    }

    fn add(&mut self, mem: &[u8]) -> Result<(), UCode> {
        if self.buf.len() + mem.len() + 1 > self.toobig {
            self.buf = Vec::new();
            self.allocated = false;
            return Err(UCode::TooLarge);
        }
        self.allocated = true;
        self.buf.extend_from_slice(mem);
        Ok(())
    }

    fn into_opt(self) -> Option<Vec<u8>> {
        if self.allocated { Some(self.buf) } else { None }
    }
}

// ---------------------------------------------------------------------------
// escape.c
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reject {
    Nada,
    Ctrl,
    Zero,
}

/// Curl_urldecode(); Err(()) means CURLE_URL_MALFORMAT.
fn urldecode(s: &[u8], ctrl: Reject) -> Result<Vec<u8>, ()> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let mut c = s[i];
        if c == b'%'
            && s.len() - i > 2
            && s[i + 1].is_ascii_hexdigit()
            && s[i + 2].is_ascii_hexdigit()
        {
            c = (hexval(s[i + 1]) << 4) | hexval(s[i + 2]);
            i += 3;
        } else {
            i += 1;
        }
        if (ctrl == Reject::Ctrl && c < 0x20) || (ctrl == Reject::Zero && c == 0) {
            return Err(());
        }
        out.push(c);
    }
    Ok(out)
}

/// curl_easy_escape(): everything but ALPHA / DIGIT / "-._~" becomes %XX
/// (uppercase hex).
pub fn easy_escape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3);
    for &c in s {
        if is_unreserved(c) {
            out.push(c);
        } else {
            push_hexbyte(&mut out, c, false);
        }
    }
    out
}

/// curl_easy_unescape(): decodes every %XX, accepts anything.
pub fn easy_unescape(s: &[u8]) -> Vec<u8> {
    urldecode(s, Reject::Nada).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// inet_pton / inet_ntop (glibc behaviour, which 8.14.1 uses on Linux)
// ---------------------------------------------------------------------------

fn inet_pton4(src: &[u8]) -> Option<[u8; 4]> {
    let mut tmp = [0u8; 4];
    let mut tp = 0;
    let mut saw_digit = false;
    let mut octets = 0;
    for &ch in src {
        if ch.is_ascii_digit() {
            let new = tmp[tp] as u32 * 10 + (ch - b'0') as u32;
            if saw_digit && tmp[tp] == 0 {
                return None;
            }
            if new > 255 {
                return None;
            }
            tmp[tp] = new as u8;
            if !saw_digit {
                octets += 1;
                if octets > 4 {
                    return None;
                }
                saw_digit = true;
            }
        } else if ch == b'.' && saw_digit {
            if octets == 4 {
                return None;
            }
            tp += 1;
            tmp[tp] = 0;
            saw_digit = false;
        } else {
            return None;
        }
    }
    if octets < 4 {
        return None;
    }
    Some(tmp)
}

fn inet_pton6(src: &[u8]) -> Option<[u8; 16]> {
    let mut tmp = [0u8; 16];
    let mut tp = 0usize;
    let mut colonp: Option<usize> = None;
    let mut i = 0;
    if src.is_empty() {
        return None;
    }
    if src[0] == b':' {
        i += 1;
        if i == src.len() || src[i] != b':' {
            return None;
        }
    }
    let mut curtok = i;
    let mut xdigits_seen = 0;
    let mut val: u32 = 0;
    while i < src.len() {
        let ch = src[i];
        i += 1;
        if ch.is_ascii_hexdigit() {
            if xdigits_seen == 4 {
                return None;
            }
            val = (val << 4) | hexval(ch) as u32;
            if val > 0xffff {
                return None;
            }
            xdigits_seen += 1;
            continue;
        }
        if ch == b':' {
            curtok = i;
            if xdigits_seen == 0 {
                if colonp.is_some() {
                    return None;
                }
                colonp = Some(tp);
                continue;
            } else if i == src.len() {
                return None;
            }
            if tp + 2 > 16 {
                return None;
            }
            tmp[tp] = (val >> 8) as u8;
            tmp[tp + 1] = val as u8;
            tp += 2;
            xdigits_seen = 0;
            val = 0;
            continue;
        }
        if ch == b'.'
            && tp + 4 <= 16
            && let Some(v4) = inet_pton4(&src[curtok..])
        {
            tmp[tp..tp + 4].copy_from_slice(&v4);
            tp += 4;
            xdigits_seen = 0;
            break;
        }
        return None;
    }
    if xdigits_seen > 0 {
        if tp + 2 > 16 {
            return None;
        }
        tmp[tp] = (val >> 8) as u8;
        tmp[tp + 1] = val as u8;
        tp += 2;
    }
    if let Some(cp) = colonp {
        if tp == 16 {
            return None;
        }
        let n = tp - cp;
        let moved: Vec<u8> = tmp[cp..tp].to_vec();
        for b in tmp[cp..].iter_mut() {
            *b = 0;
        }
        tmp[16 - n..].copy_from_slice(&moved);
        tp = 16;
    }
    if tp != 16 {
        return None;
    }
    Some(tmp)
}

fn inet_ntop6(src: &[u8; 16]) -> Vec<u8> {
    let mut words = [0u32; 8];
    for (i, b) in src.iter().enumerate() {
        words[i / 2] |= (*b as u32) << ((1 - (i % 2)) << 3);
    }
    let (mut best_base, mut best_len): (isize, usize) = (-1, 0);
    let (mut cur_base, mut cur_len): (isize, usize) = (-1, 0);
    for (i, w) in words.iter().enumerate() {
        if *w == 0 {
            if cur_base == -1 {
                cur_base = i as isize;
                cur_len = 1;
            } else {
                cur_len += 1;
            }
        } else if cur_base != -1 {
            if best_base == -1 || cur_len > best_len {
                best_base = cur_base;
                best_len = cur_len;
            }
            cur_base = -1;
        }
    }
    if cur_base != -1 && (best_base == -1 || cur_len > best_len) {
        best_base = cur_base;
        best_len = cur_len;
    }
    if best_base != -1 && best_len < 2 {
        best_base = -1;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < 8 {
        if best_base != -1
            && (i as isize) >= best_base
            && (i as isize) < best_base + best_len as isize
        {
            if i as isize == best_base {
                out.push(b':');
            }
            i += 1;
            continue;
        }
        if i != 0 {
            out.push(b':');
        }
        if i == 6 && best_base == 0 && (best_len == 6 || (best_len == 5 && words[5] == 0xffff)) {
            out.extend_from_slice(
                format!("{}.{}.{}.{}", src[12], src[13], src[14], src[15]).as_bytes(),
            );
            break;
        }
        out.extend_from_slice(format!("{:x}", words[i]).as_bytes());
        i += 1;
    }
    if best_base != -1 && best_base as usize + best_len == 8 {
        out.push(b':');
    }
    out
}

// ---------------------------------------------------------------------------
// urlapi.c internals
// ---------------------------------------------------------------------------

/// Find the separator at the end of the hostname, or the '?' in cases like
/// http://www.example.com?id=2380
fn find_host_sep(url: &[u8]) -> usize {
    let mut sep = match find_sub(url, b"//") {
        Some(n) => n + 2,
        None => 0,
    };
    while sep < url.len() && url[sep] != b'/' && url[sep] != b'?' {
        sep += 1;
    }
    sep
}

/// urlencode_str(): append `url` to `o`, URL-encoding spaces (as %20, or '+'
/// in the query part), control bytes and bytes >= 0x7f.
fn urlencode_str(o: &mut DynBuf, url: &[u8], relative: bool, query: bool) -> Result<(), UCode> {
    let mut left = !query;
    let mut start = 0;
    if !relative {
        start = find_host_sep(url);
        o.add(&url[..start])?;
    }
    let mut tmp = Vec::with_capacity(url.len() - start);
    for &c in &url[start..] {
        if c == b' ' {
            if left {
                tmp.extend_from_slice(b"%20");
            } else {
                tmp.push(b'+');
            }
        } else if !(b' '..0x7f).contains(&c) {
            push_hexbyte(&mut tmp, c, true);
        } else {
            tmp.push(c);
            if c == b'?' {
                left = false;
            }
        }
    }
    o.add(&tmp)
}

/// Curl_is_absolute_url(): the length of the scheme if `url` is absolute,
/// otherwise 0. With `guess_scheme`, the scheme must be followed by ":/".
pub fn is_absolute_url(url: &[u8], guess_scheme: bool) -> usize {
    let mut i = 0;
    if at(url, 0).is_ascii_alphabetic() {
        i = 1;
        while i < MAX_SCHEME_LEN {
            let s = at(url, i);
            if s != 0 && (s.is_ascii_alphanumeric() || s == b'+' || s == b'-' || s == b'.') {
                i += 1;
            } else {
                break;
            }
        }
    }
    if i != 0 && at(url, i) == b':' && (at(url, i + 1) == b'/' || !guess_scheme) {
        return i;
    }
    0
}

/// Curl_junkscan(): reject control bytes (and space unless allowed).
fn junkscan(url: &[u8], allowspace: bool) -> Result<usize, UCode> {
    let n = url.len();
    if n > CURL_MAX_INPUT_LENGTH {
        return Err(UCode::MalformedInput);
    }
    let control = if allowspace { 0x1f } else { 0x20 };
    if url.iter().any(|&c| c <= control || c == 127) {
        return Err(UCode::MalformedInput);
    }
    Ok(n)
}

/// Curl_parse_login_details(): (user, password, options)
fn parse_login_details(
    login: &[u8],
    want_options: bool,
) -> (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>) {
    let len = login.len();
    let psep = find(login, b':');
    let osep = if want_options {
        find(login, b';')
    } else {
        None
    };
    let ulen = match (psep, osep) {
        (Some(p), Some(o)) if p > o => o,
        (Some(p), _) => p,
        (None, Some(o)) => o,
        (None, None) => len,
    };
    let plen = match psep {
        Some(p) => {
            (match osep {
                Some(o) if o > p => o - p,
                _ => len - p,
            }) - 1
        }
        None => 0,
    };
    let olen = match osep {
        Some(o) => {
            (match psep {
                Some(p) if p > o => p - o,
                _ => len - o,
            }) - 1
        }
        None => 0,
    };
    let user = login[..ulen].to_vec();
    let pass = psep.map(|p| login[p + 1..p + 1 + plen].to_vec());
    let opts = match osep {
        Some(o) if want_options && olen > 0 => Some(login[o + 1..o + 1 + olen].to_vec()),
        _ => None,
    };
    (user, pass, opts)
}

/// dedotdotify(): RFC 3986 section 5.2.4 "Remove Dot Segments".
/// Returns None when the input is shorter than 2 bytes (left as is).
fn dedotdotify(input: &[u8]) -> Option<Vec<u8>> {
    // is_dot(): returns the new (position, remaining length) on a match
    fn is_dot(s: &[u8], pos: usize, clen: usize) -> Option<(usize, usize)> {
        if at(s, pos) == b'.' {
            Some((pos + 1, clen.wrapping_sub(1)))
        } else if clen >= 3
            && at(s, pos) == b'%'
            && at(s, pos + 1) == b'2'
            && (at(s, pos + 2) | 0x20) == b'e'
        {
            Some((pos + 3, clen - 3))
        } else {
            None
        }
    }
    let mut clen = input.len();
    if clen < 2 {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(clen);
    let mut i = 0usize;
    'end: {
        // A. If the input buffer begins with a prefix of "../" or "./",
        // then remove that prefix from the input buffer; otherwise,
        if let Some((np, nl)) = is_dot(input, i, clen) {
            i = np;
            clen = nl;
            if clen == 0 {
                break 'end;
            } else if at(input, i) == b'/' {
                i += 1;
                clen -= 1;
            } else if let Some((p2, b2)) = is_dot(input, i, clen) {
                // D. if the input buffer consists only of "." or "..", then
                // remove that from the input buffer; otherwise,
                if b2 == 0 {
                    break 'end;
                } else if at(input, p2) == b'/' {
                    i = p2 + 1;
                    clen = b2 - 1;
                }
            }
        }
        while clen > 0 {
            if at(input, i) == b'/' {
                let p = i + 1;
                let blen = clen - 1;
                // B. "/./" or "/." -> "/"
                if let Some((p, blen)) = is_dot(input, p, blen) {
                    if blen == 0 {
                        out.push(b'/');
                        break;
                    } else if at(input, p) == b'/' {
                        i = p;
                        clen = blen;
                        continue;
                    } else if let Some((p2, b2)) = is_dot(input, p, blen) {
                        // C. "/../" or "/.." -> "/" and remove the last
                        // segment from the output
                        if at(input, p2) == b'/' || b2 == 0 {
                            if !out.is_empty()
                                && let Some(last) = rfind(&out, b'/')
                            {
                                out.truncate(last);
                            }
                            if b2 != 0 {
                                i = p2;
                                clen = b2;
                                continue;
                            }
                            out.push(b'/');
                            break;
                        }
                    }
                }
            }
            // E. move the first byte to the output
            out.push(input[i]);
            i += 1;
            clen -= 1;
        }
    }
    Some(out)
}

/// The libcurl CURLU handle.
#[derive(Clone, Debug)]
pub struct Url {
    scheme: Option<Vec<u8>>,
    user: Option<Vec<u8>>,
    password: Option<Vec<u8>>,
    options: Option<Vec<u8>>,
    host: Option<Vec<u8>>,
    zoneid: Option<Vec<u8>>,
    port: Option<Vec<u8>>,
    path: Option<Vec<u8>>,
    query: Option<Vec<u8>>,
    fragment: Option<Vec<u8>>,
    portnum: u16,
    query_present: bool,
    fragment_present: bool,
    guessed_scheme: bool,
    schemes: &'static [SchemeInfo],
}

impl Default for Url {
    fn default() -> Self {
        Url::new()
    }
}

#[derive(PartialEq, Eq)]
enum HostType {
    Name,
    Ipv4,
    Ipv6,
}

impl Url {
    /// curl_url() using the default [`SCHEMES`] table.
    pub fn new() -> Url {
        Url::with_schemes(SCHEMES)
    }

    /// curl_url() with a custom scheme table.
    pub fn with_schemes(schemes: &'static [SchemeInfo]) -> Url {
        Url {
            scheme: None,
            user: None,
            password: None,
            options: None,
            host: None,
            zoneid: None,
            port: None,
            path: None,
            query: None,
            fragment: None,
            portnum: 0,
            query_present: false,
            fragment_present: false,
            guessed_scheme: false,
            schemes,
        }
    }

    /// The scheme table this handle uses.
    pub fn schemes(&self) -> &'static [SchemeInfo] {
        self.schemes
    }

    /// The numeric port (only meaningful when a port is stored).
    pub fn portnum(&self) -> u16 {
        self.portnum
    }

    fn handler(&self, name: &[u8]) -> Option<&'static SchemeInfo> {
        get_scheme_handler(self.schemes, name)
    }

    /// curl_url_dup(). Note that, like libcurl, the duplicate does not keep
    /// the "guessed scheme" bit.
    pub fn dup(&self) -> Url {
        let mut u = self.clone();
        u.guessed_scheme = false;
        u
    }

    // -- parsing ----------------------------------------------------------

    /// parse_hostname_login(): returns the offset of the hostname
    fn parse_hostname_login(&mut self, login: &[u8], flags: u32) -> Result<usize, UCode> {
        let Some(at_pos) = find(login, b'@') else {
            self.user = None;
            self.password = None;
            self.options = None;
            return Ok(0);
        };
        let h = self.scheme.as_deref().and_then(|s| self.handler(s));
        let (userp, passwdp, optionsp) =
            parse_login_details(&login[..at_pos], h.is_some_and(|h| h.urloptions));
        if flags & CURLU_DISALLOW_USER != 0 {
            self.user = None;
            self.password = None;
            self.options = None;
            return Err(UCode::UserNotAllowed);
        }
        self.user = Some(userp);
        if passwdp.is_some() {
            self.password = passwdp;
        }
        if optionsp.is_some() {
            self.options = optionsp;
        }
        Ok(at_pos + 1)
    }

    /// Curl_parse_port()
    fn parse_port(&mut self, host: &mut Vec<u8>, has_scheme: bool) -> Result<(), UCode> {
        let portptr;
        if at(host, 0) == b'[' {
            let Some(rb) = find(host, b']') else {
                return Err(UCode::BadIpv6);
            };
            let p = rb + 1;
            if p < host.len() {
                if host[p] != b':' {
                    return Err(UCode::BadPortNumber);
                }
                portptr = Some(p);
            } else {
                portptr = None;
            }
        } else {
            portptr = find(host, b':');
        }
        if let Some(p) = portptr {
            let rest = host[p + 1..].to_vec();
            host.truncate(p);
            if rest.is_empty() {
                return if has_scheme {
                    Ok(())
                } else {
                    Err(UCode::BadPortNumber)
                };
            }
            match str_number(&rest, 0xffff) {
                Some((port, n)) if n == rest.len() => {
                    self.portnum = port as u16;
                    self.port = Some(port.to_string().into_bytes());
                }
                _ => return Err(UCode::BadPortNumber),
            }
        }
        Ok(())
    }

    /// ipv6_parse(): `host` starts with '['
    fn ipv6_parse(&mut self, host: &mut Vec<u8>) -> Result<(), UCode> {
        let total = host.len();
        if total < 4 {
            return Err(UCode::BadIpv6);
        }
        let mut hlen = total - 2;
        let len = strspn(&host[1..], b"0123456789abcdefABCDEF:.");
        if hlen != len {
            hlen = len;
            if at(host, 1 + len) == b'%' {
                // this could now be '%[zone id]'
                let mut h = 1 + len + 1;
                if at(host, h) == b'2'
                    && at(host, h + 1) == b'5'
                    && at(host, h + 2) != 0
                    && at(host, h + 2) != b']'
                {
                    h += 2;
                }
                let mut zoneid = Vec::new();
                while at(host, h) != 0 && at(host, h) != b']' && zoneid.len() < 15 {
                    zoneid.push(host[h]);
                    h += 1;
                }
                if zoneid.is_empty() || at(host, h) != b']' {
                    return Err(UCode::BadIpv6);
                }
                self.zoneid = Some(zoneid);
                host.truncate(1 + len);
                host.push(b']');
            } else {
                return Err(UCode::BadIpv6);
            }
        }
        // Normalize the IPv6 address
        let Some(bin) = inet_pton6(&host[1..1 + hlen]) else {
            return Err(UCode::BadIpv6);
        };
        let txt = inet_ntop6(&bin);
        // inet_ntop() is given a buffer of hlen bytes (including the NUL)
        if txt.len() < hlen {
            let mut n = Vec::with_capacity(txt.len() + 2);
            n.push(b'[');
            n.extend_from_slice(&txt);
            n.push(b']');
            *host = n;
        } else {
            host.truncate(1 + hlen);
            host.push(b']');
        }
        Ok(())
    }

    /// hostname_check()
    fn hostname_check(&mut self, hostname: &mut Vec<u8>) -> Result<(), UCode> {
        if hostname.is_empty() {
            Err(UCode::NoHost)
        } else if hostname[0] == b'[' {
            self.ipv6_parse(hostname)
        } else {
            let len = strcspn(hostname, b" \r\n\t/:#?!@{}[]\\$'\"^`*<>=;,+&()%");
            if len != hostname.len() {
                Err(UCode::BadHostname)
            } else {
                Ok(())
            }
        }
    }

    /// parse_authority(): returns the hostname
    fn parse_authority(
        &mut self,
        auth: &[u8],
        flags: u32,
        has_scheme: bool,
    ) -> Result<Vec<u8>, UCode> {
        let offset = self.parse_hostname_login(auth, flags)?;
        let mut d = DynBuf::new(CURL_MAX_INPUT_LENGTH);
        d.add(&auth[offset..])?;
        let mut host = d.buf;
        self.parse_port(&mut host, has_scheme)?;
        if host.is_empty() {
            return Err(UCode::NoHost);
        }
        match ipv4_normalize(&mut host) {
            HostType::Ipv4 => {}
            HostType::Ipv6 => self.ipv6_parse(&mut host)?,
            HostType::Name => {
                // urldecode_host()
                if find(&host, b'%').is_some() {
                    host = urldecode(&host, Reject::Ctrl).map_err(|_| UCode::BadHostname)?;
                }
                self.hostname_check(&mut host)?;
            }
        }
        Ok(host)
    }

    /// parseurl() into a fresh handle
    fn parseurl(&self, url: &[u8], flags: u32) -> Result<Url, UCode> {
        let mut u = Url::with_schemes(self.schemes);
        let urllen = junkscan(url, flags & CURLU_ALLOW_SPACE != 0)?;
        let schemelen = is_absolute_url(
            url,
            flags & (CURLU_GUESS_SCHEME | CURLU_DEFAULT_SCHEME) != 0,
        );
        let schemebuf = url[..schemelen].to_ascii_lowercase();
        let mut host: Option<Vec<u8>> = None;
        let mut path: usize;
        let mut pathlen: usize;

        if schemelen != 0 && schemebuf == b"file" {
            if urllen <= 6 {
                // file:/ is not enough to actually be a complete file: URL
                return Err(UCode::BadFileUrl);
            }
            path = 5;
            pathlen = urllen - 5;
            u.scheme = Some(b"file".to_vec());
            if at(url, path) == b'/' && at(url, path + 1) == b'/' {
                let mut ptr = path + 2;
                if at(url, ptr) != b'/' && !starts_with_url_drive_prefix(&url[ptr..]) {
                    if checkprefix(b"localhost/", &url[ptr..])
                        || checkprefix(b"127.0.0.1/", &url[ptr..])
                    {
                        ptr += 9;
                    } else {
                        // Invalid file://hostname/, expected localhost or
                        // 127.0.0.1 or none
                        return Err(UCode::BadFileUrl);
                    }
                }
                path = ptr;
                pathlen = urllen - ptr;
            }
            // Do not allow Windows drive letters when not in Windows.
            if (at(url, path) == b'/' && starts_with_url_drive_prefix(&url[path + 1..]))
                || starts_with_url_drive_prefix(&url[path..])
            {
                return Err(UCode::BadFileUrl);
            }
        } else {
            let mut schemep: Option<Vec<u8>> = None;

            let hostp = if schemelen != 0 {
                let mut i = 0;
                let mut p = schemelen + 1;
                while at(url, p) == b'/' && i < 4 {
                    p += 1;
                    i += 1;
                }
                if self.handler(&schemebuf).is_none() && flags & CURLU_NON_SUPPORT_SCHEME == 0 {
                    return Err(UCode::UnsupportedScheme);
                }
                schemep = Some(schemebuf.clone());
                if !(1..=3).contains(&i) {
                    return Err(UCode::BadSlashes);
                }
                p
            } else {
                if flags & (CURLU_DEFAULT_SCHEME | CURLU_GUESS_SCHEME) == 0 {
                    return Err(UCode::BadScheme);
                }
                if flags & CURLU_DEFAULT_SCHEME != 0 {
                    schemep = Some(DEFAULT_SCHEME.to_vec());
                }
                0
            };
            if let Some(s) = &schemep {
                u.scheme = Some(s.clone());
            }
            let hostlen = strcspn(&url[hostp..], b"/?#");
            path = hostp + hostlen;
            pathlen = urllen - path;
            if hostlen != 0 {
                let h = u.parse_authority(&url[hostp..hostp + hostlen], flags, schemelen != 0)?;
                if flags & CURLU_GUESS_SCHEME != 0 && schemep.is_none() {
                    let guess: &[u8] = if checkprefix(b"ftp.", &h) {
                        b"ftp"
                    } else if checkprefix(b"dict.", &h) {
                        b"dict"
                    } else if checkprefix(b"ldap.", &h) {
                        b"ldap"
                    } else if checkprefix(b"imap.", &h) {
                        b"imap"
                    } else if checkprefix(b"smtp.", &h) {
                        b"smtp"
                    } else if checkprefix(b"pop3.", &h) {
                        b"pop3"
                    } else {
                        b"http"
                    };
                    u.scheme = Some(guess.to_vec());
                    u.guessed_scheme = true;
                }
                host = Some(h);
            } else if flags & CURLU_NO_AUTHORITY != 0 {
                host = Some(Vec::new());
            } else {
                return Err(UCode::NoHost);
            }
        }

        let rest = &url[path..];
        let fragment = find(rest, b'#');
        if let Some(f) = fragment {
            let fraglen = pathlen - f;
            u.fragment_present = true;
            if fraglen > 1 {
                let src = &rest[f + 1..f + fraglen];
                if flags & CURLU_URLENCODE != 0 {
                    let mut enc = DynBuf::new(CURL_MAX_INPUT_LENGTH);
                    urlencode_str(&mut enc, src, true, false)?;
                    u.fragment = enc.into_opt();
                } else {
                    u.fragment = Some(src.to_vec());
                }
            }
            pathlen -= fraglen;
        }

        if let Some(q) = find(&rest[..pathlen], b'?') {
            let qlen = match fragment {
                Some(f) => f - q,
                None => pathlen - q,
            };
            pathlen -= qlen;
            u.query_present = true;
            if qlen > 1 {
                let src = &rest[q + 1..q + qlen];
                if flags & CURLU_URLENCODE != 0 {
                    let mut enc = DynBuf::new(CURL_MAX_INPUT_LENGTH);
                    urlencode_str(&mut enc, src, true, true)?;
                    u.query = enc.into_opt();
                } else {
                    u.query = Some(src.to_vec());
                }
            } else {
                u.query = Some(Vec::new());
            }
        }

        let mut pathbuf: &[u8] = &rest[..pathlen];
        let encoded;
        if pathlen != 0 && flags & CURLU_URLENCODE != 0 {
            let mut enc = DynBuf::new(CURL_MAX_INPUT_LENGTH);
            urlencode_str(&mut enc, pathbuf, true, false)?;
            encoded = enc.buf;
            u.path = Some(encoded.clone());
            pathbuf = &encoded;
        }
        if pathbuf.len() > 1 {
            if u.path.is_none() {
                u.path = Some(pathbuf.to_vec());
            }
            if flags & CURLU_PATH_AS_IS == 0
                && let Some(d) = dedotdotify(u.path.as_deref().unwrap_or_default())
            {
                u.path = Some(d);
            }
        }
        u.host = host;
        Ok(u)
    }

    /// parseurl_and_replace()
    fn parseurl_and_replace(&mut self, url: &[u8], flags: u32) -> Result<(), UCode> {
        let tmp = self.parseurl(url, flags)?;
        *self = tmp;
        Ok(())
    }

    /// redirect_url(): concatenate a relative URL onto a base URL
    fn redirect_url(&mut self, base: &[u8], relurl: &[u8], flags: u32) -> Result<(), UCode> {
        let mut host_changed = false;
        let mut useurl = relurl;
        let mut cutoff: Option<usize> = None;
        let protsep = self.scheme.as_ref().map_or(0, |s| s.len()) + 3;
        let ps = &base[protsep.min(base.len())..];
        let protsep = protsep.min(base.len());

        match relurl[0] {
            b'/' => {
                if at(relurl, 1) == b'/' {
                    // protocol-relative URL: //example.com/path
                    cutoff = Some(protsep);
                    useurl = &relurl[2..];
                    host_changed = true;
                } else {
                    cutoff = find(ps, b'/').map(|n| protsep + n);
                }
            }
            b'#' => {
                if self.fragment.is_some() {
                    cutoff = find(ps, b'#').map(|n| protsep + n);
                }
            }
            _ => {
                if self.query.as_ref().is_some_and(|q| !q.is_empty()) {
                    cutoff = find(ps, b'?').map(|n| protsep + n);
                } else if self.fragment.as_ref().is_some_and(|f| !f.is_empty()) {
                    cutoff = find(ps, b'#').map(|n| protsep + n);
                }
                if relurl[0] != b'?' {
                    let end = cutoff.map_or(ps.len(), |c| c - protsep);
                    cutoff = rfind(&ps[..end], b'/').map(|n| protsep + n + 1);
                }
            }
        }
        let prelen = cutoff.unwrap_or(base.len());
        let mut urlbuf = DynBuf::new(CURL_MAX_INPUT_LENGTH);
        if urlbuf.add(&base[..prelen]).is_err()
            || urlencode_str(&mut urlbuf, useurl, !host_changed, false).is_err()
        {
            return Err(UCode::OutOfMemory);
        }
        let newurl = urlbuf.buf;
        self.parseurl_and_replace(&newurl, flags & !CURLU_PATH_AS_IS)
    }

    // -- curl_url_get -----------------------------------------------------

    /// curl_url_get()
    pub fn get(&self, what: Part, flags: u32) -> Result<Vec<u8>, UCode> {
        let mut urldecode_f = flags & CURLU_URLDECODE != 0;
        let urlencode_f = flags & CURLU_URLENCODE != 0;
        let mut punycode = false;
        let mut depunyfy = false;
        let mut plusdecode = false;
        let ifmissing;
        let ptr: Option<Vec<u8>>;
        match what {
            Part::Scheme => {
                ptr = self.scheme.clone();
                ifmissing = UCode::NoScheme;
                urldecode_f = false;
                if flags & CURLU_NO_GUESS_SCHEME != 0 && self.guessed_scheme {
                    return Err(UCode::NoScheme);
                }
            }
            Part::User => {
                ptr = self.user.clone();
                ifmissing = UCode::NoUser;
            }
            Part::Password => {
                ptr = self.password.clone();
                ifmissing = UCode::NoPassword;
            }
            Part::Options => {
                ptr = self.options.clone();
                ifmissing = UCode::NoOptions;
            }
            Part::Host => {
                ptr = self.host.clone();
                ifmissing = UCode::NoHost;
                punycode = flags & CURLU_PUNYCODE != 0;
                depunyfy = flags & CURLU_PUNY2IDN != 0;
            }
            Part::Zoneid => {
                ptr = self.zoneid.clone();
                ifmissing = UCode::NoZoneid;
            }
            Part::Port => {
                ifmissing = UCode::NoPort;
                urldecode_f = false;
                let mut p = self.port.clone();
                if p.is_none() && flags & CURLU_DEFAULT_PORT != 0 && self.scheme.is_some() {
                    if let Some(h) = self.handler(self.scheme.as_deref().unwrap_or_default()) {
                        p = Some(h.defport.to_string().into_bytes());
                    }
                } else if p.is_some()
                    && self.scheme.is_some()
                    && let Some(h) = self.handler(self.scheme.as_deref().unwrap_or_default())
                    && h.defport == self.portnum
                    && flags & CURLU_NO_DEFAULT_PORT != 0
                {
                    p = None;
                }
                ptr = p;
            }
            Part::Path => {
                ifmissing = UCode::UnknownPart;
                ptr = Some(self.path.clone().unwrap_or_else(|| b"/".to_vec()));
            }
            Part::Query => {
                ifmissing = UCode::NoQuery;
                plusdecode = urldecode_f;
                ptr = match &self.query {
                    Some(q) if q.is_empty() && flags & CURLU_GET_EMPTY == 0 => None,
                    q => q.clone(),
                };
            }
            Part::Fragment => {
                ifmissing = UCode::NoFragment;
                ptr = match &self.fragment {
                    None if self.fragment_present && flags & CURLU_GET_EMPTY != 0 => {
                        Some(Vec::new())
                    }
                    f => f.clone(),
                };
            }
            Part::Url => return self.get_url(flags),
        }
        let Some(mut part) = ptr else {
            return Err(ifmissing);
        };
        if plusdecode {
            for c in part.iter_mut() {
                if *c == b'+' {
                    *c = b' ';
                }
            }
        }
        if urldecode_f {
            part = urldecode(&part, Reject::Ctrl).map_err(|_| UCode::Urldecode)?;
        }
        if urlencode_f {
            let mut enc = DynBuf::new(CURL_MAX_INPUT_LENGTH);
            urlencode_str(&mut enc, &part, true, what == Part::Query)?;
            part = enc.buf;
        } else if punycode {
            if !is_ascii_name(self.host.as_deref().unwrap_or_default()) {
                return Err(UCode::LacksIdn);
            }
        } else if depunyfy && is_ascii_name(self.host.as_deref().unwrap_or_default()) {
            return Err(UCode::LacksIdn);
        }
        Ok(part)
    }

    fn get_url(&self, flags: u32) -> Result<Vec<u8>, UCode> {
        let urlencode_f = flags & CURLU_URLENCODE != 0;
        let mut options = self.options.as_deref();
        let mut port = self.port.clone();
        let show_fragment =
            self.fragment.is_some() || (self.fragment_present && flags & CURLU_GET_EMPTY != 0);
        let show_query = self.query.as_ref().is_some_and(|q| !q.is_empty())
            || (self.query_present && flags & CURLU_GET_EMPTY != 0);
        let punycode = flags & CURLU_PUNYCODE != 0;
        let depunyfy = flags & CURLU_PUNY2IDN != 0;
        let mut url = Vec::new();
        if self
            .scheme
            .as_ref()
            .is_some_and(|s| s.eq_ignore_ascii_case(b"file"))
        {
            url.extend_from_slice(b"file://");
            // libcurl's printf renders a NULL %s as "(nil)"
            url.extend_from_slice(self.path.as_deref().unwrap_or(b"(nil)"));
            if show_query {
                url.push(b'?');
            }
            url.extend_from_slice(self.query.as_deref().unwrap_or_default());
            if show_fragment {
                url.push(b'#');
            }
            url.extend_from_slice(self.fragment.as_deref().unwrap_or_default());
            return Ok(url);
        }
        let Some(host) = self.host.as_deref() else {
            return Err(UCode::NoHost);
        };
        let scheme: &[u8] = if let Some(s) = &self.scheme {
            s
        } else if flags & CURLU_DEFAULT_SCHEME != 0 {
            DEFAULT_SCHEME
        } else {
            return Err(UCode::NoScheme);
        };
        let h = self.handler(scheme);
        if port.is_none() && flags & CURLU_DEFAULT_PORT != 0 {
            if let Some(h) = h {
                port = Some(h.defport.to_string().into_bytes());
            }
        } else if port.is_some()
            && let Some(h) = h
            && h.defport == self.portnum
            && flags & CURLU_NO_DEFAULT_PORT != 0
        {
            port = None;
        }
        if h.is_some_and(|h| !h.urloptions) {
            options = None;
        }
        let mut allochost: Option<Vec<u8>> = None;
        if at(host, 0) == b'[' {
            if let Some(z) = &self.zoneid {
                let mut e = host[..host.len() - 1].to_vec();
                e.extend_from_slice(b"%25");
                e.extend_from_slice(z);
                e.push(b']');
                allochost = Some(e);
            }
        } else if urlencode_f {
            allochost = Some(easy_escape(host));
        } else if punycode {
            if !is_ascii_name(host) {
                return Err(UCode::LacksIdn);
            }
        } else if depunyfy && is_ascii_name(host) {
            return Err(UCode::LacksIdn);
        }
        if flags & CURLU_NO_GUESS_SCHEME == 0 || !self.guessed_scheme {
            url.extend_from_slice(scheme);
            url.extend_from_slice(b"://");
        }
        if let Some(u) = &self.user {
            url.extend_from_slice(u);
        }
        if let Some(p) = &self.password {
            url.push(b':');
            url.extend_from_slice(p);
        }
        if let Some(o) = options {
            url.push(b';');
            url.extend_from_slice(o);
        }
        if self.user.is_some() || self.password.is_some() || options.is_some() {
            url.push(b'@');
        }
        url.extend_from_slice(allochost.as_deref().unwrap_or(host));
        if let Some(p) = &port {
            url.push(b':');
            url.extend_from_slice(p);
        }
        url.extend_from_slice(self.path.as_deref().unwrap_or(b"/"));
        if show_query {
            url.push(b'?');
        }
        url.extend_from_slice(self.query.as_deref().unwrap_or_default());
        if show_fragment {
            url.push(b'#');
        }
        url.extend_from_slice(self.fragment.as_deref().unwrap_or_default());
        Ok(url)
    }

    // -- curl_url_set -----------------------------------------------------

    /// curl_url_set(). `None` clears the part (for [`Part::Url`]: the whole
    /// handle).
    pub fn set(&mut self, what: Part, value: Option<&[u8]>, flags: u32) -> Result<(), UCode> {
        let Some(part) = value else {
            match what {
                Part::Url => {
                    *self = Url::with_schemes(self.schemes);
                }
                Part::Scheme => {
                    self.scheme = None;
                    self.guessed_scheme = false;
                }
                Part::User => self.user = None,
                Part::Password => self.password = None,
                Part::Options => self.options = None,
                Part::Host => self.host = None,
                Part::Zoneid => self.zoneid = None,
                Part::Port => {
                    self.portnum = 0;
                    self.port = None;
                }
                Part::Path => self.path = None,
                Part::Query => {
                    self.query = None;
                    self.query_present = false;
                }
                Part::Fragment => {
                    self.fragment = None;
                    self.fragment_present = false;
                }
            }
            return Ok(());
        };
        let part = cstr(part);
        let nalloc = part.len();
        if nalloc > CURL_MAX_INPUT_LENGTH {
            return Err(UCode::MalformedInput);
        }
        let mut urlencode_f = flags & CURLU_URLENCODE != 0;
        let mut plusencode = false;
        let mut urlskipslash = false;
        let mut leadingslash = false;
        let mut appendquery = false;
        let mut equalsencode = false;

        match what {
            Part::Scheme => {
                self.set_url_scheme(part, flags)?;
                urlencode_f = false;
            }
            Part::User | Part::Password | Part::Options | Part::Zoneid | Part::Fragment => {}
            Part::Host => {
                self.zoneid = None;
            }
            Part::Port => return self.set_url_port(part),
            Part::Path => {
                urlskipslash = true;
                leadingslash = true;
            }
            Part::Query => {
                plusencode = urlencode_f;
                appendquery = flags & CURLU_APPENDQUERY != 0;
                equalsencode = appendquery;
                self.query_present = true;
            }
            Part::Url => return self.set_url(part, flags),
        }
        if what == Part::Fragment {
            self.fragment_present = true;
        }

        let mut enc = DynBuf::new(nalloc * 3 + 1 + leadingslash as usize);
        if leadingslash && at(part, 0) != b'/' {
            enc.add(b"/")?;
        }
        if urlencode_f {
            let mut tmp = Vec::with_capacity(nalloc * 3);
            for &c in part {
                if c == b' ' && plusencode {
                    tmp.push(b'+');
                } else if is_unreserved(c)
                    || (c == b'/' && urlskipslash)
                    || (c == b'=' && equalsencode)
                {
                    if c == b'=' && equalsencode {
                        // only skip the first equals sign
                        equalsencode = false;
                    }
                    tmp.push(c);
                } else {
                    push_hexbyte(&mut tmp, c, true);
                }
            }
            if !tmp.is_empty() {
                enc.add(&tmp)?;
            }
        } else {
            let start = enc.buf.len();
            enc.add(part)?;
            // make sure percent encoded are lower case
            let p = &mut enc.buf;
            let mut i = start;
            while i < p.len() {
                if p[i] == b'%'
                    && at(p, i + 1).is_ascii_hexdigit()
                    && at(p, i + 2).is_ascii_hexdigit()
                    && (p[i + 1].is_ascii_uppercase() || p[i + 2].is_ascii_uppercase())
                {
                    p[i + 1] = p[i + 1].to_ascii_lowercase();
                    p[i + 2] = p[i + 2].to_ascii_lowercase();
                    i += 3;
                } else {
                    i += 1;
                }
            }
        }
        let n = enc.buf.len();
        let mut newp = enc.into_opt();

        if appendquery && newp.is_some() {
            if let Some(q) = self.query.as_ref().filter(|q| !q.is_empty()) {
                let mut qbuf = DynBuf::new(CURL_MAX_INPUT_LENGTH);
                let addamperand = q.last() != Some(&b'&');
                qbuf.add(q).map_err(|_| UCode::OutOfMemory)?;
                if addamperand {
                    qbuf.add(b"&").map_err(|_| UCode::OutOfMemory)?;
                }
                qbuf.add(newp.as_deref().unwrap_or_default())
                    .map_err(|_| UCode::OutOfMemory)?;
                self.query = Some(qbuf.buf);
                return Ok(());
            }
        } else if what == Part::Host {
            if n == 0 && flags & CURLU_NO_AUTHORITY != 0 {
                // Skip hostname check, it is allowed to be empty.
            } else {
                let bad = if n == 0 {
                    true
                } else if !urlencode_f {
                    // the host name part was set ready URL encoded so decode
                    // it to check
                    match urldecode(newp.as_deref().unwrap_or_default(), Reject::Ctrl) {
                        Ok(mut decoded) => self.hostname_check(&mut decoded).is_err(),
                        Err(()) => true,
                    }
                } else {
                    let mut v = newp.take().unwrap_or_default();
                    let r = self.hostname_check(&mut v).is_err();
                    newp = Some(v);
                    r
                };
                if bad {
                    return Err(UCode::BadHostname);
                }
            }
        }
        let store = match what {
            Part::Scheme => &mut self.scheme,
            Part::User => &mut self.user,
            Part::Password => &mut self.password,
            Part::Options => &mut self.options,
            Part::Host => &mut self.host,
            Part::Zoneid => &mut self.zoneid,
            Part::Path => &mut self.path,
            Part::Query => &mut self.query,
            Part::Fragment => &mut self.fragment,
            Part::Url | Part::Port => unreachable!(),
        };
        *store = newp;
        Ok(())
    }

    fn set_url_scheme(&mut self, scheme: &[u8], flags: u32) -> Result<(), UCode> {
        let plen = scheme.len();
        if !(1..=MAX_SCHEME_LEN).contains(&plen) {
            return Err(UCode::BadScheme);
        }
        if flags & CURLU_NON_SUPPORT_SCHEME == 0 && self.handler(scheme).is_none() {
            return Err(UCode::UnsupportedScheme);
        }
        if !scheme[0].is_ascii_alphabetic() {
            return Err(UCode::BadScheme);
        }
        // ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) -- libcurl checks all
        // but the last byte
        for &c in &scheme[..plen - 1] {
            if !(c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.') {
                return Err(UCode::BadScheme);
            }
        }
        self.guessed_scheme = false;
        Ok(())
    }

    fn set_url_port(&mut self, provided: &[u8]) -> Result<(), UCode> {
        if !at(provided, 0).is_ascii_digit() {
            return Err(UCode::BadPortNumber);
        }
        match str_number(provided, 0xffff) {
            Some((port, n)) if n == provided.len() => {
                self.port = Some(port.to_string().into_bytes());
                self.portnum = port as u16;
                Ok(())
            }
            _ => Err(UCode::BadPortNumber),
        }
    }

    fn set_url(&mut self, url: &[u8], flags: u32) -> Result<(), UCode> {
        if url.is_empty() {
            // a blank URL is not a valid URL unless we already have a
            // complete one and this is a redirect
            return match self.get(Part::Url, flags) {
                Ok(_) => Ok(()),
                Err(_) => Err(UCode::MalformedInput),
            };
        }
        if is_absolute_url(
            url,
            flags & (CURLU_GUESS_SCHEME | CURLU_DEFAULT_SCHEME) != 0,
        ) != 0
        {
            return self.parseurl_and_replace(url, flags);
        }
        match self.get(Part::Url, flags) {
            Err(_) => self.parseurl_and_replace(url, flags),
            Ok(oldurl) => self.redirect_url(&oldurl, url, flags),
        }
    }
}

/// ipv4_normalize(): handle partial IPv4 numerical addresses and different
/// bases, like '16843009', '0x7f', '0x7f.1' '0177.1.1.1' etc.
fn ipv4_normalize(host: &mut Vec<u8>) -> HostType {
    if at(host, 0) == b'[' {
        return HostType::Ipv6;
    }
    let mut c = 0usize;
    let mut n = 0usize;
    let mut parts = [0u64; 4];
    loop {
        let r = if at(host, c) == b'0' {
            if at(host, c + 1) == b'x' {
                c += 2;
                str_num_base(&host[c.min(host.len())..], u32::MAX as u64, 16)
            } else {
                str_num_base(&host[c..], u32::MAX as u64, 8)
            }
        } else {
            str_number(&host[c..], u32::MAX as u64)
        };
        let Some((l, adv)) = r else {
            return HostType::Name;
        };
        c += adv;
        parts[n] = l;
        match at(host, c) {
            b'.' => {
                if n == 3 {
                    return HostType::Name;
                }
                n += 1;
                c += 1;
            }
            0 => break,
            _ => return HostType::Name,
        }
    }
    let s = match n {
        0 => format!(
            "{}.{}.{}.{}",
            parts[0] >> 24,
            (parts[0] >> 16) & 0xff,
            (parts[0] >> 8) & 0xff,
            parts[0] & 0xff
        ),
        1 => {
            if parts[0] > 0xff || parts[1] > 0xffffff {
                return HostType::Name;
            }
            format!(
                "{}.{}.{}.{}",
                parts[0],
                (parts[1] >> 16) & 0xff,
                (parts[1] >> 8) & 0xff,
                parts[1] & 0xff
            )
        }
        2 => {
            if parts[0] > 0xff || parts[1] > 0xff || parts[2] > 0xffff {
                return HostType::Name;
            }
            format!(
                "{}.{}.{}.{}",
                parts[0],
                parts[1],
                (parts[2] >> 8) & 0xff,
                parts[2] & 0xff
            )
        }
        _ => {
            if parts.iter().any(|p| *p > 0xff) {
                return HostType::Name;
            }
            format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3])
        }
    };
    *host = s.into_bytes();
    HostType::Ipv4
}

// ---------------------------------------------------------------------------
// url.c: parseurlandfillconn() and friends
// ---------------------------------------------------------------------------

/// A CURLcode plus the text libcurl writes to the error buffer (empty when
/// libcurl does not call failf() for that error).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurlError {
    pub code: u32,
    pub message: String,
}

impl CurlError {
    fn code(code: u32) -> CurlError {
        CurlError {
            code,
            message: String::new(),
        }
    }
}

/// The message findprotocol() writes: `Protocol "foo" not supported`,
/// `Protocol "foo" disabled`, with " (in redirect)" appended for follows.
pub fn protocol_error_text(scheme: &[u8], found_but_disabled: bool, in_redirect: bool) -> String {
    format!(
        "Protocol \"{}\" {}{}",
        String::from_utf8_lossy(scheme),
        if found_but_disabled {
            "disabled"
        } else {
            "not supported"
        },
        if in_redirect { " (in redirect)" } else { "" }
    )
}

/// Settings of the easy handle that influence URL parsing.
#[derive(Clone, Debug)]
pub struct TransferOptions<'a> {
    /// CURLOPT_DEFAULT_PROTOCOL
    pub default_protocol: Option<&'a [u8]>,
    /// CURLOPT_PATH_AS_IS
    pub path_as_is: bool,
    /// CURLOPT_DISALLOW_USERNAME_IN_URL
    pub disallow_username_in_url: bool,
    /// data->state.this_is_a_follow (the URL comes from a redirect)
    pub this_is_a_follow: bool,
    /// CURLOPT_PROTOCOLS(_STR) mask
    pub allowed_protocols: u64,
    /// CURLOPT_REDIR_PROTOCOLS(_STR) mask
    pub redir_protocols: u64,
    /// scheme table ("built in" protocols)
    pub schemes: &'static [SchemeInfo],
}

impl Default for TransferOptions<'_> {
    fn default() -> Self {
        TransferOptions {
            default_protocol: None,
            path_as_is: false,
            disallow_username_in_url: false,
            this_is_a_follow: false,
            allowed_protocols: CURLPROTO_ALL,
            redir_protocols: CURLPROTO_REDIR,
            schemes: SCHEMES,
        }
    }
}

/// What parseurlandfillconn() derives from the URL.
#[derive(Clone, Debug)]
pub struct TransferUrl {
    /// data->state.uh: the parsed handle (keep it, redirects resolve
    /// against it, see [`http_follow`])
    pub uh: Url,
    /// data->state.url: the normalized URL (CURLINFO_EFFECTIVE_URL)
    pub url: Vec<u8>,
    /// data->state.up.scheme (as stored, e.g. "http")
    pub scheme: Vec<u8>,
    /// conn->handler
    pub handler: &'static SchemeInfo,
    /// data->state.up.hostname (IPv6 literals keep their brackets); None
    /// for file:// URLs
    pub hostname: Option<Vec<u8>>,
    /// conn->host.name: hostname with IPv6 brackets cut off ("" for file)
    pub host_name: Vec<u8>,
    /// conn->bits.ipv6_ip
    pub ipv6_ip: bool,
    /// zone id of an IPv6 literal (raw, as in the URL)
    pub zoneid: Option<Vec<u8>>,
    /// conn->scope_id when the zone id starts with a number; a non-numeric
    /// one needs if_nametoindex() which is left to the caller
    pub scope_id: Option<u32>,
    /// conn->user: URL-decoded user name from the URL
    pub user: Option<Vec<u8>>,
    /// conn->passwd: URL-decoded password from the URL
    pub password: Option<Vec<u8>>,
    /// conn->options: URL-decoded login options (IMAP/POP3/SMTP)
    pub options: Option<Vec<u8>>,
    /// data->state.up.path (URL-encoded, starts with "/" except odd file:
    /// URLs)
    pub path: Vec<u8>,
    /// data->state.up.query (None when absent or empty)
    pub query: Option<Vec<u8>>,
    /// data->state.up.port (the port or the scheme default, as a string)
    pub port_str: Option<Vec<u8>>,
    /// conn->remote_port as derived from the URL (CURLOPT_PORT not applied)
    pub port: Option<u16>,
}

impl TransferUrl {
    /// The HTTP request-target when not talking to a plain HTTP proxy
    /// (http_target()): path, plus "?query" when there is one.
    pub fn request_target(&self) -> Vec<u8> {
        let mut r = self.path.clone();
        if let Some(q) = &self.query {
            r.push(b'?');
            r.extend_from_slice(q);
        }
        r
    }

    /// The value of the Host: header http_host() generates for
    /// `remote_port` (conn->remote_port, i.e. after CURLOPT_PORT): IPv6
    /// literals in brackets, ":port" unless it is the default one.
    pub fn host_header(&self, remote_port: u16) -> Vec<u8> {
        let p = self.handler.protocol;
        let mut h = Vec::new();
        if self.ipv6_ip {
            h.push(b'[');
        }
        h.extend_from_slice(&self.host_name);
        if self.ipv6_ip {
            h.push(b']');
        }
        let default = (p & (CURLPROTO_HTTPS | CURLPROTO_WSS) != 0 && remote_port == 443)
            || (p & (CURLPROTO_HTTP | CURLPROTO_WS) != 0 && remote_port == 80);
        if !default {
            h.extend_from_slice(format!(":{remote_port}").as_bytes());
        }
        h
    }

    /// The absolute URL sent as request-target to a non-tunnelling HTTP
    /// proxy (http_target()): no fragment, no credentials for http://, and
    /// no port number when it is the scheme default.
    pub fn proxy_request_url(&self) -> Vec<u8> {
        let mut h = self.uh.dup();
        let _ = h.set(Part::Fragment, None, 0);
        if self.scheme.eq_ignore_ascii_case(b"http") {
            let _ = h.set(Part::User, None, 0);
            let _ = h.set(Part::Password, None, 0);
        }
        h.get(Part::Url, CURLU_NO_DEFAULT_PORT).unwrap_or_default()
    }
}

/// parseurlandfillconn() (url.c) for a URL string set with CURLOPT_URL
/// (or a redirect target; set `opts.this_is_a_follow`). On failure returns
/// the CURLcode and the error buffer text.
pub fn parse_transfer_url(url: &[u8], opts: &TransferOptions) -> Result<TransferUrl, CurlError> {
    let mut url = cstr(url).to_vec();
    if let Some(dp) = opts.default_protocol
        && is_absolute_url(&url, true) == 0
    {
        let mut n = cstr(dp).to_vec();
        n.extend_from_slice(b"://");
        n.extend_from_slice(&url);
        url = n;
    }
    let mut uh = Url::with_schemes(opts.schemes);
    let flags = CURLU_GUESS_SCHEME
        | CURLU_NON_SUPPORT_SCHEME
        | if opts.disallow_username_in_url {
            CURLU_DISALLOW_USER
        } else {
            0
        }
        | if opts.path_as_is { CURLU_PATH_AS_IS } else { 0 };
    if let Err(uc) = uh.set(Part::Url, Some(&url), flags) {
        return Err(CurlError {
            code: uc_to_curlcode(uc),
            message: format!("URL rejected: {}", url_strerror(uc)),
        });
    }
    let uce = |uc: UCode| CurlError::code(uc_to_curlcode(uc));
    let newurl = uh.get(Part::Url, 0).map_err(uce)?;
    let scheme = uh.get(Part::Scheme, 0).map_err(uce)?;
    let is_file = scheme.eq_ignore_ascii_case(b"file");
    let hostname = match uh.get(Part::Host, 0) {
        Err(_) => {
            if !is_file {
                return Err(CurlError::code(CURLE_OUT_OF_MEMORY));
            }
            None
        }
        Ok(h) => {
            if h.len() > MAX_URL_LEN {
                return Err(CurlError {
                    code: CURLE_URL_MALFORMAT,
                    message: format!("Too long hostname (maximum is {MAX_URL_LEN})"),
                });
            }
            Some(h)
        }
    };
    let mut host_name = hostname.clone().unwrap_or_default();
    let mut ipv6_ip = false;
    let mut zoneid = None;
    let mut scope_id = None;
    if at(&host_name, 0) == b'[' {
        ipv6_ip = true;
        host_name.remove(0);
        host_name.pop();
        if let Ok(z) = uh.get(Part::Zoneid, 0) {
            if let Some((n, _)) = str_number(&z, u32::MAX as u64) {
                scope_id = Some(n as u32);
            }
            zoneid = Some(z);
        }
    }

    // findprotocol()
    let handler = match uh.handler(&scheme) {
        Some(p)
            if opts.allowed_protocols & p.protocol != 0
                && !(opts.this_is_a_follow && opts.redir_protocols & p.protocol == 0) =>
        {
            p
        }
        p => {
            return Err(CurlError {
                code: CURLE_UNSUPPORTED_PROTOCOL,
                message: protocol_error_text(&scheme, p.is_some(), opts.this_is_a_follow),
            });
        }
    };
    let reject = if handler.userpwdctrl {
        Reject::Zero
    } else {
        Reject::Ctrl
    };
    let password = match uh.get(Part::Password, 0) {
        Ok(p) => Some(urldecode(&p, reject).map_err(|_| CurlError::code(CURLE_URL_MALFORMAT))?),
        Err(UCode::NoPassword) => None,
        Err(uc) => return Err(uce(uc)),
    };
    let user = match uh.get(Part::User, 0) {
        Ok(p) => Some(urldecode(&p, reject).map_err(|_| CurlError::code(CURLE_URL_MALFORMAT))?),
        Err(UCode::NoUser) => None,
        Err(uc) => return Err(uce(uc)),
    };
    let options = match uh.get(Part::Options, CURLU_URLDECODE) {
        Ok(o) => Some(o),
        Err(UCode::NoOptions) => None,
        Err(uc) => return Err(uce(uc)),
    };
    let path = uh.get(Part::Path, CURLU_URLENCODE).map_err(uce)?;
    let (port_str, port) = match uh.get(Part::Port, CURLU_DEFAULT_PORT) {
        Err(_) => {
            if !is_file {
                return Err(CurlError::code(CURLE_OUT_OF_MEMORY));
            }
            (None, None)
        }
        Ok(p) => {
            let n = str_number(&p, 0xffff).map(|(n, _)| n as u16);
            (Some(p), n)
        }
    };
    let query = uh.get(Part::Query, 0).ok();
    Ok(TransferUrl {
        uh,
        url: newurl,
        scheme,
        handler,
        hostname,
        host_name,
        ipv6_ip,
        zoneid,
        scope_id,
        user,
        password,
        options,
        path,
        query,
        port_str,
        port,
    })
}

// ---------------------------------------------------------------------------
// http.c: Curl_http_follow() and friends
// ---------------------------------------------------------------------------

/// followtype (http.h)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowType {
    /// only records stuff, not actually following (CURLINFO_REDIRECT_URL)
    Fake,
    /// a request retry as opposed to a real redirect
    Retry,
    /// a full true redirect
    Redir,
}

/// Inputs of Curl_http_follow() besides the handle and the new URL.
#[derive(Clone, Debug)]
pub struct FollowParams {
    pub kind: FollowType,
    /// CURLOPT_PATH_AS_IS
    pub path_as_is: bool,
    /// data->req.httpcode of the response carrying the Location
    pub httpcode: u16,
    /// CURLOPT_UNRESTRICTED_AUTH
    pub allow_auth_to_other_hosts: bool,
    /// CURLOPT_PORT when it is in effect (data->set.use_port &&
    /// data->state.allow_port)
    pub custom_port: Option<u16>,
    /// data->info.conn_remote_port
    pub conn_remote_port: u16,
    /// data->info.conn_protocol (CURLPROTO_* of the current connection)
    pub conn_protocol: u64,
}

impl Default for FollowParams {
    fn default() -> Self {
        FollowParams {
            kind: FollowType::Redir,
            path_as_is: false,
            httpcode: 302,
            allow_auth_to_other_hosts: false,
            custom_port: None,
            conn_remote_port: 0,
            conn_protocol: 0,
        }
    }
}

/// Outcome of [`http_follow`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowResult {
    /// The new URL (data->state.url for a real follow, or
    /// data->info.wouldredirect, i.e. CURLINFO_REDIRECT_URL, for Fake)
    pub url: Vec<u8>,
    /// The new URL is absolute (and this is no retry and no 401/407): the
    /// caller must stop honouring CURLOPT_PORT (state.allow_port = FALSE)
    pub disallow_port: bool,
    /// Credentials must be cleared (port or protocol changed and
    /// CURLOPT_UNRESTRICTED_AUTH is not set)
    pub clear_auth: bool,
}

/// The URL part of Curl_http_follow(): resolve `newurl` (a Location header
/// value) against `uh` (data->state.uh, updated in place like libcurl
/// does). Counting of redirects, CURLOPT_MAXREDIRS ("Maximum (%ld)
/// redirects followed", CURLE_TOO_MANY_REDIRECTS), method switching and
/// referer handling are left to the caller.
pub fn http_follow(
    uh: &mut Url,
    newurl: &[u8],
    p: &FollowParams,
) -> Result<FollowResult, CurlError> {
    let newurl = cstr(newurl);
    let disallow_port = p.kind != FollowType::Retry
        && p.httpcode != 401
        && p.httpcode != 407
        && is_absolute_url(newurl, false) != 0;
    let flags = if p.kind == FollowType::Fake {
        CURLU_NON_SUPPORT_SCHEME
    } else {
        (if p.kind == FollowType::Redir {
            CURLU_URLENCODE
        } else {
            0
        }) | CURLU_ALLOW_SPACE
            | if p.path_as_is { CURLU_PATH_AS_IS } else { 0 }
    };
    let mut clear_auth = false;
    let follow_url;
    match uh.set(Part::Url, Some(newurl), flags) {
        Err(uc) => {
            if p.kind != FollowType::Fake {
                return Err(CurlError {
                    code: uc_to_curlcode(uc),
                    message: format!(
                        "The redirect target URL could not be parsed: {}",
                        url_strerror(uc)
                    ),
                });
            }
            // the URL could not be parsed for some reason, but since this is
            // FAKE mode, just duplicate the field as-is
            follow_url = newurl.to_vec();
        }
        Ok(()) => {
            let uce = |uc: UCode| CurlError::code(uc_to_curlcode(uc));
            follow_url = uh.get(Part::Url, 0).map_err(uce)?;
            if !p.allow_auth_to_other_hosts && p.kind != FollowType::Fake {
                let port: i64 = match p.custom_port {
                    Some(cp) => cp as i64,
                    None => {
                        let s = uh.get(Part::Port, CURLU_DEFAULT_PORT).map_err(uce)?;
                        atoi(&s)
                    }
                };
                if port != p.conn_remote_port as i64 {
                    clear_auth = true;
                } else {
                    let scheme = uh.get(Part::Scheme, 0).map_err(uce)?;
                    if let Some(h) = uh.handler(&scheme)
                        && h.protocol != p.conn_protocol
                    {
                        clear_auth = true;
                    }
                }
            }
        }
    }
    Ok(FollowResult {
        url: follow_url,
        disallow_port,
        clear_auth,
    })
}

fn atoi(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (b'\t'..=b'\r').contains(&s[i])) {
        i += 1;
    }
    let neg = match at(s, i) {
        b'-' => {
            i += 1;
            true
        }
        b'+' => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut n: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        n = n.saturating_mul(10).saturating_add((s[i] - b'0') as i64);
        i += 1;
    }
    if neg { -n } else { n }
}

/// The Referer libcurl sets with CURLOPT_AUTOREFERER when following away
/// from `url` (data->state.url): the URL without fragment and credentials.
/// None means libcurl fails with CURLE_OUT_OF_MEMORY.
pub fn auto_referer(url: &[u8], schemes: &'static [SchemeInfo]) -> Option<Vec<u8>> {
    let mut u = Url::with_schemes(schemes);
    u.set(Part::Url, Some(url), 0).ok()?;
    u.set(Part::Fragment, None, 0).ok()?;
    u.set(Part::User, None, 0).ok()?;
    u.set(Part::Password, None, 0).ok()?;
    u.get(Part::Url, 0).ok()
}

/// Curl_copy_header_value(): the trimmed value of a header line such as
/// "Location: /x\r\n". None for a line without a "name:" (or "name;").
/// libcurl ignores a Location whose value is empty.
pub fn copy_header_value(header: &[u8]) -> Option<Vec<u8>> {
    let header = cstr(header);
    let nlen = strcspn(header, b";:");
    if nlen == 0 {
        return None;
    }
    let sep = at(header, nlen);
    if sep != b':' && sep != b';' {
        return None;
    }
    let rest = &header[nlen + 1..];
    let end = rest
        .iter()
        .position(|&c| c == b'\r' || c == b'\n')
        .unwrap_or(rest.len());
    let mut v = &rest[..end];
    while let [b' ' | b'\t', tail @ ..] = v {
        v = tail;
    }
    while let [head @ .., b' ' | b'\t'] = v {
        v = head;
    }
    Some(v.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The protocols built into the reference libcurl 8.14.1 the expected
    /// values were produced with (curl-config --protocols).
    static REF_SCHEMES: &[SchemeInfo] = &[
        scheme("http", 80, CURLPROTO_HTTP, false, true, true),
        scheme("https", 443, CURLPROTO_HTTPS, false, true, true),
        scheme("file", 0, CURLPROTO_FILE, false, false, true),
        scheme("ftp", 21, CURLPROTO_FTP, false, false, true),
        scheme("ftps", 990, CURLPROTO_FTPS, false, false, true),
        scheme("ws", 80, CURLPROTO_WS, false, true, true),
        scheme("wss", 443, CURLPROTO_WSS, false, true, true),
        scheme("dict", 2628, CURLPROTO_DICT, false, false, true),
        scheme("ldap", 389, CURLPROTO_LDAP, false, false, false),
        scheme("ldaps", 636, CURLPROTO_LDAPS, false, false, false),
        scheme("imap", 143, CURLPROTO_IMAP, true, false, true),
        scheme("imaps", 993, CURLPROTO_IMAPS, true, false, true),
        scheme("pop3", 110, CURLPROTO_POP3, true, false, true),
        scheme("pop3s", 995, CURLPROTO_POP3S, true, false, true),
        scheme("smtp", 25, CURLPROTO_SMTP, true, false, true),
        scheme("smtps", 465, CURLPROTO_SMTPS, true, false, true),
        scheme("telnet", 23, CURLPROTO_TELNET, false, false, true),
        scheme("tftp", 69, CURLPROTO_TFTP, false, false, true),
        scheme("gopher", 70, CURLPROTO_GOPHER, false, false, true),
        scheme("gophers", 70, CURLPROTO_GOPHERS, false, false, true),
        scheme("rtsp", 554, CURLPROTO_RTSP, false, false, true),
        scheme("smb", 445, CURLPROTO_SMB, false, false, true),
        scheme("smbs", 445, CURLPROTO_SMBS, false, false, true),
        scheme("scp", 22, CURLPROTO_SCP, false, false, false),
        scheme("sftp", 22, CURLPROTO_SFTP, false, false, false),
        scheme("mqtt", 1883, CURLPROTO_MQTT, false, false, true),
        scheme("rtmp", 1935, CURLPROTO_RTMP, false, false, false),
    ];

    fn unesc(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'\\') {
                out.push(b'\\');
                i += 2;
            } else if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
                out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).unwrap());
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn esc_all(s: &[u8]) -> String {
        let mut o = String::new();
        for &c in s {
            if c == b'\\' {
                o.push_str("\\\\");
            } else if !(0x20..0x7f).contains(&c) {
                o.push_str(&format!("\\x{c:02x}"));
            } else {
                o.push(c as char);
            }
        }
        o
    }

    /// what the harness prints for a C string
    fn esc(s: &[u8]) -> String {
        esc_all(cstr(s))
    }

    fn parse_pf(op: &str) -> (Part, u32, &str) {
        let (p, rest) = op.split_once(':').unwrap();
        let (f, v) = rest.split_once(':').unwrap_or((rest, ""));
        (
            Part::from_u32(p.parse().unwrap()).unwrap(),
            f.parse().unwrap(),
            v,
        )
    }

    fn ures(r: Result<(), UCode>) -> String {
        match r {
            Ok(()) => "0".to_string(),
            Err(e) => format!("E{}", e.code()),
        }
    }

    /// Run one line of the op language of the C harness (see the generator
    /// notes in the report: S set, Z set NULL, G get, D dup, X escape, U
    /// unescape, R/F/T follow, P transfer parse).
    fn run(line: &str) -> String {
        let mut u = Url::with_schemes(REF_SCHEMES);
        let mut res = Vec::new();
        for op in line.split('\t').filter(|s| !s.is_empty()) {
            let (kind, rest) = op.split_at(1);
            let r = match kind {
                "S" => {
                    let (p, f, v) = parse_pf(rest);
                    ures(u.set(p, Some(&unesc(v)), f))
                }
                "Z" => {
                    let (p, f, _) = parse_pf(rest);
                    ures(u.set(p, None, f))
                }
                "G" => {
                    let (p, f, _) = parse_pf(rest);
                    match u.get(p, f) {
                        Ok(v) => format!("={}", esc(&v)),
                        Err(e) => format!("E{}", e.code()),
                    }
                }
                "D" => {
                    u = u.dup();
                    "D".to_string()
                }
                "X" => format!("={}", esc(&easy_escape(&unesc(rest)))),
                // the harness prints olen bytes, embedded NULs included
                "U" => format!("={}", esc_all(&easy_unescape(&unesc(rest)))),
                "R" | "F" | "T" => {
                    let kind = match kind {
                        "R" => FollowType::Redir,
                        "F" => FollowType::Fake,
                        _ => FollowType::Retry,
                    };
                    let p = FollowParams {
                        kind,
                        ..FollowParams::default()
                    };
                    match http_follow(&mut u, &unesc(rest), &p) {
                        Ok(r) => format!("={}", esc(&r.url)),
                        Err(e) => {
                            // the harness prints the CURLUcode
                            let uc = (0..32)
                                .filter_map(UCode::from_u32)
                                .find(|uc| e.message.ends_with(url_strerror(*uc)))
                                .unwrap();
                            format!("E{}", uc.code())
                        }
                    }
                }
                "P" => {
                    let mut it = rest.splitn(4, '|');
                    let dp = it.next().unwrap();
                    let pai = it.next().unwrap() == "1";
                    let du = it.next().unwrap() == "1";
                    let url = unesc(it.next().unwrap());
                    let opts = TransferOptions {
                        default_protocol: if dp == "-" { None } else { Some(dp.as_bytes()) },
                        path_as_is: pai,
                        disallow_username_in_url: du,
                        schemes: REF_SCHEMES,
                        ..TransferOptions::default()
                    };
                    match parse_transfer_url(&url, &opts) {
                        Ok(t) => format!("OK|{}", esc(&t.url)),
                        Err(e) => format!("C{}|{}|", e.code, e.message),
                    }
                }
                _ => panic!("bad op {op}"),
            };
            res.push(r);
        }
        res.join("\t")
    }

    #[test]
    fn against_libcurl_8_14_1() {
        let mut failures = Vec::new();
        for (inp, want) in CASES {
            let got = run(inp);
            if got != *want {
                failures.push(format!("input: {inp}\n want: {want}\n  got: {got}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} failed:\n{}",
            failures.len(),
            CASES.len(),
            failures.join("\n")
        );
    }

    /// Emulate a libcurl transfer of `url` whose server answers 302 with
    /// `loc` as Location, and whose redirect target answers 200 with a body
    /// echoing the request line and the Host: header. Produces what the
    /// harness prints: "code|effective url|redirect url|body".
    fn emulate_http(follow: bool, url: &[u8], loc: &[u8]) -> String {
        let opts = TransferOptions {
            schemes: REF_SCHEMES,
            ..TransferOptions::default()
        };
        let t = match parse_transfer_url(url, &opts) {
            Ok(t) => t,
            Err(e) => return format!("{}|||", e.code),
        };
        let mut header = b"Location: ".to_vec();
        header.extend_from_slice(loc);
        header.extend_from_slice(b"\r\n");
        let location = copy_header_value(&header).unwrap();
        let mut uh = t.uh.clone();
        let params = FollowParams {
            kind: if follow {
                FollowType::Redir
            } else {
                FollowType::Fake
            },
            httpcode: 302,
            conn_remote_port: t.port.unwrap(),
            conn_protocol: t.handler.protocol,
            ..FollowParams::default()
        };
        let r = match http_follow(&mut uh, &location, &params) {
            Ok(r) => r,
            Err(e) => return format!("{}|{}||", e.code, esc(&t.url)),
        };
        if !follow {
            return format!("0|{}|{}|", esc(&t.url), esc(&r.url));
        }
        let opts2 = TransferOptions {
            this_is_a_follow: true,
            ..opts
        };
        let t2 = match parse_transfer_url(&r.url, &opts2) {
            Ok(t) => t,
            Err(e) => return format!("{}|{}||", e.code, esc(&r.url)),
        };
        let mut body = b"GET ".to_vec();
        body.extend_from_slice(&t2.request_target());
        body.extend_from_slice(b" HTTP/1.1\n");
        body.extend_from_slice(&t2.host_header(t2.port.unwrap()));
        format!("0|{}||{}", esc(&t2.url), esc_all(&body))
    }

    #[test]
    fn http_redirects_against_libcurl_8_14_1() {
        let mut failures = Vec::new();
        for (follow, url, loc, want) in HTTP_CASES {
            let got = emulate_http(*follow, &unesc(url), &unesc(loc));
            if got != *want {
                failures.push(format!(
                    "follow={follow} url: {url} loc: {loc}\n want: {want}\n  got: {got}"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} failed:\n{}",
            failures.len(),
            HTTP_CASES.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn transfer_url_fields() {
        let opts = TransferOptions::default();
        let t = parse_transfer_url(b"localhost:8080/get.inc?test=file#x", &opts).unwrap();
        assert_eq!(t.url, b"http://localhost:8080/get.inc?test=file#x");
        assert_eq!(t.request_target(), b"/get.inc?test=file");
        assert_eq!(t.host_header(8080), b"localhost:8080");
        assert_eq!(t.port, Some(8080));
        assert_eq!(
            t.proxy_request_url(),
            b"http://localhost:8080/get.inc?test=file"
        );

        let t = parse_transfer_url(b"https://u%41:p%01w@[fe80::1%25eth0]/a%20b", &opts).unwrap();
        assert_eq!(t.host_name, b"fe80::1");
        assert!(t.ipv6_ip);
        assert_eq!(t.zoneid.as_deref(), Some(&b"eth0"[..]));
        assert_eq!(t.scope_id, None);
        assert_eq!(t.user.as_deref(), Some(&b"uA"[..]));
        assert_eq!(t.password.as_deref(), Some(&b"p\x01w"[..]));
        assert_eq!(t.host_header(443), b"[fe80::1]");
        assert_eq!(t.host_header(8443), b"[fe80::1]:8443");
        assert_eq!(t.port, Some(443));
        assert_eq!(t.request_target(), b"/a%20b");

        let t = parse_transfer_url(b"http://[::1%2542]:80/", &opts).unwrap();
        assert_eq!(t.scope_id, Some(42));
        assert_eq!(t.host_header(80), b"[::1]");

        // file:// URLs have no host; the port is the handler default (0)
        let t = parse_transfer_url(b"file:///tmp/x", &opts).unwrap();
        assert_eq!(t.hostname, None);
        assert_eq!(t.host_name, b"");
        assert_eq!(t.port, Some(0));
        assert_eq!(t.path, b"/tmp/x");

        // only http, https and file are built in by default
        let e = parse_transfer_url(b"ftp://x/", &opts).unwrap_err();
        assert_eq!(e.code, CURLE_UNSUPPORTED_PROTOCOL);
        assert_eq!(e.message, "Protocol \"ftp\" not supported");
        let follow = TransferOptions {
            this_is_a_follow: true,
            ..TransferOptions::default()
        };
        let e = parse_transfer_url(b"gopher://x/", &follow).unwrap_err();
        assert_eq!(e.message, "Protocol \"gopher\" not supported (in redirect)");
        let restricted = TransferOptions {
            allowed_protocols: CURLPROTO_HTTPS,
            ..TransferOptions::default()
        };
        let e = parse_transfer_url(b"http://x/", &restricted).unwrap_err();
        assert_eq!(e.message, "Protocol \"http\" disabled");
        let e = parse_transfer_url(b"http://x:99999/", &opts).unwrap_err();
        assert_eq!(e.code, CURLE_URL_MALFORMAT);
        assert_eq!(
            e.message,
            "URL rejected: Port number was not a decimal number between 0 and 65535"
        );
        let long = format!("http://{}/", "a".repeat(70000));
        let e = parse_transfer_url(long.as_bytes(), &opts).unwrap_err();
        assert_eq!(e.message, "Too long hostname (maximum is 65535)");
    }

    #[test]
    fn follow_details() {
        let opts = TransferOptions::default();
        let t = parse_transfer_url(b"http://u:p@example.com/a/b", &opts).unwrap();
        let mut uh = t.uh.clone();
        let p = FollowParams {
            conn_remote_port: 80,
            conn_protocol: CURLPROTO_HTTP,
            ..FollowParams::default()
        };
        let r = http_follow(&mut uh, b"c d", &p).unwrap();
        assert_eq!(r.url, b"http://u:p@example.com/a/c%20d");
        assert!(!r.disallow_port);
        assert!(!r.clear_auth);
        let r = http_follow(&mut uh, b"https://example.com/", &p).unwrap();
        assert!(r.disallow_port);
        assert!(r.clear_auth);
        let mut uh = t.uh.clone();
        let r = http_follow(&mut uh, b"http://example.com:81/", &p).unwrap();
        assert!(r.clear_auth);
        let e = http_follow(&mut uh, b"http://", &p).unwrap_err();
        assert_eq!(e.code, CURLE_URL_MALFORMAT);
        assert_eq!(
            e.message,
            "The redirect target URL could not be parsed: No host part in the URL"
        );
        assert_eq!(
            auto_referer(b"http://u:p@example.com/a#frag", SCHEMES).unwrap(),
            b"http://example.com/a"
        );
        assert_eq!(
            copy_header_value(b"Location:   /x y \t\r\n").unwrap(),
            b"/x y"
        );
        assert_eq!(copy_header_value(b"Location:\r\n").unwrap(), b"");
        assert_eq!(copy_header_value(b": x"), None);
        assert_eq!(copy_header_value(b"nocolon"), None);
    }

    #[test]
    fn strerror_and_codes() {
        assert_eq!(url_strerror(UCode::NoHost), "No host part in the URL");
        assert_eq!(
            url_strerror_u32(31),
            "A value or data field is larger than allowed"
        );
        assert_eq!(url_strerror_u32(32), "CURLUcode unknown");
        assert_eq!(uc_to_curlcode(UCode::UnsupportedScheme), 1);
        assert_eq!(uc_to_curlcode(UCode::UserNotAllowed), 67);
        assert_eq!(uc_to_curlcode(UCode::OutOfMemory), 27);
        assert_eq!(uc_to_curlcode(UCode::BadPortNumber), 3);
        assert_eq!(Part::from_u32(11), Err(UCode::UnknownPart));
        assert_eq!(scheme_default_port(SCHEMES, b"FTP"), Some(21));
        assert!(get_scheme_handler(SCHEMES, b"ftp").is_none());
        assert_eq!(get_scheme_handler(SCHEMES, b"HTTPS").unwrap().defport, 443);
        let mut u = Url::new();
        assert_eq!(
            u.set(Part::Url, Some(b"ftp://x/"), 0),
            Err(UCode::UnsupportedScheme)
        );
        let big = vec![b'a'; CURL_MAX_INPUT_LENGTH + 1];
        assert_eq!(u.set(Part::Path, Some(&big), 0), Err(UCode::MalformedInput));
        // embedded NUL ends the C string
        u.set(Part::Url, Some(b"http://h/a\0b"), 0).unwrap();
        assert_eq!(u.get(Part::Url, 0).unwrap(), b"http://h/a");
    }

    // BEGIN HTTP CASES
    const HTTP_CASES: &[(bool, &str, &str, &str)] = &[
        (
            false,
            r###"http://127.0.0.1:28931/L/2f782f793f7a3d31?n0"###,
            r###"/x/y?z=1"###,
            r###"0|http://127.0.0.1:28931/L/2f782f793f7a3d31?n0|http://127.0.0.1:28931/x/y?z=1|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/2f782f793f7a3d31?n1"###,
            r###"/x/y?z=1"###,
            r###"0|http://127.0.0.1:28931/x/y?z=1||GET /x/y?z=1 HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/782079?n2"###,
            r###"x y"###,
            r###"0|http://127.0.0.1:28931/L/782079?n2|http://127.0.0.1:28931/L/x%20y|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/782079?n3"###,
            r###"x y"###,
            r###"0|http://127.0.0.1:28931/L/x%20y||GET /L/x%20y HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/2e2e2f6120623f6320642366726167?n4"###,
            r###"../a b?c d#frag"###,
            r###"0|http://127.0.0.1:28931/L/2e2e2f6120623f6320642366726167?n4|http://127.0.0.1:28931/a%20b?c+d#frag|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/2e2e2f6120623f6320642366726167?n5"###,
            r###"../a b?c d#frag"###,
            r###"0|http://127.0.0.1:28931/a%20b?c+d#frag||GET /a%20b?c+d HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/3f713d31?n6"###,
            r###"?q=1"###,
            r###"0|http://127.0.0.1:28931/L/3f713d31?n6|http://127.0.0.1:28931/L/3f713d31?q=1|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/3f713d31?n7"###,
            r###"?q=1"###,
            r###"0|http://127.0.0.1:28931/L/3f713d31?q=1||GET /L/3f713d31?q=1 HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/2f2f3132372e302e302e313a32383933312f616273?n8"###,
            r###"//127.0.0.1:28931/abs"###,
            r###"0|http://127.0.0.1:28931/L/2f2f3132372e302e302e313a32383933312f616273?n8|http://127.0.0.1:28931/abs|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/2f2f3132372e302e302e313a32383933312f616273?n9"###,
            r###"//127.0.0.1:28931/abs"###,
            r###"0|http://127.0.0.1:28931/abs||GET /abs HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a32383933312fc3a4623fc3a42062?n10"###,
            r###"http://localhost:28931/\xc3\xa4b?\xc3\xa4 b"###,
            r###"0|http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a32383933312fc3a4623fc3a42062?n10|http://localhost:28931/\xc3\xa4b?\xc3\xa4 b|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a32383933312fc3a4623fc3a42062?n11"###,
            r###"http://localhost:28931/\xc3\xa4b?\xc3\xa4 b"###,
            r###"0|http://localhost:28931/%c3%a4b?%c3%a4+b||GET /%c3%a4b?%c3%a4+b HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/666f6f3a2f2f626172?n12"###,
            r###"foo://bar"###,
            r###"0|http://127.0.0.1:28931/L/666f6f3a2f2f626172?n12|foo://bar/|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/666f6f3a2f2f626172?n13"###,
            r###"foo://bar"###,
            r###"1|http://127.0.0.1:28931/L/666f6f3a2f2f626172?n13||"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/687474703a2f2f?n14"###,
            r###"http://"###,
            r###"0|http://127.0.0.1:28931/L/687474703a2f2f?n14|http://|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/687474703a2f2f?n15"###,
            r###"http://"###,
            r###"3|http://127.0.0.1:28931/L/687474703a2f2f?n15||"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/2366726167?n16"###,
            r###"#frag"###,
            r###"0|http://127.0.0.1:28931/L/2366726167?n16|http://127.0.0.1:28931/L/2366726167?n16#frag|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/2366726167?n17"###,
            r###"#frag"###,
            r###"0|http://127.0.0.1:28931/L/2366726167?n17#frag||GET /L/2366726167?n17 HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/20202f737061636564202009?n18"###,
            r###"  /spaced  \x09"###,
            r###"0|http://127.0.0.1:28931/L/20202f737061636564202009?n18|http://127.0.0.1:28931/spaced|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/20202f737061636564202009?n19"###,
            r###"  /spaced  \x09"###,
            r###"0|http://127.0.0.1:28931/spaced||GET /spaced HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/2f700171?n20"###,
            r###"/p\x01q"###,
            r###"0|http://127.0.0.1:28931/L/2f700171?n20|http://127.0.0.1:28931/p%01q|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/2f700171?n21"###,
            r###"/p\x01q"###,
            r###"0|http://127.0.0.1:28931/p%01q||GET /p%01q HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/485454503a2f2f4c4f43414c484f53543a32383933312f5570?n22"###,
            r###"HTTP://LOCALHOST:28931/Up"###,
            r###"0|http://127.0.0.1:28931/L/485454503a2f2f4c4f43414c484f53543a32383933312f5570?n22|http://LOCALHOST:28931/Up|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/485454503a2f2f4c4f43414c484f53543a32383933312f5570?n23"###,
            r###"HTTP://LOCALHOST:28931/Up"###,
            r###"0|http://LOCALHOST:28931/Up||GET /Up HTTP/1.1\x0aLOCALHOST:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/687474703a2f2f3132372e313a32383933312f2e2e2f646f74732f2e2f78?n24"###,
            r###"http://127.1:28931/../dots/./x"###,
            r###"0|http://127.0.0.1:28931/L/687474703a2f2f3132372e313a32383933312f2e2e2f646f74732f2e2f78?n24|http://127.0.0.1:28931/dots/x|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/687474703a2f2f3132372e313a32383933312f2e2e2f646f74732f2e2f78?n25"###,
            r###"http://127.1:28931/../dots/./x"###,
            r###"0|http://127.0.0.1:28931/dots/x||GET /dots/x HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/2f612f2532652532652f62?n26"###,
            r###"/a/%2e%2e/b"###,
            r###"0|http://127.0.0.1:28931/L/2f612f2532652532652f62?n26|http://127.0.0.1:28931/b|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/2f612f2532652532652f62?n27"###,
            r###"/a/%2e%2e/b"###,
            r###"0|http://127.0.0.1:28931/b||GET /b HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/687474703a2f2f753a70406c6f63616c686f73743a32383933312f6372656473?n28"###,
            r###"http://u:p@localhost:28931/creds"###,
            r###"0|http://127.0.0.1:28931/L/687474703a2f2f753a70406c6f63616c686f73743a32383933312f6372656473?n28|http://u:p@localhost:28931/creds|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/687474703a2f2f753a70406c6f63616c686f73743a32383933312f6372656473?n29"###,
            r###"http://u:p@localhost:28931/creds"###,
            r###"0|http://u:p@localhost:28931/creds||GET /creds HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n30"###,
            r###"http://localhost:0x479d/hexport"###,
            r###"0|http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n30|http://localhost:0x479d/hexport|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n31"###,
            r###"http://localhost:0x479d/hexport"###,
            r###"3|http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n31||"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/687474703a2f2f323133303730363433333a32383933312f6e756d?n32"###,
            r###"http://2130706433:28931/num"###,
            r###"0|http://127.0.0.1:28931/L/687474703a2f2f323133303730363433333a32383933312f6e756d?n32|http://127.0.0.1:28931/num|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/687474703a2f2f323133303730363433333a32383933312f6e756d?n33"###,
            r###"http://2130706433:28931/num"###,
            r###"0|http://127.0.0.1:28931/num||GET /num HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/2f2f6c6f63616c686f73743a3238393331?n34"###,
            r###"//localhost:28931"###,
            r###"0|http://127.0.0.1:28931/L/2f2f6c6f63616c686f73743a3238393331?n34|http://localhost:28931/|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/2f2f6c6f63616c686f73743a3238393331?n35"###,
            r###"//localhost:28931"###,
            r###"0|http://localhost:28931/||GET / HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a32383933313f6f6e6c79?n36"###,
            r###"http://localhost:28931?only"###,
            r###"0|http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a32383933313f6f6e6c79?n36|http://localhost:28931/?only|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/687474703a2f2f6c6f63616c686f73743a32383933313f6f6e6c79?n37"###,
            r###"http://localhost:28931?only"###,
            r###"0|http://localhost:28931/?only||GET /?only HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"http://127.0.0.1:28931/L/5c6261636b5c736c617368?n38"###,
            r###"\\back\\slash"###,
            r###"0|http://127.0.0.1:28931/L/5c6261636b5c736c617368?n38|http://127.0.0.1:28931/L/\\back\\slash|"###,
        ),
        (
            true,
            r###"http://127.0.0.1:28931/L/5c6261636b5c736c617368?n39"###,
            r###"\\back\\slash"###,
            r###"0|http://127.0.0.1:28931/L/\\back\\slash||GET /L/\\back\\slash HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/2f782f793f7a3d31?n40"###,
            r###"/x/y?z=1"###,
            r###"0|http://localhost:28931/d/L/2f782f793f7a3d31?n40|http://localhost:28931/x/y?z=1|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/2f782f793f7a3d31?n41"###,
            r###"/x/y?z=1"###,
            r###"0|http://localhost:28931/x/y?z=1||GET /x/y?z=1 HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/782079?n42"###,
            r###"x y"###,
            r###"0|http://localhost:28931/d/L/782079?n42|http://localhost:28931/d/L/x%20y|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/782079?n43"###,
            r###"x y"###,
            r###"0|http://localhost:28931/d/L/x%20y||GET /d/L/x%20y HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/2e2e2f6120623f6320642366726167?n44"###,
            r###"../a b?c d#frag"###,
            r###"0|http://localhost:28931/d/L/2e2e2f6120623f6320642366726167?n44|http://localhost:28931/d/a%20b?c+d#frag|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/2e2e2f6120623f6320642366726167?n45"###,
            r###"../a b?c d#frag"###,
            r###"0|http://localhost:28931/d/a%20b?c+d#frag||GET /d/a%20b?c+d HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/3f713d31?n46"###,
            r###"?q=1"###,
            r###"0|http://localhost:28931/d/L/3f713d31?n46|http://localhost:28931/d/L/3f713d31?q=1|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/3f713d31?n47"###,
            r###"?q=1"###,
            r###"0|http://localhost:28931/d/L/3f713d31?q=1||GET /d/L/3f713d31?q=1 HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/2f2f3132372e302e302e313a32383933312f616273?n48"###,
            r###"//127.0.0.1:28931/abs"###,
            r###"0|http://localhost:28931/d/L/2f2f3132372e302e302e313a32383933312f616273?n48|http://127.0.0.1:28931/abs|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/2f2f3132372e302e302e313a32383933312f616273?n49"###,
            r###"//127.0.0.1:28931/abs"###,
            r###"0|http://127.0.0.1:28931/abs||GET /abs HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a32383933312fc3a4623fc3a42062?n50"###,
            r###"http://localhost:28931/\xc3\xa4b?\xc3\xa4 b"###,
            r###"0|http://localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a32383933312fc3a4623fc3a42062?n50|http://localhost:28931/\xc3\xa4b?\xc3\xa4 b|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a32383933312fc3a4623fc3a42062?n51"###,
            r###"http://localhost:28931/\xc3\xa4b?\xc3\xa4 b"###,
            r###"0|http://localhost:28931/%c3%a4b?%c3%a4+b||GET /%c3%a4b?%c3%a4+b HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/666f6f3a2f2f626172?n52"###,
            r###"foo://bar"###,
            r###"0|http://localhost:28931/d/L/666f6f3a2f2f626172?n52|foo://bar/|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/666f6f3a2f2f626172?n53"###,
            r###"foo://bar"###,
            r###"1|http://localhost:28931/d/L/666f6f3a2f2f626172?n53||"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/687474703a2f2f?n54"###,
            r###"http://"###,
            r###"0|http://localhost:28931/d/L/687474703a2f2f?n54|http://|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/687474703a2f2f?n55"###,
            r###"http://"###,
            r###"3|http://localhost:28931/d/L/687474703a2f2f?n55||"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/2366726167?n56"###,
            r###"#frag"###,
            r###"0|http://localhost:28931/d/L/2366726167?n56|http://localhost:28931/d/L/2366726167?n56#frag|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/2366726167?n57"###,
            r###"#frag"###,
            r###"0|http://localhost:28931/d/L/2366726167?n57#frag||GET /d/L/2366726167?n57 HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/20202f737061636564202009?n58"###,
            r###"  /spaced  \x09"###,
            r###"0|http://localhost:28931/d/L/20202f737061636564202009?n58|http://localhost:28931/spaced|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/20202f737061636564202009?n59"###,
            r###"  /spaced  \x09"###,
            r###"0|http://localhost:28931/spaced||GET /spaced HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/2f700171?n60"###,
            r###"/p\x01q"###,
            r###"0|http://localhost:28931/d/L/2f700171?n60|http://localhost:28931/p%01q|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/2f700171?n61"###,
            r###"/p\x01q"###,
            r###"0|http://localhost:28931/p%01q||GET /p%01q HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/485454503a2f2f4c4f43414c484f53543a32383933312f5570?n62"###,
            r###"HTTP://LOCALHOST:28931/Up"###,
            r###"0|http://localhost:28931/d/L/485454503a2f2f4c4f43414c484f53543a32383933312f5570?n62|http://LOCALHOST:28931/Up|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/485454503a2f2f4c4f43414c484f53543a32383933312f5570?n63"###,
            r###"HTTP://LOCALHOST:28931/Up"###,
            r###"0|http://LOCALHOST:28931/Up||GET /Up HTTP/1.1\x0aLOCALHOST:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/687474703a2f2f3132372e313a32383933312f2e2e2f646f74732f2e2f78?n64"###,
            r###"http://127.1:28931/../dots/./x"###,
            r###"0|http://localhost:28931/d/L/687474703a2f2f3132372e313a32383933312f2e2e2f646f74732f2e2f78?n64|http://127.0.0.1:28931/dots/x|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/687474703a2f2f3132372e313a32383933312f2e2e2f646f74732f2e2f78?n65"###,
            r###"http://127.1:28931/../dots/./x"###,
            r###"0|http://127.0.0.1:28931/dots/x||GET /dots/x HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/2f612f2532652532652f62?n66"###,
            r###"/a/%2e%2e/b"###,
            r###"0|http://localhost:28931/d/L/2f612f2532652532652f62?n66|http://localhost:28931/b|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/2f612f2532652532652f62?n67"###,
            r###"/a/%2e%2e/b"###,
            r###"0|http://localhost:28931/b||GET /b HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/687474703a2f2f753a70406c6f63616c686f73743a32383933312f6372656473?n68"###,
            r###"http://u:p@localhost:28931/creds"###,
            r###"0|http://localhost:28931/d/L/687474703a2f2f753a70406c6f63616c686f73743a32383933312f6372656473?n68|http://u:p@localhost:28931/creds|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/687474703a2f2f753a70406c6f63616c686f73743a32383933312f6372656473?n69"###,
            r###"http://u:p@localhost:28931/creds"###,
            r###"0|http://u:p@localhost:28931/creds||GET /creds HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n70"###,
            r###"http://localhost:0x479d/hexport"###,
            r###"0|http://localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n70|http://localhost:0x479d/hexport|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n71"###,
            r###"http://localhost:0x479d/hexport"###,
            r###"3|http://localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a3078343739642f686578706f7274?n71||"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/687474703a2f2f323133303730363433333a32383933312f6e756d?n72"###,
            r###"http://2130706433:28931/num"###,
            r###"0|http://localhost:28931/d/L/687474703a2f2f323133303730363433333a32383933312f6e756d?n72|http://127.0.0.1:28931/num|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/687474703a2f2f323133303730363433333a32383933312f6e756d?n73"###,
            r###"http://2130706433:28931/num"###,
            r###"0|http://127.0.0.1:28931/num||GET /num HTTP/1.1\x0a127.0.0.1:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/2f2f6c6f63616c686f73743a3238393331?n74"###,
            r###"//localhost:28931"###,
            r###"0|http://localhost:28931/d/L/2f2f6c6f63616c686f73743a3238393331?n74|http://localhost:28931/|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/2f2f6c6f63616c686f73743a3238393331?n75"###,
            r###"//localhost:28931"###,
            r###"0|http://localhost:28931/||GET / HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a32383933313f6f6e6c79?n76"###,
            r###"http://localhost:28931?only"###,
            r###"0|http://localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a32383933313f6f6e6c79?n76|http://localhost:28931/?only|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/687474703a2f2f6c6f63616c686f73743a32383933313f6f6e6c79?n77"###,
            r###"http://localhost:28931?only"###,
            r###"0|http://localhost:28931/?only||GET /?only HTTP/1.1\x0alocalhost:28931"###,
        ),
        (
            false,
            r###"localhost:28931/d/L/5c6261636b5c736c617368?n78"###,
            r###"\\back\\slash"###,
            r###"0|http://localhost:28931/d/L/5c6261636b5c736c617368?n78|http://localhost:28931/d/L/\\back\\slash|"###,
        ),
        (
            true,
            r###"localhost:28931/d/L/5c6261636b5c736c617368?n79"###,
            r###"\\back\\slash"###,
            r###"0|http://localhost:28931/d/L/\\back\\slash||GET /d/L/\\back\\slash HTTP/1.1\x0alocalhost:28931"###,
        ),
    ];
    // END HTTP CASES

    // BEGIN GENERATED CASES
    const CASES: &[(&str, &str)] = &[
        (
            r###"S0:0:http://example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com/	=http	E11	E12	E13	=example.com	E15	=/	E16	E17	E18	=http://example.com:80/	=http://example.com/	=http://example.com/	=80	=http://example.com/	=/"###,
        ),
        (
            r###"S0:0:http://example.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com/	=http	E11	E12	E13	=example.com	E15	=/	E16	E17	E18	=http://example.com:80/	=http://example.com/	=http://example.com/	=80	=http://example.com/	=/"###,
        ),
        (
            r###"S0:0:https://example.com:443/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=https://example.com:443/	=https	E11	E12	E13	=example.com	=443	=/	E16	E17	E18	=https://example.com:443/	=https://example.com/	=https://example.com:443/	=443	=https://example.com:443/	=/"###,
        ),
        (
            r###"S0:0:http://example.com:80/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com:80/x	=http	E11	E12	E13	=example.com	=80	=/x	E16	E17	E18	=http://example.com:80/x	=http://example.com/x	=http://example.com:80/x	=80	=http://example.com:80/x	=/x"###,
        ),
        (
            r###"S0:0:http://example.com:0080/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com:80/x	=http	E11	E12	E13	=example.com	=80	=/x	E16	E17	E18	=http://example.com:80/x	=http://example.com/x	=http://example.com:80/x	=80	=http://example.com:80/x	=/x"###,
        ),
        (
            r###"S0:0:http://example.com:/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com/x	=http	E11	E12	E13	=example.com	E15	=/x	E16	E17	E18	=http://example.com:80/x	=http://example.com/x	=http://example.com/x	=80	=http://example.com/x	=/x"###,
        ),
        (
            r###"S0:0:http://example.com:65535/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com:65535/	=http	E11	E12	E13	=example.com	=65535	=/	E16	E17	E18	=http://example.com:65535/	=http://example.com:65535/	=http://example.com:65535/	=65535	=http://example.com:65535/	=/"###,
        ),
        (
            r###"S0:0:http://a:99999/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://a:65536/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://a:-1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://a:+1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://a:1x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://a:1:2/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E14	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http:///	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E14	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://x/../../a/./b/../c	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/a/c	=http	E11	E12	E13	=x	E15	=/a/c	E16	E17	E18	=http://x:80/a/c	=http://x/a/c	=http://x/a/c	=80	=http://x/a/c	=/a/c"###,
        ),
        (
            r###"S0:16:http://x/../../a/./b/../c	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/../../a/./b/../c	=http	E11	E12	E13	=x	E15	=/../../a/./b/../c	E16	E17	E18	=http://x:80/../../a/./b/../c	=http://x/../../a/./b/../c	=http://x/../../a/./b/../c	=80	=http://x/../../a/./b/../c	=/../../a/./b/../c"###,
        ),
        (
            r###"S0:0:http://x/a/b/..	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/a/	=http	E11	E12	E13	=x	E15	=/a/	E16	E17	E18	=http://x:80/a/	=http://x/a/	=http://x/a/	=80	=http://x/a/	=/a/"###,
        ),
        (
            r###"S0:0:http://x/a/b/.	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/a/b/	=http	E11	E12	E13	=x	E15	=/a/b/	E16	E17	E18	=http://x:80/a/b/	=http://x/a/b/	=http://x/a/b/	=80	=http://x/a/b/	=/a/b/"###,
        ),
        (
            r###"S0:0:http://x/a/%2e%2E/b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/b	=http	E11	E12	E13	=x	E15	=/b	E16	E17	E18	=http://x:80/b	=http://x/b	=http://x/b	=80	=http://x/b	=/b"###,
        ),
        (
            r###"S0:0:http://x/a/%2e%2Ex/b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/a/%2e%2Ex/b	=http	E11	E12	E13	=x	E15	=/a/%2e%2Ex/b	E16	E17	E18	=http://x:80/a/%2e%2Ex/b	=http://x/a/%2e%2Ex/b	=http://x/a/%2e%2Ex/b	=80	=http://x/a/%2e%2Ex/b	=/a/%2e%2Ex/b"###,
        ),
        (
            r###"S0:0:http://x/.hidden/..file	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/.hidden/..file	=http	E11	E12	E13	=x	E15	=/.hidden/..file	E16	E17	E18	=http://x:80/.hidden/..file	=http://x/.hidden/..file	=http://x/.hidden/..file	=80	=http://x/.hidden/..file	=/.hidden/..file"###,
        ),
        (
            r###"S0:0:http://x/a/b/../../../..	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/	=http	E11	E12	E13	=x	E15	=/	E16	E17	E18	=http://x:80/	=http://x/	=http://x/	=80	=http://x/	=/"###,
        ),
        (
            r###"S0:0:http://x//a//b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x//a//b	=http	E11	E12	E13	=x	E15	=//a//b	E16	E17	E18	=http://x:80//a//b	=http://x//a//b	=http://x//a//b	=80	=http://x//a//b	=//a//b"###,
        ),
        (
            r###"S0:0:HTTP://Example.COM/Path	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://Example.COM/Path	=http	E11	E12	E13	=Example.COM	E15	=/Path	E16	E17	E18	=http://Example.COM:80/Path	=http://Example.COM/Path	=http://Example.COM/Path	=80	=http://Example.COM/Path	=/Path"###,
        ),
        (
            r###"S0:0:http://user:pass@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://user:pass@host/	=http	=user	=pass	E13	=host	E15	=/	E16	E17	E18	=http://user:pass@host:80/	=http://user:pass@host/	=http://user:pass@host/	=80	=http://user:pass@host/	=/"###,
        ),
        (
            r###"S0:0:http://user@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://user@host/	=http	=user	E12	E13	=host	E15	=/	E16	E17	E18	=http://user@host:80/	=http://user@host/	=http://user@host/	=80	=http://user@host/	=/"###,
        ),
        (
            r###"S0:0:http://:pass@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://:pass@host/	=http	=	=pass	E13	=host	E15	=/	E16	E17	E18	=http://:pass@host:80/	=http://:pass@host/	=http://:pass@host/	=80	=http://:pass@host/	=/"###,
        ),
        (
            r###"S0:0:http://@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://@host/	=http	=	E12	E13	=host	E15	=/	E16	E17	E18	=http://@host:80/	=http://@host/	=http://@host/	=80	=http://@host/	=/"###,
        ),
        (
            r###"S0:0:http://user:@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://user:@host/	=http	=user	=	E13	=host	E15	=/	E16	E17	E18	=http://user:@host:80/	=http://user:@host/	=http://user:@host/	=80	=http://user:@host/	=/"###,
        ),
        (
            r###"S0:0:http://us%40er:p%3Ass@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://us%40er:p%3Ass@host/	=http	=us%40er	=p%3Ass	E13	=host	E15	=/	E16	E17	E18	=http://us%40er:p%3Ass@host:80/	=http://us%40er:p%3Ass@host/	=http://us%40er:p%3Ass@host/	=80	=http://us%40er:p%3Ass@host/	=/"###,
        ),
        (
            r###"S0:0:http://a@b@c/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E21	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:32:http://user:pass@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E8	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:32:http://@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E8	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:imap://user;AUTH=x:pw@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=imap://user:pw;AUTH=x@host/	=imap	=user	=pw	=AUTH=x	=host	E15	=/	E16	E17	E18	=imap://user:pw;AUTH=x@host:143/	=imap://user:pw;AUTH=x@host/	=imap://user:pw;AUTH=x@host/	=143	=imap://user:pw;AUTH=x@host/	=/"###,
        ),
        (
            r###"S0:0:imap://user:pw;AUTH=x@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=imap://user:pw;AUTH=x@host/	=imap	=user	=pw	=AUTH=x	=host	E15	=/	E16	E17	E18	=imap://user:pw;AUTH=x@host:143/	=imap://user:pw;AUTH=x@host/	=imap://user:pw;AUTH=x@host/	=143	=imap://user:pw;AUTH=x@host/	=/"###,
        ),
        (
            r###"S0:0:imap://;AUTH=x@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=imap://;AUTH=x@host/	=imap	=	E12	=AUTH=x	=host	E15	=/	E16	E17	E18	=imap://;AUTH=x@host:143/	=imap://;AUTH=x@host/	=imap://;AUTH=x@host/	=143	=imap://;AUTH=x@host/	=/"###,
        ),
        (
            r###"S0:0:http://user;AUTH=x:pw@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://user;AUTH=x:pw@host/	=http	=user;AUTH=x	=pw	E13	=host	E15	=/	E16	E17	E18	=http://user;AUTH=x:pw@host:80/	=http://user;AUTH=x:pw@host/	=http://user;AUTH=x:pw@host/	=80	=http://user;AUTH=x:pw@host/	=/"###,
        ),
        (
            r###"S0:0:http://host/p?q=1#frag	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/p?q=1#frag	=http	E11	E12	E13	=host	E15	=/p	=q=1	=frag	E18	=http://host:80/p?q=1#frag	=http://host/p?q=1#frag	=http://host/p?q=1#frag	=80	=http://host/p?q=1#frag	=/p"###,
        ),
        (
            r###"S0:0:http://host/p?#	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/p	=http	E11	E12	E13	=host	E15	=/p	E16	E17	E18	=http://host:80/p	=http://host/p	=http://host/p	=80	=http://host/p?#	=/p"###,
        ),
        (
            r###"S0:0:http://host/p?	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/p	=http	E11	E12	E13	=host	E15	=/p	E16	E17	E18	=http://host:80/p	=http://host/p	=http://host/p	=80	=http://host/p?	=/p"###,
        ),
        (
            r###"S0:0:http://host/p#	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/p	=http	E11	E12	E13	=host	E15	=/p	E16	E17	E18	=http://host:80/p	=http://host/p	=http://host/p	=80	=http://host/p#	=/p"###,
        ),
        (
            r###"S0:0:http://host?q	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/?q	=http	E11	E12	E13	=host	E15	=/	=q	E17	E18	=http://host:80/?q	=http://host/?q	=http://host/?q	=80	=http://host/?q	=/"###,
        ),
        (
            r###"S0:0:http://host#f	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/#f	=http	E11	E12	E13	=host	E15	=/	E16	=f	E18	=http://host:80/#f	=http://host/#f	=http://host/#f	=80	=http://host/#f	=/"###,
        ),
        (
            r###"S0:0:http://host/p?a=b c	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://host/a b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:2048:http://host/a b?c d#e f	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/a b?c d#e f	=http	E11	E12	E13	=host	E15	=/a b	=c d	=e f	E18	=http://host:80/a b?c d#e f	=http://host/a b?c d#e f	=http://host/a b?c d#e f	=80	=http://host/a b?c d#e f	=/a%20b"###,
        ),
        (
            r###"S0:2176:http://host/a b?c d#e f	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/a%20b?c+d#e%20f	=http	E11	E12	E13	=host	E15	=/a%20b	=c+d	=e%20f	E18	=http://host:80/a%20b?c+d#e%20f	=http://host/a%20b?c+d#e%20f	=http://host/a%20b?c+d#e%20f	=80	=http://host/a%20b?c+d#e%20f	=/a%20b"###,
        ),
        (
            r###"S0:128:http://host/\xc3\xa4?\xc3\xa4=\xc3\xa4#\xc3\xa4	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/%c3%a4?%c3%a4=%c3%a4#%c3%a4	=http	E11	E12	E13	=host	E15	=/%c3%a4	=%c3%a4=%c3%a4	=%c3%a4	E18	=http://host:80/%c3%a4?%c3%a4=%c3%a4#%c3%a4	=http://host/%c3%a4?%c3%a4=%c3%a4#%c3%a4	=http://host/%c3%a4?%c3%a4=%c3%a4#%c3%a4	=80	=http://host/%c3%a4?%c3%a4=%c3%a4#%c3%a4	=/%c3%a4"###,
        ),
        (
            r###"S0:0:http://host/\xc3\xa4	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/\xc3\xa4	=http	E11	E12	E13	=host	E15	=/\xc3\xa4	E16	E17	E18	=http://host:80/\xc3\xa4	=http://host/\xc3\xa4	=http://host/\xc3\xa4	=80	=http://host/\xc3\xa4	=/%c3%a4"###,
        ),
        (
            r###"S0:0:http://host/\x01	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://host/\x7f	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://ho st/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:2048:http://ho st/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E21	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://h\xc3\xa4st/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h\xc3\xa4st/	=http	E11	E12	E13	=h\xc3\xa4st	E15	=/	E16	E17	E18	=http://h\xc3\xa4st:80/	=http://h\xc3\xa4st/	=http://h%C3%A4st/	=80	=http://h\xc3\xa4st/	=/"###,
        ),
        (
            r###"S0:0:http://ex%61mple.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com/	=http	E11	E12	E13	=example.com	E15	=/	E16	E17	E18	=http://example.com:80/	=http://example.com/	=http://example.com/	=80	=http://example.com/	=/"###,
        ),
        (
            r###"S0:0:http://ex%00mple.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E21	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://ex%2fmple.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E21	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://exa_mple.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://exa_mple.com/	=http	E11	E12	E13	=exa_mple.com	E15	=/	E16	E17	E18	=http://exa_mple.com:80/	=http://exa_mple.com/	=http://exa_mple.com/	=80	=http://exa_mple.com/	=/"###,
        ),
        (
            r###"S0:0:http://exa!mple.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E21	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://127.1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://127.0.0.1/	=http	E11	E12	E13	=127.0.0.1	E15	=/	E16	E17	E18	=http://127.0.0.1:80/	=http://127.0.0.1/	=http://127.0.0.1/	=80	=http://127.0.0.1/	=/"###,
        ),
        (
            r###"S0:0:http://127.0.0.1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://127.0.0.1/	=http	E11	E12	E13	=127.0.0.1	E15	=/	E16	E17	E18	=http://127.0.0.1:80/	=http://127.0.0.1/	=http://127.0.0.1/	=80	=http://127.0.0.1/	=/"###,
        ),
        (
            r###"S0:0:http://0x7f.1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://127.0.0.1/	=http	E11	E12	E13	=127.0.0.1	E15	=/	E16	E17	E18	=http://127.0.0.1:80/	=http://127.0.0.1/	=http://127.0.0.1/	=80	=http://127.0.0.1/	=/"###,
        ),
        (
            r###"S0:0:http://0177.0.0.1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://127.0.0.1/	=http	E11	E12	E13	=127.0.0.1	E15	=/	E16	E17	E18	=http://127.0.0.1:80/	=http://127.0.0.1/	=http://127.0.0.1/	=80	=http://127.0.0.1/	=/"###,
        ),
        (
            r###"S0:0:http://2130706433/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://127.0.0.1/	=http	E11	E12	E13	=127.0.0.1	E15	=/	E16	E17	E18	=http://127.0.0.1:80/	=http://127.0.0.1/	=http://127.0.0.1/	=80	=http://127.0.0.1/	=/"###,
        ),
        (
            r###"S0:0:http://4294967295/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://255.255.255.255/	=http	E11	E12	E13	=255.255.255.255	E15	=/	E16	E17	E18	=http://255.255.255.255:80/	=http://255.255.255.255/	=http://255.255.255.255/	=80	=http://255.255.255.255/	=/"###,
        ),
        (
            r###"S0:0:http://4294967296/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://4294967296/	=http	E11	E12	E13	=4294967296	E15	=/	E16	E17	E18	=http://4294967296:80/	=http://4294967296/	=http://4294967296/	=80	=http://4294967296/	=/"###,
        ),
        (
            r###"S0:0:http://1.2.3.256/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.2.3.256/	=http	E11	E12	E13	=1.2.3.256	E15	=/	E16	E17	E18	=http://1.2.3.256:80/	=http://1.2.3.256/	=http://1.2.3.256/	=80	=http://1.2.3.256/	=/"###,
        ),
        (
            r###"S0:0:http://1.2.65536/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.2.65536/	=http	E11	E12	E13	=1.2.65536	E15	=/	E16	E17	E18	=http://1.2.65536:80/	=http://1.2.65536/	=http://1.2.65536/	=80	=http://1.2.65536/	=/"###,
        ),
        (
            r###"S0:0:http://1.2.65535/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.2.255.255/	=http	E11	E12	E13	=1.2.255.255	E15	=/	E16	E17	E18	=http://1.2.255.255:80/	=http://1.2.255.255/	=http://1.2.255.255/	=80	=http://1.2.255.255/	=/"###,
        ),
        (
            r###"S0:0:http://1.16777215/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.255.255.255/	=http	E11	E12	E13	=1.255.255.255	E15	=/	E16	E17	E18	=http://1.255.255.255:80/	=http://1.255.255.255/	=http://1.255.255.255/	=80	=http://1.255.255.255/	=/"###,
        ),
        (
            r###"S0:0:http://1.16777216/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.16777216/	=http	E11	E12	E13	=1.16777216	E15	=/	E16	E17	E18	=http://1.16777216:80/	=http://1.16777216/	=http://1.16777216/	=80	=http://1.16777216/	=/"###,
        ),
        (
            r###"S0:0:http://256.1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://256.1/	=http	E11	E12	E13	=256.1	E15	=/	E16	E17	E18	=http://256.1:80/	=http://256.1/	=http://256.1/	=80	=http://256.1/	=/"###,
        ),
        (
            r###"S0:0:http://1.2.3.4.5/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.2.3.4.5/	=http	E11	E12	E13	=1.2.3.4.5	E15	=/	E16	E17	E18	=http://1.2.3.4.5:80/	=http://1.2.3.4.5/	=http://1.2.3.4.5/	=80	=http://1.2.3.4.5/	=/"###,
        ),
        (
            r###"S0:0:http://1.2.3./	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.2.3./	=http	E11	E12	E13	=1.2.3.	E15	=/	E16	E17	E18	=http://1.2.3.:80/	=http://1.2.3./	=http://1.2.3./	=80	=http://1.2.3./	=/"###,
        ),
        (
            r###"S0:0:http://08/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://08/	=http	E11	E12	E13	=08	E15	=/	E16	E17	E18	=http://08:80/	=http://08/	=http://08/	=80	=http://08/	=/"###,
        ),
        (
            r###"S0:0:http://0x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://0x/	=http	E11	E12	E13	=0x	E15	=/	E16	E17	E18	=http://0x:80/	=http://0x/	=http://0x/	=80	=http://0x/	=/"###,
        ),
        (
            r###"S0:0:http://0X10/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://0X10/	=http	E11	E12	E13	=0X10	E15	=/	E16	E17	E18	=http://0X10:80/	=http://0X10/	=http://0X10/	=80	=http://0X10/	=/"###,
        ),
        (
            r###"S0:0:http://0xffffffff/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://255.255.255.255/	=http	E11	E12	E13	=255.255.255.255	E15	=/	E16	E17	E18	=http://255.255.255.255:80/	=http://255.255.255.255/	=http://255.255.255.255/	=80	=http://255.255.255.255/	=/"###,
        ),
        (
            r###"S0:0:http://0x100000000/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://0x100000000/	=http	E11	E12	E13	=0x100000000	E15	=/	E16	E17	E18	=http://0x100000000:80/	=http://0x100000000/	=http://0x100000000/	=80	=http://0x100000000/	=/"###,
        ),
        (
            r###"S0:0:http://[::1]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1]/	=http	E11	E12	E13	=[::1]	E15	=/	E16	E17	E18	=http://[::1]:80/	=http://[::1]/	=http://[::1]/	=80	=http://[::1]/	=/"###,
        ),
        (
            r###"S0:0:http://[::1]:8080/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1]:8080/	=http	E11	E12	E13	=[::1]	=8080	=/	E16	E17	E18	=http://[::1]:8080/	=http://[::1]:8080/	=http://[::1]:8080/	=8080	=http://[::1]:8080/	=/"###,
        ),
        (
            r###"S0:0:http://[::1]x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::]/	=http	E11	E12	E13	=[::]	E15	=/	E16	E17	E18	=http://[::]:80/	=http://[::]/	=http://[::]/	=80	=http://[::]/	=/"###,
        ),
        (
            r###"S0:0:http://[0:0:0:0:0:0:0:1]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1]/	=http	E11	E12	E13	=[::1]	E15	=/	E16	E17	E18	=http://[::1]:80/	=http://[::1]/	=http://[::1]/	=80	=http://[::1]/	=/"###,
        ),
        (
            r###"S0:0:http://[ABCD::1]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[ABCD::1]/	=http	E11	E12	E13	=[ABCD::1]	E15	=/	E16	E17	E18	=http://[ABCD::1]:80/	=http://[ABCD::1]/	=http://[ABCD::1]/	=80	=http://[ABCD::1]/	=/"###,
        ),
        (
            r###"S0:0:http://[abcd:0:0:0:0:0:0:1]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[abcd::1]/	=http	E11	E12	E13	=[abcd::1]	E15	=/	E16	E17	E18	=http://[abcd::1]:80/	=http://[abcd::1]/	=http://[abcd::1]/	=80	=http://[abcd::1]/	=/"###,
        ),
        (
            r###"S0:0:http://[::ffff:1.2.3.4]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::ffff:1.2.3.4]/	=http	E11	E12	E13	=[::ffff:1.2.3.4]	E15	=/	E16	E17	E18	=http://[::ffff:1.2.3.4]:80/	=http://[::ffff:1.2.3.4]/	=http://[::ffff:1.2.3.4]/	=80	=http://[::ffff:1.2.3.4]/	=/"###,
        ),
        (
            r###"S0:0:http://[::ffff:102:304]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::ffff:102:304]/	=http	E11	E12	E13	=[::ffff:102:304]	E15	=/	E16	E17	E18	=http://[::ffff:102:304]:80/	=http://[::ffff:102:304]/	=http://[::ffff:102:304]/	=80	=http://[::ffff:102:304]/	=/"###,
        ),
        (
            r###"S0:0:http://[0:0:0:0:0:ffff:1.2.3.4]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::ffff:1.2.3.4]/	=http	E11	E12	E13	=[::ffff:1.2.3.4]	E15	=/	E16	E17	E18	=http://[::ffff:1.2.3.4]:80/	=http://[::ffff:1.2.3.4]/	=http://[::ffff:1.2.3.4]/	=80	=http://[::ffff:1.2.3.4]/	=/"###,
        ),
        (
            r###"S0:0:http://[::1.2.3.4]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1.2.3.4]/	=http	E11	E12	E13	=[::1.2.3.4]	E15	=/	E16	E17	E18	=http://[::1.2.3.4]:80/	=http://[::1.2.3.4]/	=http://[::1.2.3.4]/	=80	=http://[::1.2.3.4]/	=/"###,
        ),
        (
            r###"S0:0:http://[1:2:3:4:5:6:7:8]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[1:2:3:4:5:6:7:8]/	=http	E11	E12	E13	=[1:2:3:4:5:6:7:8]	E15	=/	E16	E17	E18	=http://[1:2:3:4:5:6:7:8]:80/	=http://[1:2:3:4:5:6:7:8]/	=http://[1:2:3:4:5:6:7:8]/	=80	=http://[1:2:3:4:5:6:7:8]/	=/"###,
        ),
        (
            r###"S0:0:http://[1:2:3:4:5:6:7::8]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[1:2:3:4:5:6:7:8:9]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[1::2::3]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[12345::1]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%25eth0]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[fe80::1%25eth0]/	=http	E11	E12	E13	=[fe80::1]	E15	=/	E16	E17	=eth0	=http://[fe80::1%25eth0]:80/	=http://[fe80::1%25eth0]/	=http://[fe80::1%25eth0]/	=80	=http://[fe80::1%25eth0]/	=/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%eth0]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[fe80::1%25eth0]/	=http	E11	E12	E13	=[fe80::1]	E15	=/	E16	E17	=eth0	=http://[fe80::1%25eth0]:80/	=http://[fe80::1%25eth0]/	=http://[fe80::1%25eth0]/	=80	=http://[fe80::1%25eth0]/	=/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%25]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[fe80::1%2525]/	=http	E11	E12	E13	=[fe80::1]	E15	=/	E16	E17	=25	=http://[fe80::1%2525]:80/	=http://[fe80::1%2525]/	=http://[fe80::1%2525]/	=80	=http://[fe80::1%2525]/	=/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%251]:80/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[fe80::1%251]:80/	=http	E11	E12	E13	=[fe80::1]	=80	=/	E16	E17	=1	=http://[fe80::1%251]:80/	=http://[fe80::1%251]/	=http://[fe80::1%251]:80/	=80	=http://[fe80::1%251]:80/	=/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%2525]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[fe80::1%2525]/	=http	E11	E12	E13	=[fe80::1]	E15	=/	E16	E17	=25	=http://[fe80::1%2525]:80/	=http://[fe80::1%2525]/	=http://[fe80::1%2525]/	=80	=http://[fe80::1%2525]/	=/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%25abcdefghijklmnopq]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[fe80:0:0:0:0:0:0:1%25eth0]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[fe80::1%25eth0]/	=http	E11	E12	E13	=[fe80::1]	E15	=/	E16	E17	=eth0	=http://[fe80::1%25eth0]:80/	=http://[fe80::1%25eth0]/	=http://[fe80::1%25eth0]/	=80	=http://[fe80::1%25eth0]/	=/"###,
        ),
        (
            r###"S0:0:http://[g::1]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::1.2.3.04]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[1:2:3:4:5:6:1.2.3.4]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[1:2:3:4:5:6:1.2.3.4]/	=http	E11	E12	E13	=[1:2:3:4:5:6:1.2.3.4]	E15	=/	E16	E17	E18	=http://[1:2:3:4:5:6:1.2.3.4]:80/	=http://[1:2:3:4:5:6:1.2.3.4]/	=http://[1:2:3:4:5:6:1.2.3.4]/	=80	=http://[1:2:3:4:5:6:1.2.3.4]/	=/"###,
        ),
        (
            r###"S0:0:http://[1:2:3:4:5:6:7:1.2.3.4]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::1]:/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1]/	=http	E11	E12	E13	=[::1]	E15	=/	E16	E17	E18	=http://[::1]:80/	=http://[::1]/	=http://[::1]/	=80	=http://[::1]/	=/"###,
        ),
        (
            r###"S0:0:foo://bar	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E5	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:8:foo://bar	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=foo://bar/	=foo	E11	E12	E13	=bar	E15	=/	E16	E17	E18	=foo://bar/	=foo://bar/	=foo://bar/	E15	=foo://bar/	=/"###,
        ),
        (
            r###"S0:8:foo://bar:99/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=foo://bar:99/x	=foo	E11	E12	E13	=bar	=99	=/x	E16	E17	E18	=foo://bar:99/x	=foo://bar:99/x	=foo://bar:99/x	=99	=foo://bar:99/x	=/x"###,
        ),
        (
            r###"S0:8:foo:bar	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E28	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:foo:bar	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E5	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:8:foo:/bar	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=foo://bar/	=foo	E11	E12	E13	=bar	E15	=/	E16	E17	E18	=foo://bar/	=foo://bar/	=foo://bar/	E15	=foo://bar/	=/"###,
        ),
        (
            r###"S0:8:foo:////bar	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E28	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http:/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/	=http	E11	E12	E13	=x	E15	=/	E16	E17	E18	=http://x:80/	=http://x/	=http://x/	=80	=http://x/	=/"###,
        ),
        (
            r###"S0:0:http:////x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E28	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:ftp://ftp.example.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=ftp://ftp.example.com/	=ftp	E11	E12	E13	=ftp.example.com	E15	=/	E16	E17	E18	=ftp://ftp.example.com:21/	=ftp://ftp.example.com/	=ftp://ftp.example.com/	=21	=ftp://ftp.example.com/	=/"###,
        ),
        (
            r###"S0:0:ftp://u:p@x:21/a;type=i	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=ftp://u:p@x:21/a;type=i	=ftp	=u	=p	E13	=x	=21	=/a;type=i	E16	E17	E18	=ftp://u:p@x:21/a;type=i	=ftp://u:p@x/a;type=i	=ftp://u:p@x:21/a;type=i	=21	=ftp://u:p@x:21/a;type=i	=/a;type=i"###,
        ),
        (
            r###"S0:0:dict://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=dict://x/	=dict	E11	E12	E13	=x	E15	=/	E16	E17	E18	=dict://x:2628/	=dict://x/	=dict://x/	=2628	=dict://x/	=/"###,
        ),
        (
            r###"S0:0:ldap://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E5	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:gopher://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=gopher://x/	=gopher	E11	E12	E13	=x	E15	=/	E16	E17	E18	=gopher://x:70/	=gopher://x/	=gopher://x/	=70	=gopher://x/	=/"###,
        ),
        (
            r###"S0:0:ws://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=ws://x/	=ws	E11	E12	E13	=x	E15	=/	E16	E17	E18	=ws://x:80/	=ws://x/	=ws://x/	=80	=ws://x/	=/"###,
        ),
        (
            r###"S0:0:wss://x:443/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=wss://x:443/	=wss	E11	E12	E13	=x	=443	=/	E16	E17	E18	=wss://x:443/	=wss://x/	=wss://x:443/	=443	=wss://x:443/	=/"###,
        ),
        (
            r###"S0:0:rtsp://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=rtsp://x/	=rtsp	E11	E12	E13	=x	E15	=/	E16	E17	E18	=rtsp://x:554/	=rtsp://x/	=rtsp://x/	=554	=rtsp://x/	=/"###,
        ),
        (
            r###"S0:0:smb://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=smb://x/	=smb	E11	E12	E13	=x	E15	=/	E16	E17	E18	=smb://x:445/	=smb://x/	=smb://x/	=445	=smb://x/	=/"###,
        ),
        (
            r###"S0:0:mqtt://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=mqtt://x/	=mqtt	E11	E12	E13	=x	E15	=/	E16	E17	E18	=mqtt://x:1883/	=mqtt://x/	=mqtt://x/	=1883	=mqtt://x/	=/"###,
        ),
        (
            r###"S0:0:telnet://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=telnet://x/	=telnet	E11	E12	E13	=x	E15	=/	E16	E17	E18	=telnet://x:23/	=telnet://x/	=telnet://x/	=23	=telnet://x/	=/"###,
        ),
        (
            r###"S0:0:tftp://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=tftp://x/	=tftp	E11	E12	E13	=x	E15	=/	E16	E17	E18	=tftp://x:69/	=tftp://x/	=tftp://x/	=69	=tftp://x/	=/"###,
        ),
        (
            r###"S0:0:scp://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E5	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:rtmp://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E5	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:abcdefghijklmnopqrstuvwxyzabcdefghijklm://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E5	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:8:abcdefghijklmnopqrstuvwxyzabcdefghijklm://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=abcdefghijklmnopqrstuvwxyzabcdefghijklm://x/	=abcdefghijklmnopqrstuvwxyzabcdefghijklm	E11	E12	E13	=x	E15	=/	E16	E17	E18	=abcdefghijklmnopqrstuvwxyzabcdefghijklm://x/	=abcdefghijklmnopqrstuvwxyzabcdefghijklm://x/	=abcdefghijklmnopqrstuvwxyzabcdefghijklm://x/	E15	=abcdefghijklmnopqrstuvwxyzabcdefghijklm://x/	=/"###,
        ),
        (
            r###"S0:8:abcdefghijklmnopqrstuvwxyzabcdefghijklmn://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=abcdefghijklmnopqrstuvwxyzabcdefghijklmn://x/	=abcdefghijklmnopqrstuvwxyzabcdefghijklmn	E11	E12	E13	=x	E15	=/	E16	E17	E18	=abcdefghijklmnopqrstuvwxyzabcdefghijklmn://x/	=abcdefghijklmnopqrstuvwxyzabcdefghijklmn://x/	=abcdefghijklmnopqrstuvwxyzabcdefghijklmn://x/	E15	=abcdefghijklmnopqrstuvwxyzabcdefghijklmn://x/	=/"###,
        ),
        (
            r###"S0:8:a+b-c.d://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=a+b-c.d://x/	=a+b-c.d	E11	E12	E13	=x	E15	=/	E16	E17	E18	=a+b-c.d://x/	=a+b-c.d://x/	=a+b-c.d://x/	E15	=a+b-c.d://x/	=/"###,
        ),
        (
            r###"S0:8:1a://x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E27	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E27	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E27	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com/	=http	E11	E12	E13	=example.com	E15	=/	E16	E17	E18	=http://example.com:80/	=http://example.com/	=http://example.com/	=80	=http://example.com/	=/"###,
        ),
        (
            r###"S0:512:example.com/x?y#z	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com/x?y#z	=http	E11	E12	E13	=example.com	E15	=/x	=y	=z	E18	=http://example.com:80/x?y#z	=http://example.com/x?y#z	=http://example.com/x?y#z	=80	=http://example.com/x?y#z	=/x"###,
        ),
        (
            r###"S0:512:ftp.example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=ftp://ftp.example.com/	=ftp	E11	E12	E13	=ftp.example.com	E15	=/	E16	E17	E18	=ftp://ftp.example.com:21/	=ftp://ftp.example.com/	=ftp://ftp.example.com/	=21	=ftp://ftp.example.com/	=/"###,
        ),
        (
            r###"S0:512:FTP.example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=ftp://FTP.example.com/	=ftp	E11	E12	E13	=FTP.example.com	E15	=/	E16	E17	E18	=ftp://FTP.example.com:21/	=ftp://FTP.example.com/	=ftp://FTP.example.com/	=21	=ftp://FTP.example.com/	=/"###,
        ),
        (
            r###"S0:512:dict.x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=dict://dict.x/	=dict	E11	E12	E13	=dict.x	E15	=/	E16	E17	E18	=dict://dict.x:2628/	=dict://dict.x/	=dict://dict.x/	=2628	=dict://dict.x/	=/"###,
        ),
        (
            r###"S0:512:ldap.x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=ldap://ldap.x/	=ldap	E11	E12	E13	=ldap.x	E15	=/	E16	E17	E18	=ldap://ldap.x/	=ldap://ldap.x/	=ldap://ldap.x/	E15	=ldap://ldap.x/	=/"###,
        ),
        (
            r###"S0:512:imap.x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=imap://imap.x/	=imap	E11	E12	E13	=imap.x	E15	=/	E16	E17	E18	=imap://imap.x:143/	=imap://imap.x/	=imap://imap.x/	=143	=imap://imap.x/	=/"###,
        ),
        (
            r###"S0:512:smtp.x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=smtp://smtp.x/	=smtp	E11	E12	E13	=smtp.x	E15	=/	E16	E17	E18	=smtp://smtp.x:25/	=smtp://smtp.x/	=smtp://smtp.x/	=25	=smtp://smtp.x/	=/"###,
        ),
        (
            r###"S0:512:pop3.x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=pop3://pop3.x/	=pop3	E11	E12	E13	=pop3.x	E15	=/	E16	E17	E18	=pop3://pop3.x:110/	=pop3://pop3.x/	=pop3://pop3.x/	=110	=pop3://pop3.x/	=/"###,
        ),
        (
            r###"S0:512:example.com:8080	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com:8080/	=http	E11	E12	E13	=example.com	=8080	=/	E16	E17	E18	=http://example.com:8080/	=http://example.com:8080/	=http://example.com:8080/	=8080	=http://example.com:8080/	=/"###,
        ),
        (
            r###"S0:512:example.com:	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:data:x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:localhost/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://localhost/x	=http	E11	E12	E13	=localhost	E15	=/x	E16	E17	E18	=http://localhost:80/x	=http://localhost/x	=http://localhost/x	=80	=http://localhost/x	=/x"###,
        ),
        (
            r###"S0:4:example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=https://example.com/	=https	E11	E12	E13	=example.com	E15	=/	E16	E17	E18	=https://example.com:443/	=https://example.com/	=https://example.com/	=443	=https://example.com/	=/"###,
        ),
        (
            r###"S0:516:ftp.example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=https://ftp.example.com/	=https	E11	E12	E13	=ftp.example.com	E15	=/	E16	E17	E18	=https://ftp.example.com:443/	=https://ftp.example.com/	=https://ftp.example.com/	=443	=https://ftp.example.com/	=/"###,
        ),
        (
            r###"S0:512://example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E14	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:user:pw@example.com	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://user:pw@example.com/	=http	=user	=pw	E13	=example.com	E15	=/	E16	E17	E18	=http://user:pw@example.com:80/	=http://user:pw@example.com/	=http://user:pw@example.com/	=80	=http://user:pw@example.com/	=/"###,
        ),
        (
            r###"S0:512:[::1]:80/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1]:80/	=http	E11	E12	E13	=[::1]	=80	=/	E16	E17	E18	=http://[::1]:80/	=http://[::1]/	=http://[::1]:80/	=80	=http://[::1]:80/	=/"###,
        ),
        (
            r###"S0:1024:foo:///x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E5	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:1032:foo:///x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=foo://x/	=foo	E11	E12	E13	=x	E15	=/	E16	E17	E18	=foo://x/	=foo://x/	=foo://x/	E15	=foo://x/	=/"###,
        ),
        (
            r###"S0:1032:foo:/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=foo://x/	=foo	E11	E12	E13	=x	E15	=/	E16	E17	E18	=foo://x/	=foo://x/	=foo://x/	E15	=foo://x/	=/"###,
        ),
        (
            r###"S0:8:foo:///x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=foo://x/	=foo	E11	E12	E13	=x	E15	=/	E16	E17	E18	=foo://x/	=foo://x/	=foo://x/	E15	=foo://x/	=/"###,
        ),
        (
            r###"S0:0:file:///tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x	=file	E11	E12	E13	E14	E15	=/tmp/x	E16	E17	E18	=file:///tmp/x	=file:///tmp/x	=file:///tmp/x	=0	=file:///tmp/x	=/tmp/x"###,
        ),
        (
            r###"S0:0:file:/tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x	=file	E11	E12	E13	E14	E15	=/tmp/x	E16	E17	E18	=file:///tmp/x	=file:///tmp/x	=file:///tmp/x	=0	=file:///tmp/x	=/tmp/x"###,
        ),
        (
            r###"S0:0:file:tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://tmp/x	=file	E11	E12	E13	E14	E15	=tmp/x	E16	E17	E18	=file://tmp/x	=file://tmp/x	=file://tmp/x	=0	=file://tmp/x	=tmp/x"###,
        ),
        (
            r###"S0:0:file://localhost/tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x	=file	E11	E12	E13	E14	E15	=/tmp/x	E16	E17	E18	=file:///tmp/x	=file:///tmp/x	=file:///tmp/x	=0	=file:///tmp/x	=/tmp/x"###,
        ),
        (
            r###"S0:0:file://LOCALHOST/tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x	=file	E11	E12	E13	E14	E15	=/tmp/x	E16	E17	E18	=file:///tmp/x	=file:///tmp/x	=file:///tmp/x	=0	=file:///tmp/x	=/tmp/x"###,
        ),
        (
            r###"S0:0:file://127.0.0.1/tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x	=file	E11	E12	E13	E14	E15	=/tmp/x	E16	E17	E18	=file:///tmp/x	=file:///tmp/x	=file:///tmp/x	=0	=file:///tmp/x	=/tmp/x"###,
        ),
        (
            r###"S0:0:file://otherhost/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file://localhost	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file://localhost/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://(nil)	=file	E11	E12	E13	E14	E15	=/	E16	E17	E18	=file://(nil)	=file://(nil)	=file://(nil)	=0	=file://(nil)	=/"###,
        ),
        (
            r###"S0:0:file:///	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://(nil)	=file	E11	E12	E13	E14	E15	=/	E16	E17	E18	=file://(nil)	=file://(nil)	=file://(nil)	=0	=file://(nil)	=/"###,
        ),
        (
            r###"S0:0:file://	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:///c:/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:///C|/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file://c:/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:c:/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:///c:	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:///cx/y	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///cx/y	=file	E11	E12	E13	E14	E15	=/cx/y	E16	E17	E18	=file:///cx/y	=file:///cx/y	=file:///cx/y	=0	=file:///cx/y	=/cx/y"###,
        ),
        (
            r###"S0:0:file:///tmp/../x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///x	=file	E11	E12	E13	E14	E15	=/x	E16	E17	E18	=file:///x	=file:///x	=file:///x	=0	=file:///x	=/x"###,
        ),
        (
            r###"S0:0:file:///tmp/x?q#f	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x?q#f	=file	E11	E12	E13	E14	E15	=/tmp/x	=q	=f	E18	=file:///tmp/x?q#f	=file:///tmp/x?q#f	=file:///tmp/x?q#f	=0	=file:///tmp/x?q#f	=/tmp/x"###,
        ),
        (
            r###"S0:0:file:///tmp/x y	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:..	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://	=file	E11	E12	E13	E14	E15	=	E16	E17	E18	=file://	=file://	=file://	=0	=file://	="###,
        ),
        (
            r###"S0:0:file:.abc	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://abc	=file	E11	E12	E13	E14	E15	=abc	E16	E17	E18	=file://abc	=file://abc	=file://abc	=0	=file://abc	=abc"###,
        ),
        (
            r###"S0:512:file:///tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x	=file	E11	E12	E13	E14	E15	=/tmp/x	E16	E17	E18	=file:///tmp/x	=file:///tmp/x	=file:///tmp/x	=0	=file:///tmp/x	=/tmp/x"###,
        ),
        (
            r###"S0:512:file:tmp	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:FILE:///tmp/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///tmp/x	=file	E11	E12	E13	E14	E15	=/tmp/x	E16	E17	E18	=file:///tmp/x	=file:///tmp/x	=file:///tmp/x	=0	=file:///tmp/x	=/tmp/x"###,
        ),
        (
            r###"S0:0:http://host/?q=a+b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/?q=a+b	=http	E11	E12	E13	=host	E15	=/	=q=a+b	E17	E18	=http://host:80/?q=a+b	=http://host/?q=a+b	=http://host/?q=a+b	=80	=http://host/?q=a+b	=/"###,
        ),
        (
            r###"S0:0:http://host/p%41th?%41=%42	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://host/p%41th?%41=%42	=http	E11	E12	E13	=host	E15	=/p%41th	=%41=%42	E17	E18	=http://host:80/p%41th?%41=%42	=http://host/p%41th?%41=%42	=http://host/p%41th?%41=%42	=80	=http://host/p%41th?%41=%42	=/p%41th"###,
        ),
        (
            r###"S0:0:http://x/a?b#c?d#e	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/a?b#c?d#e	=http	E11	E12	E13	=x	E15	=/a	=b	=c?d#e	E18	=http://x:80/a?b#c?d#e	=http://x/a?b#c?d#e	=http://x/a?b#c?d#e	=80	=http://x/a?b#c?d#e	=/a"###,
        ),
        (
            r###"S0:0:http://x/?#?	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/#?	=http	E11	E12	E13	=x	E15	=/	E16	=?	E18	=http://x:80/#?	=http://x/#?	=http://x/#?	=80	=http://x/?#?	=/"###,
        ),
        (
            r###"S0:0:http://x:12/#	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x:12/	=http	E11	E12	E13	=x	=12	=/	E16	E17	E18	=http://x:12/	=http://x:12/	=http://x:12/	=12	=http://x:12/#	=/"###,
        ),
        (
            r###"S0:0:https://[::1]:443/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=https://[::1]:443/	=https	E11	E12	E13	=[::1]	=443	=/	E16	E17	E18	=https://[::1]:443/	=https://[::1]/	=https://[::1]:443/	=443	=https://[::1]:443/	=/"###,
        ),
        (
            r###"S0:0:http://x/a\x09b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://x/a\\b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/a\\b	=http	E11	E12	E13	=x	E15	=/a\\b	E16	E17	E18	=http://x:80/a\\b	=http://x/a\\b	=http://x/a\\b	=80	=http://x/a\\b	=/a\\b"###,
        ),
        (
            r###"S0:0:http://x\\y/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E21	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://x/%zz	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/%zz	=http	E11	E12	E13	=x	E15	=/%zz	E16	E17	E18	=http://x:80/%zz	=http://x/%zz	=http://x/%zz	=80	=http://x/%zz	=/%zz"###,
        ),
        (
            r###"S0:0:http://x/%	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/%	=http	E11	E12	E13	=x	E15	=/%	E16	E17	E18	=http://x:80/%	=http://x/%	=http://x/%	=80	=http://x/%	=/%"###,
        ),
        (
            r###"S0:0:http://user%00:x@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://user%00:x@host/	=http	=user%00	=x	E13	=host	E15	=/	E16	E17	E18	=http://user%00:x@host:80/	=http://user%00:x@host/	=http://user%00:x@host/	=80	=http://user%00:x@host/	=/"###,
        ),
        (
            r###"S0:0:http://us\xffer:x@host/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://us\xffer:x@host/	=http	=us\xffer	=x	E13	=host	E15	=/	E16	E17	E18	=http://us\xffer:x@host:80/	=http://us\xffer:x@host/	=http://us\xffer:x@host/	=80	=http://us\xffer:x@host/	=/"###,
        ),
        (
            r###"S0:0:http://x/;a=b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://x/;a=b	=http	E11	E12	E13	=x	E15	=/;a=b	E16	E17	E18	=http://x:80/;a=b	=http://x/;a=b	=http://x/;a=b	=80	=http://x/;a=b	=/;a=b"###,
        ),
        (
            r###"S0:0:smtp://u;AUTH=*:p@x/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=smtp://u:p;AUTH=*@x/	=smtp	=u	=p	=AUTH=*	=x	E15	=/	E16	E17	E18	=smtp://u:p;AUTH=*@x:25/	=smtp://u:p;AUTH=*@x/	=smtp://u:p;AUTH=*@x/	=25	=smtp://u:p;AUTH=*@x/	=/"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0:../x	G0:0"###,
            r###"0	0	=http://example.com/a/x"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0:x	G0:0"###,
            r###"0	0	=http://example.com/a/b/x"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0:./	G0:0"###,
            r###"0	0	=http://example.com/a/b/"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0:/root	G0:0"###,
            r###"0	0	=http://example.com/root"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0://other.com/x	G0:0"###,
            r###"0	0	=http://other.com/x"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0://other.com	G0:0"###,
            r###"0	0	=http://other.com/"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0:?z	G0:0"###,
            r###"0	0	=http://example.com/a/b/c?z"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?q=1#f	S0:0:#g	G0:0"###,
            r###"0	0	=http://example.com/a/b/c?q=1#g"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:0:#g	G0:0"###,
            r###"0	0	=http://example.com/a/b/c#g"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c#f	S0:0:?z	G0:0"###,
            r###"0	0	=http://example.com/a/b/c?z"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c#f	S0:0:z	G0:0"###,
            r###"0	0	=http://example.com/a/b/z"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c?	S0:0:z	G0:0"###,
            r###"0	0	=http://example.com/a/b/z"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:0:	G0:0"###,
            r###"0	0	=http://example.com/a/b/c"###,
        ),
        (
            r###"S0:0:http://example.com	S0:0:x	G0:0"###,
            r###"0	0	=http://example.com/x"###,
        ),
        (
            r###"S0:0:http://example.com?a/b	S0:0:x	G0:0"###,
            r###"0	0	=http://example.com/x"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:0:https://other/	G0:0"###,
            r###"0	0	=https://other/"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:0:ftp://other/	G0:0"###,
            r###"0	0	=ftp://other/"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:0:foo://other/	G0:0"###,
            r###"0	E5	=http://example.com/a/b/c"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:8:foo://other/	G0:0"###,
            r###"0	0	=foo://other/"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:0:../../../../x	G0:0"###,
            r###"0	0	=http://example.com/x"###,
        ),
        (
            r###"S0:16:http://example.com/a/b/c	S0:16:../x	G0:0"###,
            r###"0	0	=http://example.com/a/x"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:0:a b	G0:0"###,
            r###"0	0	=http://example.com/a/b/a%20b"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:2048:a b	G0:0"###,
            r###"0	0	=http://example.com/a/b/a%20b"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:2176:a b?c d#e f	G0:0"###,
            r###"0	0	=http://example.com/a/b/a%20b?c+d#e+f"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:2176://h st/x y	G0:0"###,
            r###"0	E21	=http://example.com/a/b/c"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:2176://host/a//b c	G0:0"###,
            r###"0	0	=http://host/a//b%20c"###,
        ),
        (
            r###"S0:0:http://example.com/a/b/c	S0:2176:\xc3\xa4	G0:0"###,
            r###"0	0	=http://example.com/a/b/%c3%a4"###,
        ),
        (
            r###"S0:0:http://u:p@example.com:8080/a/b/c	S0:0:x	G0:0"###,
            r###"0	0	=http://u:p@example.com:8080/a/b/x"###,
        ),
        (
            r###"S0:0:http://[fe80::1%25eth0]:8080/a	S0:0:x	G0:0"###,
            r###"0	0	=http://[fe80::1%25eth0]:8080/x"###,
        ),
        (
            r###"S0:0:file:///tmp/a/b	S0:0:c	G0:0"###,
            r###"0	0	=file:///tmp/a/c"###,
        ),
        (
            r###"S0:0:file:///tmp/a/b	S0:0:/etc/x	G0:0"###,
            r###"0	0	=file:///etc/x"###,
        ),
        (
            r###"S0:0:file:///tmp/a/b	S0:0://localhost/x	G0:0"###,
            r###"0	0	=file:///x"###,
        ),
        (
            r###"S0:0:file:///tmp/a/b	S0:0://other/x	G0:0"###,
            r###"0	E19	=file:///tmp/a/b"###,
        ),
        (
            r###"S0:0:http://h/a	S0:0:c:x	G0:0"###,
            r###"0	E5	=http://h/a"###,
        ),
        (
            r###"S0:0:http://h/a	S0:0:c:/x	G0:0"###,
            r###"0	E5	=http://h/a"###,
        ),
        (
            r###"S0:512:http://h/a	S0:512:c:x	G0:0"###,
            r###"0	0	=http://h/c:x"###,
        ),
        (
            r###"S0:0:http://h/a	S0:512:x.com/y	G0:0"###,
            r###"0	0	=http://h/x.com/y"###,
        ),
        (
            r###"S0:0:http://h/a	S0:0:http:x	G0:0"###,
            r###"0	E28	=http://h/a"###,
        ),
        (
            r###"S0:0:http://h/a	S0:0:\\x	G0:0"###,
            r###"0	0	=http://h/\\x"###,
        ),
        (
            r###"S0:0:http://h/a	S0:0:/\x01	G0:0"###,
            r###"0	0	=http://h/%01"###,
        ),
        (
            r###"S0:0:http://h/a	S0:2048:/\x01	G0:0"###,
            r###"0	0	=http://h/%01"###,
        ),
        (
            r###"S0:0:http://h/a	S0:0:hello\xff	G0:0"###,
            r###"0	0	=http://h/hello%ff"###,
        ),
        (
            r###"S0:2048:http://h/a b	G0:0	G7:0	G7:128	G7:64"###,
            r###"0	=http://h/a b	=/a b	=/a%20b	=/a b"###,
        ),
        (
            r###"S1:0:https	G0:0	S5:0:h	G0:0"###,
            r###"0	E14	0	=https://h/"###,
        ),
        (
            r###"S1:0:HTTP	S5:0:h	G0:0	G1:0"###,
            r###"0	0	=HTTP://h/	=HTTP"###,
        ),
        (r###"S1:0:foo	G1:0"###, r###"E5	E10"###),
        (
            r###"S1:8:foo	G1:0	S5:0:h	G0:0"###,
            r###"0	=foo	0	=foo://h/"###,
        ),
        (r###"S1:8:f!o	G1:0"###, r###"E27	E10"###),
        (r###"S1:8:fo!	G1:0"###, r###"0	=fo!"###),
        (r###"S1:8:1fo	G1:0"###, r###"E27	E10"###),
        (r###"S1:8:	G1:0"###, r###"E27	E10"###),
        (
            r###"S1:8:abcdefghijklmnopqrstuvwxyzabcdefghijklmn	G1:0"###,
            r###"0	=abcdefghijklmnopqrstuvwxyzabcdefghijklmn"###,
        ),
        (
            r###"S1:8:abcdefghijklmnopqrstuvwxyzabcdefghijklmno	G1:0"###,
            r###"E27	E10"###,
        ),
        (
            r###"S5:0:example.com	G0:0	G0:4	G5:0"###,
            r###"0	E10	=https://example.com/	=example.com"###,
        ),
        (r###"S5:0:ex ample	G5:0"###, r###"E21	E14"###),
        (r###"S5:0:ex%41mple	G5:0"###, r###"0	=ex%41mple"###),
        (r###"S5:0:ex%2Fmple	G5:0"###, r###"E21	E14"###),
        (r###"S5:0:ex%00mple	G5:0"###, r###"E21	E14"###),
        (r###"S5:128:ex ample	G5:0"###, r###"E21	E14"###),
        (r###"S5:128:\xc3\xa4	G5:0	G5:64"###, r###"E21	E14	E14"###),
        (
            r###"S5:0:\xc3\xa4	G5:0	G5:4096	G5:8192	S1:0:http	G0:0	G0:4096	G0:8192	G0:128"###,
            r###"0	=\xc3\xa4	E30	=\xc3\xa4	0	=http://\xc3\xa4/	E30	=http://\xc3\xa4/	=http://%C3%A4/"###,
        ),
        (r###"S5:0:	G5:0"###, r###"E21	E14"###),
        (r###"S5:1024:	G5:0"###, r###"0	="###),
        (r###"S5:0:[::1]	G5:0"###, r###"0	=[::1]"###),
        (r###"S5:0:[::1x	G5:0"###, r###"0	=[::1x"###),
        (
            r###"S5:0:[fe80::1%25eth0]	G5:0	G10:0	S1:0:http	G0:0"###,
            r###"0	=[fe80::1%25eth0]	=eth0	0	=http://[fe80::1%25eth0%25eth0]/"###,
        ),
        (r###"S5:128:[::1]	G5:0"###, r###"E21	E14"###),
        (
            r###"S0:0:http://[fe80::1%25eth0]/	G10:0	S5:0:h	G10:0	G0:0"###,
            r###"0	=eth0	0	E18	=http://h/"###,
        ),
        (
            r###"S0:0:http://[fe80::1%25eth0]/	S10:0:lo	G0:0"###,
            r###"0	0	=http://[fe80::1%25lo]/"###,
        ),
        (r###"S6:0:80	G6:0"###, r###"0	=80"###),
        (r###"S6:0:0080	G6:0"###, r###"0	=80"###),
        (r###"S6:0:65536	G6:0"###, r###"E4	E15"###),
        (r###"S6:0:	G6:0"###, r###"E4	E15"###),
        (r###"S6:0:x	G6:0"###, r###"E4	E15"###),
        (r###"S6:0:8x	G6:0"###, r###"E4	E15"###),
        (
            r###"S0:0:http://h/	S6:0:80	G0:0	G0:2	G6:2	G6:1	Z6:0	G0:0	G6:1"###,
            r###"0	0	=http://h:80/	=http://h/	E15	=80	0	=http://h/	=80"###,
        ),
        (
            r###"S0:0:https://h:443/	G0:2	G0:1	G6:2"###,
            r###"0	=https://h/	=https://h:443/	E15"###,
        ),
        (
            r###"S0:0:http://h/	S7:0:a b	G0:0	G7:0"###,
            r###"0	0	=http://h/a b	=/a b"###,
        ),
        (
            r###"S0:0:http://h/	S7:128:a b/c?d	G0:0	G7:0"###,
            r###"0	0	=http://h/a%20b/c%3fd	=/a%20b/c%3fd"###,
        ),
        (
            r###"S0:0:http://h/	S7:0:%2F%aB	G7:0"###,
            r###"0	0	=/%2f%ab"###,
        ),
        (
            r###"S0:0:http://h/	S7:0:	G7:0	G0:0"###,
            r###"0	0	=/	=http://h/"###,
        ),
        (
            r###"S0:0:http://h/	S7:128:	G7:0	G0:0"###,
            r###"0	0	=/	=http://h/"###,
        ),
        (
            r###"S0:0:http://h/	S8:0:a=b	G0:0	G8:0"###,
            r###"0	0	=http://h/?a=b	=a=b"###,
        ),
        (
            r###"S0:0:http://h/	S8:128:a b=c&d	G0:0	G8:0	G8:64"###,
            r###"0	0	=http://h/?a+b%3dc%26d	=a+b%3dc%26d	=a b=c&d"###,
        ),
        (
            r###"S0:0:http://h/?x=1	S8:384:a b=c=d	G0:0"###,
            r###"0	0	=http://h/?x=1&a+b=c%3dd"###,
        ),
        (
            r###"S0:0:http://h/?x=1&	S8:384:a=b	G0:0"###,
            r###"0	0	=http://h/?x=1&a=b"###,
        ),
        (
            r###"S0:0:http://h/	S8:384:a=b	G0:0"###,
            r###"0	0	=http://h/?a=b"###,
        ),
        (
            r###"S0:0:http://h/?x=1	S8:256:a=b	G0:0"###,
            r###"0	0	=http://h/?x=1&a=b"###,
        ),
        (
            r###"S0:0:http://h/	S8:0:	G8:0	G8:16384	G0:0	G0:16384"###,
            r###"0	0	E16	=	=http://h/	=http://h/?"###,
        ),
        (
            r###"S0:0:http://h/	S8:128:	G8:0	G8:16384	G0:16384"###,
            r###"0	0	E16	E16	=http://h/?"###,
        ),
        (
            r###"S0:0:http://h/?a	Z8:0	G0:0	G0:16384"###,
            r###"0	0	=http://h/	=http://h/"###,
        ),
        (
            r###"S0:0:http://h/	S9:0:	G9:0	G9:16384	G0:0	G0:16384"###,
            r###"0	0	=	=	=http://h/#	=http://h/#"###,
        ),
        (
            r###"S0:0:http://h/	S9:128:a b#	G9:0	G0:0"###,
            r###"0	0	=a%20b%23	=http://h/#a%20b%23"###,
        ),
        (
            r###"S0:0:http://h/#x	Z9:0	G0:16384"###,
            r###"0	0	=http://h/"###,
        ),
        (
            r###"S0:0:http://h/	S2:0:u s	S3:0:p:w	G0:0"###,
            r###"0	0	0	=http://u s:p:w@h/"###,
        ),
        (
            r###"S0:0:http://h/	S2:128:u s	S3:128:p:w	G0:0	G2:64	G3:64"###,
            r###"0	0	0	=http://u%20s:p%3aw@h/	=u s	=p:w"###,
        ),
        (
            r###"S0:0:http://h/	S4:0:opt	G0:0	G4:0"###,
            r###"0	0	=http://h/	=opt"###,
        ),
        (
            r###"S0:0:imap://h/	S4:0:opt	G0:0"###,
            r###"0	0	=imap://;opt@h/"###,
        ),
        (
            r###"S0:0:imap://h/	S4:0:opt	S2:0:u	G0:0"###,
            r###"0	0	0	=imap://u;opt@h/"###,
        ),
        (r###"S0:0:http://h/	S2:0:	G0:0"###, r###"0	0	=http://@h/"###),
        (
            r###"S0:0:http://h/	S2:0:%41%4a	G2:0	G2:64"###,
            r###"0	0	=%41%4a	=AJ"###,
        ),
        (
            r###"S0:0:http://h/	S2:0:%0a	G2:0	G2:64"###,
            r###"0	0	=%0a	E6"###,
        ),
        (
            r###"S0:0:http://h/a	Z0:0	G0:0	G5:0"###,
            r###"0	0	E14	E14"###,
        ),
        (
            r###"S0:0:http://h/a	Z1:0	G0:0	G0:4"###,
            r###"0	0	E10	=https://h/a"###,
        ),
        (
            r###"S0:512:example.com/x	G0:0	G0:32768	G1:0	G1:32768	D	G0:32768	G1:32768"###,
            r###"0	=http://example.com/x	=example.com/x	=http	E10	D	=http://example.com/x	=http"###,
        ),
        (
            r###"S0:512:example.com/x	S1:0:http	G0:32768"###,
            r###"0	0	=http://example.com/x"###,
        ),
        (
            r###"S0:0:http://h/?a+b%20c	G8:0	G8:64	G8:128	G8:192"###,
            r###"0	=a+b%20c	=a b c	=a+b%20c	=a+b+c"###,
        ),
        (
            r###"S0:0:http://h/a%20b+c	G7:64	G7:128"###,
            r###"0	=/a b+c	=/a%20b+c"###,
        ),
        (r###"S0:0:http://h/%0a	G7:64	G7:0"###, r###"0	E6	=/%0a"###),
        (
            r###"S0:0:http://h/	S5:0:ex%41mple.com	G5:0	G0:0	G5:64"###,
            r###"0	0	=ex%41mple.com	=http://ex%41mple.com/	=exAmple.com"###,
        ),
        (
            r###"S0:0:http://h:8080/	S1:0:https	G0:0	G0:2"###,
            r###"0	0	=https://h:8080/	=https://h:8080/"###,
        ),
        (
            r###"S0:0:https://h	G0:1	G6:1"###,
            r###"0	=https://h:443/	=443"###,
        ),
        (r###"S0:0:file:///x	G6:1	G0:1"###, r###"0	=0	=file:///x"###),
        (r###"S0:8:foo://h/	G6:1	G0:1"###, r###"0	E15	=foo://h/"###),
        (r###"S5:0:h	G0:0"###, r###"0	E10"###),
        (
            r###"S5:0:h	G0:4	G0:5"###,
            r###"0	=https://h/	=https://h:443/"###,
        ),
        (
            r###"S0:0:http://h/	S7:0:/../a/./b	G0:0"###,
            r###"0	0	=http://h/../a/./b"###,
        ),
        (r###"S0:0:http://ex ample.com/	G0:0"###, r###"E3	E14"###),
        (
            r###"S0:0:http://h/\xc3\xa4	G0:0	G7:128	G0:128"###,
            r###"0	=http://h/\xc3\xa4	=/%c3%a4	=http://h/\xc3\xa4"###,
        ),
        (
            r###"S0:0:http://\xc3\xa4/	G0:4096	G5:4096	G5:8192	G0:128"###,
            r###"0	E30	E30	=\xc3\xa4	=http://%C3%A4/"###,
        ),
        (
            r###"S0:0:http://h/	S5:0:h%2Fx	G5:0	G0:0	G0:128"###,
            r###"0	E21	=h	=http://h/	=http://h/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	R../x"###,
            r###"0	=http://example.com/a/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	F../x"###,
            r###"0	=http://example.com/a/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	T../x"###,
            r###"0	=http://example.com/a/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	Rx"###,
            r###"0	=http://example.com/a/b/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	Fx"###,
            r###"0	=http://example.com/a/b/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	Tx"###,
            r###"0	=http://example.com/a/b/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	R/root"###,
            r###"0	=http://example.com/root"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	F/root"###,
            r###"0	=http://example.com/root"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	T/root"###,
            r###"0	=http://example.com/root"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	R//other.com/x"###,
            r###"0	=http://other.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	F//other.com/x"###,
            r###"0	=http://other.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	T//other.com/x"###,
            r###"0	=http://other.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	R?z"###,
            r###"0	=http://example.com/a/b/c?z"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	F?z"###,
            r###"0	=http://example.com/a/b/c?z"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	T?z"###,
            r###"0	=http://example.com/a/b/c?z"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	R#g"###,
            r###"0	=http://example.com/a/b/c?q=1#g"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	F#g"###,
            r###"0	=http://example.com/a/b/c?q=1#g"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c?q=1#f	T#g"###,
            r###"0	=http://example.com/a/b/c?q=1#g"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttps://other.org/p?q"###,
            r###"0	=https://other.org/p?q"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttps://other.org/p?q"###,
            r###"0	=https://other.org/p?q"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttps://other.org/p?q"###,
            r###"0	=https://other.org/p?q"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rftp://other.org/p"###,
            r###"0	=ftp://other.org/p"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fftp://other.org/p"###,
            r###"0	=ftp://other.org/p"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Tftp://other.org/p"###,
            r###"0	=ftp://other.org/p"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rfoo://other.org/p"###,
            r###"0	E5"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Ffoo://other.org/p"###,
            r###"0	=foo://other.org/p"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Tfoo://other.org/p"###,
            r###"0	E5"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rfoo:bar"###,
            r###"0	E5"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Ffoo:bar"###,
            r###"0	=foo:bar"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Tfoo:bar"###,
            r###"0	E5"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Ra b"###,
            r###"0	=http://example.com/a/b/a%20b"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fa b"###,
            r###"0	=http://example.com/a/b/a%20b"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Ta b"###,
            r###"0	=http://example.com/a/b/a%20b"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R/a b?c d#e f"###,
            r###"0	=http://example.com/a%20b?c+d#e+f"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F/a b?c d#e f"###,
            r###"0	=http://example.com/a%20b?c+d#e+f"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T/a b?c d#e f"###,
            r###"0	=http://example.com/a%20b?c+d#e+f"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://other/a b?c d"###,
            r###"0	=http://other/a%20b?c+d"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://other/a b?c d"###,
            r###"0	=http://other/a b?c d"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://other/a b?c d"###,
            r###"0	=http://other/a b?c d"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R\xc3\xa4/\xc3\xb6?\xc3\xbc"###,
            r###"0	=http://example.com/a/b/%c3%a4/%c3%b6?%c3%bc"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F\xc3\xa4/\xc3\xb6?\xc3\xbc"###,
            r###"0	=http://example.com/a/b/%c3%a4/%c3%b6?%c3%bc"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T\xc3\xa4/\xc3\xb6?\xc3\xbc"###,
            r###"0	=http://example.com/a/b/%c3%a4/%c3%b6?%c3%bc"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://other/\xc3\xa4"###,
            r###"0	=http://other/%c3%a4"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://other/\xc3\xa4"###,
            r###"0	=http://other/\xc3\xa4"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://other/\xc3\xa4"###,
            r###"0	=http://other/\xc3\xa4"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R//h st/x"###,
            r###"0	E21"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F//h st/x"###,
            r###"0	=//h st/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T//h st/x"###,
            r###"0	E21"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://h st/x"###,
            r###"0	E21"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://h st/x"###,
            r###"0	=http://h st/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://h st/x"###,
            r###"0	E21"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://"###,
            r###"0	E14"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://"###,
            r###"0	=http://"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://"###,
            r###"0	E14"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://a:99999/"###,
            r###"0	E4"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://a:99999/"###,
            r###"0	=http://a:99999/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://a:99999/"###,
            r###"0	E4"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R../../../../../x"###,
            r###"0	=http://example.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F../../../../../x"###,
            r###"0	=http://example.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T../../../../../x"###,
            r###"0	=http://example.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://x/../y"###,
            r###"0	=http://x/y"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://x/../y"###,
            r###"0	=http://x/y"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://x/../y"###,
            r###"0	=http://x/y"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R/\x01"###,
            r###"0	=http://example.com/%01"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F/\x01"###,
            r###"0	=http://example.com/%01"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T/\x01"###,
            r###"0	=http://example.com/%01"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://[::1]:8080/x"###,
            r###"0	=http://[::1]:8080/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://[::1]:8080/x"###,
            r###"0	=http://[::1]:8080/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://[::1]:8080/x"###,
            r###"0	=http://[::1]:8080/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R//[::1]/x"###,
            r###"0	=http://[::1]/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F//[::1]/x"###,
            r###"0	=http://[::1]/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T//[::1]/x"###,
            r###"0	=http://[::1]/x"###,
        ),
        (
            r###"S0:520:http://example.com:8080/a/b/c	R/x"###,
            r###"0	=http://example.com:8080/x"###,
        ),
        (
            r###"S0:520:http://example.com:8080/a/b/c	F/x"###,
            r###"0	=http://example.com:8080/x"###,
        ),
        (
            r###"S0:520:http://example.com:8080/a/b/c	T/x"###,
            r###"0	=http://example.com:8080/x"###,
        ),
        (
            r###"S0:520:http://u:p@example.com/a/b/c	R/x"###,
            r###"0	=http://u:p@example.com/x"###,
        ),
        (
            r###"S0:520:http://u:p@example.com/a/b/c	F/x"###,
            r###"0	=http://u:p@example.com/x"###,
        ),
        (
            r###"S0:520:http://u:p@example.com/a/b/c	T/x"###,
            r###"0	=http://u:p@example.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp://user:pw@other/"###,
            r###"0	=http://user:pw@other/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp://user:pw@other/"###,
            r###"0	=http://user:pw@other/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp://user:pw@other/"###,
            r###"0	=http://user:pw@other/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rfile:///etc/passwd"###,
            r###"0	=file:///etc/passwd"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Ffile:///etc/passwd"###,
            r###"0	=file:///etc/passwd"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Tfile:///etc/passwd"###,
            r###"0	=file:///etc/passwd"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R//host/a//b c"###,
            r###"0	=http://host/a//b%20c"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F//host/a//b c"###,
            r###"0	=//host/a//b c"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T//host/a//b c"###,
            r###"0	=http://host/a//b c"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R."###,
            r###"0	=http://example.com/a/b/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F."###,
            r###"0	=http://example.com/a/b/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T."###,
            r###"0	=http://example.com/a/b/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R.."###,
            r###"0	=http://example.com/a/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F.."###,
            r###"0	=http://example.com/a/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T.."###,
            r###"0	=http://example.com/a/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R./?x"###,
            r###"0	=http://example.com/a/b/?x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F./?x"###,
            r###"0	=http://example.com/a/b/?x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T./?x"###,
            r###"0	=http://example.com/a/b/?x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/	Rc d/e"###,
            r###"0	=http://example.com/a/b/c%20d/e"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/	Fc d/e"###,
            r###"0	=http://example.com/a/b/c%20d/e"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/	Tc d/e"###,
            r###"0	=http://example.com/a/b/c%20d/e"###,
        ),
        (
            r###"S0:520:http://example.com	Rx"###,
            r###"0	=http://example.com/x"###,
        ),
        (
            r###"S0:520:http://example.com	Fx"###,
            r###"0	=http://example.com/x"###,
        ),
        (
            r###"S0:520:http://example.com	Tx"###,
            r###"0	=http://example.com/x"###,
        ),
        (
            r###"S0:520:http://example.com/a?b	R#c"###,
            r###"0	=http://example.com/a?b#c"###,
        ),
        (
            r###"S0:520:http://example.com/a?b	F#c"###,
            r###"0	=http://example.com/a?b#c"###,
        ),
        (
            r###"S0:520:http://example.com/a?b	T#c"###,
            r###"0	=http://example.com/a?b#c"###,
        ),
        (
            r###"S0:520:http://example.com/a#b	R#c"###,
            r###"0	=http://example.com/a#c"###,
        ),
        (
            r###"S0:520:http://example.com/a#b	F#c"###,
            r###"0	=http://example.com/a#c"###,
        ),
        (
            r###"S0:520:http://example.com/a#b	T#c"###,
            r###"0	=http://example.com/a#c"###,
        ),
        (
            r###"S0:520:http://example.com/a#b	R?c"###,
            r###"0	=http://example.com/a?c"###,
        ),
        (
            r###"S0:520:http://example.com/a#b	F?c"###,
            r###"0	=http://example.com/a?c"###,
        ),
        (
            r###"S0:520:http://example.com/a#b	T?c"###,
            r###"0	=http://example.com/a?c"###,
        ),
        (
            r###"S0:520:example.com/a	Rb"###,
            r###"0	=http://example.com/b"###,
        ),
        (
            r###"S0:520:example.com/a	Fb"###,
            r###"0	=http://example.com/b"###,
        ),
        (
            r###"S0:520:example.com/a	Tb"###,
            r###"0	=http://example.com/b"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R\\x"###,
            r###"0	=http://example.com/a/b/\\x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F\\x"###,
            r###"0	=http://example.com/a/b/\\x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T\\x"###,
            r###"0	=http://example.com/a/b/\\x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttp:\\\\x"###,
            r###"0	E28"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttp:\\\\x"###,
            r###"0	=http:\\\\x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttp:\\\\x"###,
            r###"0	E28"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	R%zz"###,
            r###"0	=http://example.com/a/b/%zz"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	F%zz"###,
            r###"0	=http://example.com/a/b/%zz"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	T%zz"###,
            r###"0	=http://example.com/a/b/%zz"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttps:x"###,
            r###"0	E28"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttps:x"###,
            r###"0	=https:x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttps:x"###,
            r###"0	E28"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttps:/x"###,
            r###"0	=https://x/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttps:/x"###,
            r###"0	=https://x/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttps:/x"###,
            r###"0	=https://x/"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttps:////x"###,
            r###"0	E28"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttps:////x"###,
            r###"0	=https:////x"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttps:////x"###,
            r###"0	E28"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Rhttps://x:/y"###,
            r###"0	=https://x/y"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Fhttps://x:/y"###,
            r###"0	=https://x/y"###,
        ),
        (
            r###"S0:520:http://example.com/a/b/c	Thttps://x:/y"###,
            r###"0	=https://x/y"###,
        ),
        (r###"X"###, r###"="###),
        (r###"Xabc"###, r###"=abc"###),
        (
            r###"Xa b+c/d?e=f&g~h_i-j.k"###,
            r###"=a%20b%2Bc%2Fd%3Fe%3Df%26g~h_i-j.k"###,
        ),
        (r###"X\xc3\xa4\x00\xff"###, r###"=%C3%A4%00%FF"###),
        (r###"X%"###, r###"=%25"###),
        (r###"U"###, r###"="###),
        (r###"Uabc"###, r###"=abc"###),
        (r###"Ua%20b"###, r###"=a b"###),
        (r###"U%41%4a%4A%zz%4"###, r###"=AJJ%zz%4"###),
        (r###"U%"###, r###"=%"###),
        (r###"U%00x"###, r###"=\x00x"###),
        (r###"U+"###, r###"=+"###),
        (
            r###"P-|0|0|http://localhost:8080"###,
            r###"OK|http://localhost:8080/"###,
        ),
        (r###"P-|0|0|localhost/x"###, r###"OK|http://localhost/x"###),
        (
            r###"P-|0|0|localhost:8080/get.inc?test=file"###,
            r###"OK|http://localhost:8080/get.inc?test=file"###,
        ),
        (
            r###"P-|0|0|ftp.example.com/x"###,
            r###"OK|ftp://ftp.example.com/x"###,
        ),
        (
            r###"P-|0|0|foo://bar"###,
            r###"C1|Protocol "foo" not supported|"###,
        ),
        (
            r###"P-|0|0|"###,
            r###"C3|URL rejected: Malformed input to a URL function|"###,
        ),
        (
            r###"P-|0|0|http://"###,
            r###"C3|URL rejected: No host part in the URL|"###,
        ),
        (
            r###"P-|0|0|http://a:99999/"###,
            r###"C3|URL rejected: Port number was not a decimal number between 0 and 65535|"###,
        ),
        (
            r###"P-|0|0|http://a b/"###,
            r###"C3|URL rejected: Malformed input to a URL function|"###,
        ),
        (
            r###"P-|0|0|http://[::1/"###,
            r###"C3|URL rejected: Bad IPv6 address|"###,
        ),
        (r###"P-|0|0|http://u:p@h/"###, r###"OK|http://u:p@h/"###),
        (
            r###"P-|0|1|http://u:p@h/"###,
            r###"C67|URL rejected: Credentials was passed in the URL when prohibited|"###,
        ),
        (r###"P-|0|0|http://h/a/../b"###, r###"OK|http://h/b"###),
        (r###"P-|1|0|http://h/a/../b"###, r###"OK|http://h/a/../b"###),
        (
            r###"P-|0|0|file:///tmp/nonexistent-urlapi-test"###,
            r###"OK|file:///tmp/nonexistent-urlapi-test"###,
        ),
        (
            r###"P-|0|0|file://otherhost/x"###,
            r###"C3|URL rejected: Bad file:// URL|"###,
        ),
        (
            r###"P-|0|0|file:///c:/x"###,
            r###"C3|URL rejected: Bad file:// URL|"###,
        ),
        (
            r###"P-|0|0|ldap://x/"###,
            r###"C1|Protocol "ldap" not supported|"###,
        ),
        (
            r###"P-|0|0|scp://x/"###,
            r###"C1|Protocol "scp" not supported|"###,
        ),
        (r###"P-|0|0|gopher://x/"###, r###"OK|gopher://x/"###),
        (
            r###"Phttps|0|0|example.com/x"###,
            r###"OK|https://example.com/x"###,
        ),
        (
            r###"Pftp|0|0|example.com/x"###,
            r###"OK|ftp://example.com/x"###,
        ),
        (
            r###"Pfoo|0|0|example.com/x"###,
            r###"C1|Protocol "foo" not supported|"###,
        ),
        (
            r###"Phttps|0|0|http://example.com/x"###,
            r###"OK|http://example.com/x"###,
        ),
        (
            r###"P-|0|0|http://h/a b"###,
            r###"C3|URL rejected: Malformed input to a URL function|"###,
        ),
        (
            r###"P-|0|0|http://h/\xc3\xa4"###,
            r###"OK|http://h/\xc3\xa4"###,
        ),
        (
            r###"P-|0|0|https://127.1:443/"###,
            r###"OK|https://127.0.0.1:443/"###,
        ),
        (r###"P-|0|0|http://h:/x"###, r###"OK|http://h/x"###),
        (
            r###"P-|0|0|h:/x"###,
            r###"C1|Protocol "h" not supported|"###,
        ),
        (
            r###"P-|0|0|\x01"###,
            r###"C3|URL rejected: Malformed input to a URL function|"###,
        ),
        (r###"P-|0|0|http:/x"###, r###"OK|http://x/"###),
        (
            r###"P-|0|0|http:////x"###,
            r###"C3|URL rejected: Unsupported number of slashes following scheme|"###,
        ),
        (
            r###"P-|0|0|data:text/plain,x"###,
            r###"C3|URL rejected: Port number was not a decimal number between 0 and 65535|"###,
        ),
        (r###"P-|0|0|HTTP://H/"###, r###"OK|http://H/"###),
        (r###"P-|0|0|http://h?q"###, r###"OK|http://h/?q"###),
        (r###"P-|0|0|http://h#f"###, r###"OK|http://h/#f"###),
        (
            r###"P-|0|0|imap://u;AUTH=%41:p@h/"###,
            r###"OK|imap://u:p;AUTH=%41@h/"###,
        ),
        (r###"P-|0|0|ftp://u%0a:p@h/"###, r###"C3||"###),
        (r###"P-|0|0|http://u%0a:p%00@h/"###, r###"C3||"###),
        (
            r###"P-|0|0|http://[fe80::1%25eth0]:8080/"###,
            r###"OK|http://[fe80::1%25eth0]:8080/"###,
        ),
        (
            r###"P-|0|0|https://[::ffff:1.2.3.4]/x"###,
            r###"OK|https://[::ffff:1.2.3.4]/x"###,
        ),
        (
            r###"S0:4:example.com/a	S0:4:b	G0:0"###,
            r###"0	0	=https://example.com/b"###,
        ),
        (
            r###"S0:512:example.com/a/b	S0:512:../c?d	G0:0	G0:32768"###,
            r###"0	0	=http://example.com/c?d	=http://example.com/c?d"###,
        ),
        (
            r###"S0:512:example.com/a/b	S0:512:other.com/c	G0:0"###,
            r###"0	0	=http://example.com/a/other.com/c"###,
        ),
        (
            r###"S0:0:http://h/a?b#c	S0:0:?	G0:0	G0:16384"###,
            r###"0	0	=http://h/a	=http://h/a?"###,
        ),
        (
            r###"S0:0:http://h/a?b#c	S0:0:#	G0:0	G0:16384"###,
            r###"0	0	=http://h/a?b	=http://h/a?b#"###,
        ),
        (
            r###"S0:0:http://h/a	S0:2176://x/ y z	G0:0"###,
            r###"0	0	=http://x/%20y%20z"###,
        ),
        (
            r###"S0:0:http://h/a	S0:2048://x y/z	G0:0"###,
            r###"0	E21	=http://h/a"###,
        ),
        (
            r###"S0:0:https://h/a	S0:0://x:443/	G0:0	G0:2"###,
            r###"0	0	=https://x:443/	=https://x/"###,
        ),
        (
            r###"S0:0:http://h/	S5:0:%5B::1%5D	G0:0"###,
            r###"0	0	=http://%5b::1%5d/"###,
        ),
        (
            r###"S0:0:http://%5B::1%5D/	G0:0	G5:0"###,
            r###"E4	E14	E14"###,
        ),
        (r###"S0:0:http://[::1]%41/	G0:0"###, r###"E4	E14"###),
        (
            r###"S0:0:http://h/	S10:0:x	G0:0"###,
            r###"0	0	=http://h/"###,
        ),
        (
            r###"S0:0:http://[::1]/	S10:0:x%20y	G0:0	G10:64"###,
            r###"0	0	=http://[::1%25x%20y]/	=x y"###,
        ),
        (
            r###"S0:0:http://h/	S1:8:FOO	G0:0	G6:1	G0:1"###,
            r###"0	0	=FOO://h/	E15	=FOO://h/"###,
        ),
        (
            r###"S0:0:http://u:p@h/	S2:0:a@b	G0:0"###,
            r###"0	0	=http://a@b:p@h/"###,
        ),
        (
            r###"S0:0:smtp://u;opt@h/	G4:0	G4:64	G0:0	S1:0:http	G0:0"###,
            r###"0	=opt	=opt	=smtp://u;opt@h/	0	=http://u@h/"###,
        ),
        (
            r###"S0:0:http://h/	S9:0:a%2Fb%2fc	G9:0	G9:64"###,
            r###"0	0	=a%2fb%2fc	=a/b/c"###,
        ),
        (
            r###"S0:0:http://h/	S3:0:	G0:0"###,
            r###"0	0	=http://:@h/"###,
        ),
        (
            r###"S0:0:http://h/a/b/%2E%2E/%2e/c	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/a/c	=http	E11	E12	E13	=h	E15	=/a/c	E16	E17	E18	=http://h:80/a/c	=http://h/a/c	=http://h/a/c	=80	=http://h/a/c	=/a/c"###,
        ),
        (
            r###"S0:0:http://h/%2e	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/	=http	E11	E12	E13	=h	E15	=/	E16	E17	E18	=http://h:80/	=http://h/	=http://h/	=80	=http://h/	=/"###,
        ),
        (
            r###"S0:0:http://h/.%2e	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/	=http	E11	E12	E13	=h	E15	=/	E16	E17	E18	=http://h:80/	=http://h/	=http://h/	=80	=http://h/	=/"###,
        ),
        (
            r###"S0:0:http://h/..	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/	=http	E11	E12	E13	=h	E15	=/	E16	E17	E18	=http://h:80/	=http://h/	=http://h/	=80	=http://h/	=/"###,
        ),
        (
            r###"S0:0:http://h/...	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/...	=http	E11	E12	E13	=h	E15	=/...	E16	E17	E18	=http://h:80/...	=http://h/...	=http://h/...	=80	=http://h/...	=/..."###,
        ),
        (
            r###"S0:0:http://h/a/.../b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/a/.../b	=http	E11	E12	E13	=h	E15	=/a/.../b	E16	E17	E18	=http://h:80/a/.../b	=http://h/a/.../b	=http://h/a/.../b	=80	=http://h/a/.../b	=/a/.../b"###,
        ),
        (
            r###"S0:0:http://h/a/..b/c	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/a/..b/c	=http	E11	E12	E13	=h	E15	=/a/..b/c	E16	E17	E18	=http://h:80/a/..b/c	=http://h/a/..b/c	=http://h/a/..b/c	=80	=http://h/a/..b/c	=/a/..b/c"###,
        ),
        (
            r###"S0:0:http://h?a=b/../c	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/?a=b/../c	=http	E11	E12	E13	=h	E15	=/	=a=b/../c	E17	E18	=http://h:80/?a=b/../c	=http://h/?a=b/../c	=http://h/?a=b/../c	=80	=http://h/?a=b/../c	=/"###,
        ),
        (
            r###"S0:0:http://h#a/../b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/#a/../b	=http	E11	E12	E13	=h	E15	=/	E16	=a/../b	E18	=http://h:80/#a/../b	=http://h/#a/../b	=http://h/#a/../b	=80	=http://h/#a/../b	=/"###,
        ),
        (
            r###"S0:0:http://h/a\x20b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:2048:http://us er:pa ss@h/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://us er:pa ss@h/	=http	=us er	=pa ss	E13	=h	E15	=/	E16	E17	E18	=http://us er:pa ss@h:80/	=http://us er:pa ss@h/	=http://us er:pa ss@h/	=80	=http://us er:pa ss@h/	=/"###,
        ),
        (
            r###"S0:0:http://0/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://0.0.0.0/	=http	E11	E12	E13	=0.0.0.0	E15	=/	E16	E17	E18	=http://0.0.0.0:80/	=http://0.0.0.0/	=http://0.0.0.0/	=80	=http://0.0.0.0/	=/"###,
        ),
        (
            r###"S0:0:http://0.0/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://0.0.0.0/	=http	E11	E12	E13	=0.0.0.0	E15	=/	E16	E17	E18	=http://0.0.0.0:80/	=http://0.0.0.0/	=http://0.0.0.0/	=80	=http://0.0.0.0/	=/"###,
        ),
        (
            r###"S0:0:http://00000000000000000000001/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://0.0.0.1/	=http	E11	E12	E13	=0.0.0.1	E15	=/	E16	E17	E18	=http://0.0.0.1:80/	=http://0.0.0.1/	=http://0.0.0.1/	=80	=http://0.0.0.1/	=/"###,
        ),
        (
            r###"S0:0:http://0x0000000000000000000000ff/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://0.0.0.255/	=http	E11	E12	E13	=0.0.0.255	E15	=/	E16	E17	E18	=http://0.0.0.255:80/	=http://0.0.0.255/	=http://0.0.0.255/	=80	=http://0.0.0.255/	=/"###,
        ),
        (
            r###"S0:0:http://1.0x2.03.4/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.2.3.4/	=http	E11	E12	E13	=1.2.3.4	E15	=/	E16	E17	E18	=http://1.2.3.4:80/	=http://1.2.3.4/	=http://1.2.3.4/	=80	=http://1.2.3.4/	=/"###,
        ),
        (
            r###"S0:0:http://1.2.3.4./	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1.2.3.4./	=http	E11	E12	E13	=1.2.3.4.	E15	=/	E16	E17	E18	=http://1.2.3.4.:80/	=http://1.2.3.4./	=http://1.2.3.4./	=80	=http://1.2.3.4./	=/"###,
        ),
        (
            r###"S0:0:http://.1.2.3/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://.1.2.3/	=http	E11	E12	E13	=.1.2.3	E15	=/	E16	E17	E18	=http://.1.2.3:80/	=http://.1.2.3/	=http://.1.2.3/	=80	=http://.1.2.3/	=/"###,
        ),
        (
            r###"S0:0:http://1..2/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://1..2/	=http	E11	E12	E13	=1..2	E15	=/	E16	E17	E18	=http://1..2:80/	=http://1..2/	=http://1..2/	=80	=http://1..2/	=/"###,
        ),
        (
            r###"S0:0:http://example.com./	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://example.com./	=http	E11	E12	E13	=example.com.	E15	=/	E16	E17	E18	=http://example.com.:80/	=http://example.com./	=http://example.com./	=80	=http://example.com./	=/"###,
        ),
        (
            r###"S0:0:http://-example-.com/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://-example-.com/	=http	E11	E12	E13	=-example-.com	E15	=/	E16	E17	E18	=http://-example-.com:80/	=http://-example-.com/	=http://-example-.com/	=80	=http://-example-.com/	=/"###,
        ),
        (
            r###"S0:0:http://[::ffff:0:0]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::ffff:0:0]/	=http	E11	E12	E13	=[::ffff:0:0]	E15	=/	E16	E17	E18	=http://[::ffff:0:0]:80/	=http://[::ffff:0:0]/	=http://[::ffff:0:0]/	=80	=http://[::ffff:0:0]/	=/"###,
        ),
        (
            r###"S0:0:http://[0::0:0:0:0:0:0:0]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::1:2:3:4:5:6:7]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1:2:3:4:5:6:7]/	=http	E11	E12	E13	=[::1:2:3:4:5:6:7]	E15	=/	E16	E17	E18	=http://[::1:2:3:4:5:6:7]:80/	=http://[::1:2:3:4:5:6:7]/	=http://[::1:2:3:4:5:6:7]/	=80	=http://[::1:2:3:4:5:6:7]/	=/"###,
        ),
        (
            r###"S0:0:http://[1:0:0:2:0:0:0:3]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[1:0:0:2::3]/	=http	E11	E12	E13	=[1:0:0:2::3]	E15	=/	E16	E17	E18	=http://[1:0:0:2::3]:80/	=http://[1:0:0:2::3]/	=http://[1:0:0:2::3]/	=80	=http://[1:0:0:2::3]/	=/"###,
        ),
        (
            r###"S0:0:http://[1:0:0:0:2:0:0:3]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[1::2:0:0:3]/	=http	E11	E12	E13	=[1::2:0:0:3]	E15	=/	E16	E17	E18	=http://[1::2:0:0:3]:80/	=http://[1::2:0:0:3]/	=http://[1::2:0:0:3]/	=80	=http://[1::2:0:0:3]/	=/"###,
        ),
        (
            r###"S0:0:http://[0:0:1:0:0:0:0:0]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[0:0:1::]/	=http	E11	E12	E13	=[0:0:1::]	E15	=/	E16	E17	E18	=http://[0:0:1::]:80/	=http://[0:0:1::]/	=http://[0:0:1::]/	=80	=http://[0:0:1::]/	=/"###,
        ),
        (
            r###"S0:0:http://[A:B:C:D:E:F:a:b]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[A:B:C:D:E:F:a:b]/	=http	E11	E12	E13	=[A:B:C:D:E:F:a:b]	E15	=/	E16	E17	E18	=http://[A:B:C:D:E:F:a:b]:80/	=http://[A:B:C:D:E:F:a:b]/	=http://[A:B:C:D:E:F:a:b]/	=80	=http://[A:B:C:D:E:F:a:b]/	=/"###,
        ),
        (
            r###"S0:0:http://[::]:0/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::]:0/	=http	E11	E12	E13	=[::]	=0	=/	E16	E17	E18	=http://[::]:0/	=http://[::]:0/	=http://[::]:0/	=0	=http://[::]:0/	=/"###,
        ),
        (
            r###"S0:0:http://[:1]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[1:]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::1::]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[0000:0000:0000:0000:0000:0000:0000:0001]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1]/	=http	E11	E12	E13	=[::1]	E15	=/	E16	E17	E18	=http://[::1]:80/	=http://[::1]/	=http://[::1]/	=80	=http://[::1]/	=/"###,
        ),
        (
            r###"S0:0:http://[::ffff:1.2.3]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::ffff:1.2.3.4.5]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::ffff:256.2.3.4]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[::ffff:1.2.3.4:5]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:http://[1.2.3.4]/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E22	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:8:x-y+z.w://h/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=x-y+z.w://h/	=x-y+z.w	E11	E12	E13	=h	E15	=/	E16	E17	E18	=x-y+z.w://h/	=x-y+z.w://h/	=x-y+z.w://h/	E15	=x-y+z.w://h/	=/"###,
        ),
        (
            r###"S0:8:git+ssh://h/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=git+ssh://h/	=git+ssh	E11	E12	E13	=h	E15	=/	E16	E17	E18	=git+ssh://h/	=git+ssh://h/	=git+ssh://h/	E15	=git+ssh://h/	=/"###,
        ),
        (
            r###"S0:0:http:h	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E28	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:http:h	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:http:/h	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h/	=http	E11	E12	E13	=h	E15	=/	E16	E17	E18	=http://h:80/	=http://h/	=http://h/	=80	=http://h/	=/"###,
        ),
        (
            r###"S0:0:https:////h	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E28	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:h:80	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h:80/	=http	E11	E12	E13	=h	=80	=/	E16	E17	E18	=http://h:80/	=http://h/	=http://h:80/	=80	=http://h:80/	=/"###,
        ),
        (
            r###"S0:512:h:0080/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://h:80/x	=http	E11	E12	E13	=h	=80	=/x	E16	E17	E18	=http://h:80/x	=http://h/x	=http://h:80/x	=80	=http://h:80/x	=/x"###,
        ),
        (
            r###"S0:512:h:99999	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:h:x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:[::1]	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://[::1]/	=http	E11	E12	E13	=[::1]	E15	=/	E16	E17	E18	=http://[::1]:80/	=http://[::1]/	=http://[::1]/	=80	=http://[::1]/	=/"###,
        ),
        (
            r###"S0:512:[::1]x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E4	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:user@h	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=http://user@h/	=http	=user	E12	E13	=h	E15	=/	E16	E17	E18	=http://user@h:80/	=http://user@h/	=http://user@h/	=80	=http://user@h/	=/"###,
        ),
        (
            r###"S0:544:user@h	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E8	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:512:imap.x/;	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=imap://imap.x/;	=imap	E11	E12	E13	=imap.x	E15	=/;	E16	E17	E18	=imap://imap.x:143/;	=imap://imap.x/;	=imap://imap.x/;	=143	=imap://imap.x/;	=/;"###,
        ),
        (
            r###"S0:0:telnet://u:p@h:23	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=telnet://u:p@h:23/	=telnet	=u	=p	E13	=h	=23	=/	E16	E17	E18	=telnet://u:p@h:23/	=telnet://u:p@h/	=telnet://u:p@h:23/	=23	=telnet://u:p@h:23/	=/"###,
        ),
        (
            r###"S0:0:file://localhost	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file://localhost?x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file://127.0.0.1/	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://(nil)	=file	E11	E12	E13	E14	E15	=/	E16	E17	E18	=file://(nil)	=file://(nil)	=file://(nil)	=0	=file://(nil)	=/"###,
        ),
        (
            r###"S0:0:file:///%2e%2e/x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///x	=file	E11	E12	E13	E14	E15	=/x	E16	E17	E18	=file:///x	=file:///x	=file:///x	=0	=file:///x	=/x"###,
        ),
        (
            r###"S0:0:file:////x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:////x	=file	E11	E12	E13	E14	E15	=//x	E16	E17	E18	=file:////x	=file:////x	=file:////x	=0	=file:////x	=//x"###,
        ),
        (
            r###"S0:0:file://///x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://///x	=file	E11	E12	E13	E14	E15	=///x	E16	E17	E18	=file://///x	=file://///x	=file://///x	=0	=file://///x	=///x"###,
        ),
        (
            r###"S0:0:file:///a|/b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:///a|	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:///ab:/c	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file:///ab:/c	=file	E11	E12	E13	E14	E15	=/ab:/c	E16	E17	E18	=file:///ab:/c	=file:///ab:/c	=file:///ab:/c	=0	=file:///ab:/c	=/ab:/c"###,
        ),
        (
            r###"S0:0:file:a:	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E19	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
        (
            r###"S0:0:file:a|x	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"0	=file://a|x	=file	E11	E12	E13	E14	E15	=a|x	E16	E17	E18	=file://a|x	=file://a|x	=file://a|x	=0	=file://a|x	=a|x"###,
        ),
        (
            r###"S0:128:file:///a b	G0:0	G1:0	G2:0	G3:0	G4:0	G5:0	G6:0	G7:0	G8:0	G9:0	G10:0	G0:1	G0:2	G0:128	G6:1	G0:16384	G7:128"###,
            r###"E3	E14	E10	E11	E12	E13	E14	E15	=/	E16	E17	E18	E14	E14	E14	E15	E14	=/"###,
        ),
    ];
    // END GENERATED CASES
}
