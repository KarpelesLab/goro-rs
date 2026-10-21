//! Byte-exact port of libcurl 8.14.1's cookie engine (`lib/cookie.c`, built
//! without libpsl) together with the date parser it relies on
//! (`lib/parsedate.c`: `curl_getdate()` / `Curl_getdate_capped()`).
//!
//! All strings are handled as raw bytes. Wherever libcurl works on
//! NUL-terminated C strings, the input slices are cut at their first NUL byte
//! so that behaviour matches (an embedded NUL ends the string).
//!
//! `infof()` texts are pushed, without the `* ` prefix and without the newline
//! that libcurl's trace code appends, into an [`InfoLog`] supplied by the
//! caller. Note that a few of libcurl's messages embed the rest of the header
//! line (including its CR LF) — this is reproduced as is.

use std::cmp::Ordering;
use std::io::{Read, Write};

/// Collector for the `infof()` messages emitted by the cookie engine.
pub type InfoLog = Vec<Vec<u8>>;

/// Number of hash buckets in a cookie jar.
pub const COOKIE_HASH_SIZE: usize = 63;
/// Longest accepted cookie header line / jar file line.
pub const MAX_COOKIE_LINE: usize = 5000;
/// Maximum length of an incoming cookie name or content.
pub const MAX_NAME: usize = 4096;
/// Maximum number of Set-Cookie: lines accepted per request.
pub const MAX_SET_COOKIE_AMOUNT: u8 = 50;
/// Maximum size of an outgoing Cookie: header.
pub const MAX_COOKIE_HEADER_LEN: usize = 8190;
/// Maximum number of cookies sent in one request.
pub const MAX_COOKIE_SEND_AMOUNT: usize = 150;
/// Longest accepted Expires attribute value (exclusive).
const MAX_DATE_LENGTH: usize = 80;
/// Seconds in 400 days: no cookie expires further away than this.
const COOKIES_MAXAGE: i64 = 400 * 24 * 3600;

/// `CURLE_WRITE_ERROR`, the error `cookie_output()` reports.
pub const CURLE_WRITE_ERROR: u32 = 23;

const CURL_OFF_T_MAX: i64 = i64::MAX;

/// One cookie (struct Cookie).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cookie {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    /// `path = <this>` as given in Set-Cookie:, or the default path.
    pub path: Option<Vec<u8>>,
    /// Sanitized cookie path.
    pub spath: Option<Vec<u8>>,
    pub domain: Option<Vec<u8>>,
    /// 0 means a session cookie.
    pub expires: i64,
    pub creationtime: u32,
    pub tailmatch: bool,
    pub secure: bool,
    /// Set from a server (or while running), not read from a stored file.
    pub livecookie: bool,
    pub httponly: bool,
    pub prefix_secure: bool,
    pub prefix_host: bool,
}

/// Why a cookie was not added (CERR_* in cookie.c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cerr {
    TooLong,
    Tab,
    TooBig,
    Bad,
    NoSep,
    InvalidOctet,
    BadSecure,
    NoTailmatch,
    Comment,
    Range,
    Fields,
    LiveWins,
}

/// The cookie jar (struct CookieInfo).
#[derive(Clone, Debug)]
pub struct CookieInfo {
    cookielist: Vec<Vec<Cookie>>,
    /// The next time at which expiration happens.
    pub next_expiration: i64,
    /// Number of cookies in the jar.
    pub numcookies: u32,
    /// Last creation-time used in the jar.
    pub lastct: u32,
    /// State info for cookie adding: false while loading a file.
    pub running: bool,
    /// New session: discard session cookies on load.
    pub newsession: bool,
    /// When set, used instead of the real clock (for deterministic tests).
    pub fixed_now: Option<i64>,
}

impl Default for CookieInfo {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// C string helpers
// ---------------------------------------------------------------------------

/// Cut a byte slice at its first NUL, like a C string.
fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

#[inline]
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

#[inline]
fn raw_toupper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}

/// curl_strnequal(): case-insensitive compare of at most `max` bytes of two
/// NUL-terminated strings (the slices end where the C string would).
fn ncasecompare(first: &[u8], second: &[u8], max: usize) -> bool {
    let mut i = 0;
    let mut max = max;
    while at(first, i) != 0 && max > 0 {
        if raw_toupper(at(first, i)) != raw_toupper(at(second, i)) {
            return false;
        }
        max -= 1;
        i += 1;
    }
    if max == 0 {
        return true;
    }
    raw_toupper(at(first, i)) == raw_toupper(at(second, i))
}

/// curl_strequal(): full case-insensitive equality.
fn strcasecompare(a: &[u8], b: &[u8]) -> bool {
    let a = cstr(a);
    let b = cstr(b);
    a.len() == b.len() && a.eq_ignore_ascii_case(b)
}

/// strncmp() returning only whether the strings are equal in the first n.
fn strn_equal(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let ca = at(a, i);
        let cb = at(b, i);
        if ca != cb {
            return false;
        }
        if ca == 0 {
            return true;
        }
    }
    true
}

fn strcspn(s: &[u8], reject: &[u8]) -> usize {
    s.iter()
        .position(|&b| b == 0 || reject.contains(&b))
        .unwrap_or(s.len())
}

#[inline]
fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// curlx_str_number() / str_num_base(10): parse an unsigned decimal number
/// at s[*pos..]. On success advances *pos.
#[derive(Debug, PartialEq, Eq)]
enum NumErr {
    NoNum,
    Overflow,
}

fn str_number(s: &[u8], pos: &mut usize, max: i64) -> Result<i64, NumErr> {
    let mut p = *pos;
    if !at(s, p).is_ascii_digit() {
        return Err(NumErr::NoNum);
    }
    let mut num: i64 = 0;
    if max < 10 {
        loop {
            let n = (at(s, p) - b'0') as i64;
            p += 1;
            num = num * 10 + n;
            if num > max {
                return Err(NumErr::Overflow);
            }
            if !at(s, p).is_ascii_digit() {
                break;
            }
        }
    } else {
        loop {
            let n = (at(s, p) - b'0') as i64;
            p += 1;
            if num > (max - n) / 10 {
                return Err(NumErr::Overflow);
            }
            num = num * 10 + n;
            if !at(s, p).is_ascii_digit() {
                break;
            }
        }
    }
    *pos = p;
    Ok(num)
}

fn memdup(s: &[u8]) -> Vec<u8> {
    s.to_vec()
}

fn fmt_nil(s: &Option<Vec<u8>>) -> &[u8] {
    match s {
        Some(v) => v,
        None => b"(nil)",
    }
}

fn real_now() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

// ---------------------------------------------------------------------------
// Numerical host detection (Curl_host_is_ipnum() using glibc inet_pton)
// ---------------------------------------------------------------------------

fn inet_pton4(src: &[u8]) -> bool {
    let mut saw_digit = false;
    let mut octets = 0;
    let mut cur: u32 = 0;
    for &ch in src {
        if ch.is_ascii_digit() {
            let new = cur * 10 + (ch - b'0') as u32;
            if saw_digit && cur == 0 {
                return false;
            }
            if new > 255 {
                return false;
            }
            cur = new;
            if !saw_digit {
                octets += 1;
                if octets > 4 {
                    return false;
                }
                saw_digit = true;
            }
        } else if ch == b'.' && saw_digit {
            if octets == 4 {
                return false;
            }
            cur = 0;
            saw_digit = false;
        } else {
            return false;
        }
    }
    octets >= 4
}

fn inet_pton6(src: &[u8]) -> bool {
    const IN6ADDRSZ: usize = 16;
    let end = src.len();
    if end == 0 {
        return false;
    }
    let mut i = 0;
    if src[0] == b':' {
        i += 1;
        if i == end || src[i] != b':' {
            return false;
        }
    }
    let mut curtok = i;
    let mut xdigits_seen = 0;
    let mut val: u32 = 0;
    let mut tp = 0usize;
    let mut colonp: Option<usize> = None;
    while i < end {
        let ch = src[i];
        i += 1;
        if let Some(digit) = (ch as char).to_digit(16) {
            if xdigits_seen == 4 {
                return false;
            }
            val = (val << 4) | digit;
            if val > 0xffff {
                return false;
            }
            xdigits_seen += 1;
            continue;
        }
        if ch == b':' {
            curtok = i;
            if xdigits_seen == 0 {
                if colonp.is_some() {
                    return false;
                }
                colonp = Some(tp);
                continue;
            } else if i == end {
                return false;
            }
            if tp + 2 > IN6ADDRSZ {
                return false;
            }
            tp += 2;
            xdigits_seen = 0;
            val = 0;
            continue;
        }
        if ch == b'.' && tp + 4 <= IN6ADDRSZ && inet_pton4(&src[curtok..end]) {
            tp += 4;
            xdigits_seen = 0;
            break;
        }
        return false;
    }
    if xdigits_seen > 0 {
        if tp + 2 > IN6ADDRSZ {
            return false;
        }
        tp += 2;
    }
    if colonp.is_some() {
        if tp == IN6ADDRSZ {
            return false;
        }
        tp = IN6ADDRSZ;
    }
    tp == IN6ADDRSZ
}

/// Curl_host_is_ipnum(): TRUE if the (C) string is a numerical IPv4 or IPv6
/// address.
pub fn host_is_ipnum(hostname: &[u8]) -> bool {
    let h = cstr(hostname);
    inet_pton4(h) || inet_pton6(h)
}

// ---------------------------------------------------------------------------
// Small cookie.c helpers
// ---------------------------------------------------------------------------

fn cap_expires(now: i64, co: &mut Cookie) {
    if (i64::MAX - COOKIES_MAXAGE - 30) > now {
        let mut cap = now + COOKIES_MAXAGE;
        if co.expires > cap {
            cap += 30;
            co.expires = (cap / 60) * 60;
        }
    }
}

fn cookie_tailmatch(cookie_domain: &[u8], hostname: &[u8]) -> bool {
    let hostname = cstr(hostname);
    let cookie_domain_len = cookie_domain.len();
    let hostname_len = hostname.len();
    if hostname_len < cookie_domain_len {
        return false;
    }
    if !ncasecompare(
        cookie_domain,
        &hostname[hostname_len - cookie_domain_len..],
        cookie_domain_len,
    ) {
        return false;
    }
    if hostname_len == cookie_domain_len {
        return true;
    }
    hostname[hostname_len - cookie_domain_len - 1] == b'.'
}

/// RFC6265 5.1.4 path-match of a sanitized cookie path against a URI path.
fn pathmatch(cookie_path: &[u8], uri_path: &[u8]) -> bool {
    let cookie_path = cstr(cookie_path);
    let mut uri_path = cstr(uri_path);
    let cookie_path_len = cookie_path.len();
    if cookie_path_len == 1 {
        return true;
    }
    if uri_path.is_empty() || uri_path[0] != b'/' {
        uri_path = b"/";
    }
    let uri_path_len = uri_path.len();
    if uri_path_len < cookie_path_len {
        return false;
    }
    if !strn_equal(cookie_path, uri_path, cookie_path_len) {
        return false;
    }
    if cookie_path_len == uri_path_len {
        return true;
    }
    uri_path[cookie_path_len] == b'/'
}

fn get_top_domain(domain: &[u8]) -> &[u8] {
    if let Some(last) = domain.iter().rposition(|&b| b == b'.')
        && let Some(first) = domain[..last].iter().rposition(|&b| b == b'.')
    {
        return &domain[first + 1..];
    }
    domain
}

fn cookie_hash_domain(domain: &[u8]) -> usize {
    let mut h: u64 = 5381;
    for &c in domain {
        // (size_t)Curl_raw_toupper(c): a signed char sign-extends
        let j = (raw_toupper(c) as i8) as i64 as u64;
        h = h.wrapping_add(h << 5);
        h ^= j;
    }
    (h % COOKIE_HASH_SIZE as u64) as usize
}

fn cookiehash(domain: Option<&[u8]>) -> usize {
    match domain {
        None => 0,
        Some(d) => {
            let d = cstr(d);
            if host_is_ipnum(d) {
                0
            } else {
                cookie_hash_domain(get_top_domain(d))
            }
        }
    }
}

fn sanitize_cookie_path(cookie_path: &[u8]) -> Vec<u8> {
    let mut p = cstr(cookie_path);
    let mut len = p.len();
    if at(p, 0) == b'"' {
        p = &p[1..];
        len -= 1;
    }
    if len > 0 && p[len - 1] == b'"' {
        len -= 1;
    }
    if at(p, 0) != b'/' {
        return b"/".to_vec();
    }
    if len > 0 && p[len - 1] == b'/' {
        len -= 1;
    }
    p[..len].to_vec()
}

/// Make sure domain contains a dot or is localhost (no-libpsl variant).
fn bad_domain(domain: &[u8]) -> bool {
    if domain.len() == 9 && ncasecompare(domain, b"localhost", 9) {
        return false;
    }
    if let Some(i) = domain.iter().position(|&b| b == b'.')
        && domain.len() - i > 1
    {
        return false;
    }
    true
}

fn invalid_octets(p: &[u8]) -> bool {
    cstr(p).iter().any(|&c| (c != 9 && c < 0x20) || c == 0x7f)
}

fn get_netscape_format(co: &Cookie) -> Vec<u8> {
    let mut out = Vec::new();
    if co.httponly {
        out.extend_from_slice(b"#HttpOnly_");
    }
    if co.tailmatch
        && let Some(d) = &co.domain
        && at(d, 0) != b'.'
    {
        out.push(b'.');
    }
    match &co.domain {
        Some(d) => out.extend_from_slice(d),
        None => out.extend_from_slice(b"unknown"),
    }
    out.push(b'\t');
    out.extend_from_slice(if co.tailmatch { b"TRUE" } else { b"FALSE" });
    out.push(b'\t');
    match &co.path {
        Some(p) => out.extend_from_slice(p),
        None => out.push(b'/'),
    }
    out.push(b'\t');
    out.extend_from_slice(if co.secure { b"TRUE" } else { b"FALSE" });
    out.push(b'\t');
    out.extend_from_slice(co.expires.to_string().as_bytes());
    out.push(b'\t');
    out.extend_from_slice(&co.name);
    out.push(b'\t');
    out.extend_from_slice(&co.value);
    out
}

fn cookie_sort(c1: &Cookie, c2: &Cookie) -> Ordering {
    let len = |o: &Option<Vec<u8>>| o.as_ref().map_or(0, |v| v.len());
    let (l1, l2) = (len(&c1.path), len(&c2.path));
    if l1 != l2 {
        return l2.cmp(&l1);
    }
    let (l1, l2) = (len(&c1.domain), len(&c2.domain));
    if l1 != l2 {
        return l2.cmp(&l1);
    }
    let (l1, l2) = (c1.name.len(), c2.name.len());
    if l1 != l2 {
        return l2.cmp(&l1);
    }
    c2.creationtime.cmp(&c1.creationtime)
}

/// Emulates Curl_get_line() (fgets() with a 128-byte buffer into a dynbuf
/// capped at MAX_COOKIE_LINE) over an in-memory file. Returns false at the
/// end or on error (which ends the whole file).
fn get_line(input: &[u8], pos: &mut usize, buf: &mut Vec<u8>) -> bool {
    buf.clear();
    loop {
        if *pos >= input.len() {
            // fgets() returns NULL
            return !buf.is_empty();
        }
        let start = *pos;
        let mut end = start;
        while end < input.len() && end - start < 127 {
            let c = input[end];
            end += 1;
            if c == b'\n' {
                break;
            }
        }
        *pos = end;
        let chunk = &input[start..end];
        let eof = chunk.last() != Some(&b'\n') && chunk.len() < 127;
        let rlen = cstr(chunk).len();
        if rlen == 0 {
            return false;
        }
        if buf.len() + rlen + 1 > MAX_COOKIE_LINE {
            return false;
        }
        buf.extend_from_slice(&chunk[..rlen]);
        if chunk[rlen - 1] == b'\n' {
            return true;
        } else if eof {
            if buf.len() + 2 > MAX_COOKIE_LINE {
                return false;
            }
            buf.push(b'\n');
            return true;
        }
    }
}

#[cfg(unix)]
fn bytes_to_path(b: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b))
}

#[cfg(not(unix))]
fn bytes_to_path(b: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(String::from_utf8_lossy(b).into_owned())
}

// ---------------------------------------------------------------------------
// The jar
// ---------------------------------------------------------------------------

impl CookieInfo {
    /// A fresh, empty jar (the calloc() part of Curl_cookie_init()). Note that
    /// `running` is false until [`CookieInfo::init`] is called; use
    /// [`CookieInfo::new_initialized`] for the equivalent of
    /// `Curl_cookie_init(data, NULL, NULL, newsession)`.
    pub fn new() -> Self {
        CookieInfo {
            cookielist: vec![Vec::new(); COOKIE_HASH_SIZE],
            next_expiration: CURL_OFF_T_MAX,
            numcookies: 0,
            lastct: 0,
            running: false,
            newsession: false,
            fixed_now: None,
        }
    }

    /// `Curl_cookie_init(data or NULL, NULL, NULL, newsession)`: an empty,
    /// running jar. Used by the share handle (`newsession` TRUE there) and by
    /// COOKIELIST when no engine exists yet.
    pub fn new_initialized(newsession: bool) -> Self {
        let mut ci = Self::new();
        ci.newsession = newsession;
        ci.running = true;
        ci
    }

    /// Current time as the engine sees it.
    pub fn now(&self) -> i64 {
        self.fixed_now.unwrap_or_else(real_now)
    }

    /// Iterate over all cookies in internal (hash bucket, insertion) order.
    pub fn cookies(&self) -> impl Iterator<Item = &Cookie> {
        self.cookielist.iter().flatten()
    }

    /// Curl_cookie_init() with a non-NULL `data`, on an existing jar: set
    /// `newsession`, then read `file` (when given and not empty; "-" reads
    /// stdin), then mark the jar as running. The caller must set its
    /// `state.cookie_engine` flag to TRUE after this call.
    pub fn init(
        &mut self,
        file: Option<&[u8]>,
        newsession: bool,
        setcookies: &mut u8,
        log: &mut InfoLog,
    ) {
        self.newsession = newsession;
        let mut content: Option<Vec<u8>> = None;
        if let Some(file) = file.map(cstr)
            && !file.is_empty()
        {
            if file == b"-" {
                let mut v = Vec::new();
                let _ = std::io::stdin().lock().read_to_end(&mut v);
                content = Some(v);
            } else {
                match std::fs::File::open(bytes_to_path(file)) {
                    Ok(mut f) => {
                        let mut v = Vec::new();
                        // a read error (e.g. a directory) reads as EOF
                        if f.read_to_end(&mut v).is_err() {
                            v.clear();
                        }
                        content = Some(v);
                    }
                    Err(_) => {
                        let mut m = b"WARNING: failed to open cookie file \"".to_vec();
                        m.extend_from_slice(file);
                        m.push(b'"');
                        log.push(m);
                    }
                }
            }
        }
        self.running = false;
        if let Some(content) = content {
            self.load_bytes(&content, setcookies, log);
        }
        self.running = true;
    }

    /// The file-reading part of Curl_cookie_init(), on in-memory contents.
    /// Must be called with `running == false` to get the libcurl semantics of
    /// reading a cookie file. Ends with remove_expired().
    pub fn load_bytes(&mut self, content: &[u8], setcookies: &mut u8, log: &mut InfoLog) {
        let mut pos = 0;
        let mut buf = Vec::new();
        while get_line(content, &mut pos, &mut buf) {
            let mut lineptr: &[u8] = &buf;
            let mut headerline = false;
            if ncasecompare(lineptr, b"Set-Cookie:", 11) {
                lineptr = &lineptr[11..];
                headerline = true;
                while lineptr.first().is_some_and(|&c| is_blank(c)) {
                    lineptr = &lineptr[1..];
                }
            }
            let line = lineptr.to_vec();
            self.add(setcookies, headerline, true, &line, None, None, true, log);
        }
        self.remove_expired();
    }

    /// remove_expired(): drop expired cookies (with the next_expiration
    /// shortcut libcurl uses).
    pub fn remove_expired(&mut self) {
        let now = self.now();
        if now < self.next_expiration && self.next_expiration != CURL_OFF_T_MAX {
            return;
        }
        self.next_expiration = CURL_OFF_T_MAX;
        let mut next = CURL_OFF_T_MAX;
        let mut removed = 0u32;
        for bucket in self.cookielist.iter_mut() {
            bucket.retain(|co| {
                if co.expires != 0 && co.expires < now {
                    removed += 1;
                    false
                } else {
                    if co.expires != 0 && co.expires < next {
                        next = co.expires;
                    }
                    true
                }
            });
        }
        self.next_expiration = next;
        self.numcookies = self.numcookies.wrapping_sub(removed);
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_cookie_header(
        &self,
        co: &mut Cookie,
        line: &[u8],
        domain_in: Option<&[u8]>,
        path: Option<&[u8]>,
        secure: bool,
        setcookies: &mut u8,
        log: &mut InfoLog,
    ) -> Result<(), Cerr> {
        let line = cstr(line);
        if line.len() > MAX_COOKIE_LINE {
            return Err(Cerr::TooLong);
        }
        let mut domain: Option<&[u8]> = domain_in.map(cstr);
        let now = self.now();
        let mut p = 0usize;
        let mut have_name = false;
        loop {
            let nlen = strcspn(&line[p..], b";\t\r\n=");
            if nlen > 0 {
                let mut ns = p;
                let mut ne = p + nlen;
                p = ne;
                let mut done = false;
                let mut sep = false;
                while ns < ne && is_blank(line[ns]) {
                    ns += 1;
                }
                while ns < ne && is_blank(line[ne - 1]) {
                    ne -= 1;
                }
                // value: start/end indices into line; vs == ve means empty
                let mut vs = p;
                let mut ve = p;
                if at(line, p) == b'=' {
                    p += 1;
                    sep = true;
                    let vlen = strcspn(&line[p..], b";\r\n");
                    if vlen > 0 {
                        vs = p;
                        ve = p + vlen;
                        p = ve;
                        while vs < ve && is_blank(line[vs]) {
                            vs += 1;
                        }
                        while vs < ve && is_blank(line[ve - 1]) {
                            ve -= 1;
                        }
                        if line[vs..ve].contains(&b'\t') {
                            log.push(b"cookie contains TAB, dropping".to_vec());
                            return Err(Cerr::Tab);
                        }
                    } else {
                        vs = p;
                        ve = p;
                    }
                }
                let name = &line[ns..ne];
                let nl = name.len();
                let vl = ve - vs;
                if nl >= MAX_NAME - 1 || vl >= MAX_NAME - 1 || nl + vl > MAX_NAME {
                    log.push(
                        format!("oversized cookie dropped, name/val {} + {} bytes", nl, vl)
                            .into_bytes(),
                    );
                    return Err(Cerr::TooBig);
                }
                if strn_equal(b"__Secure-", &line[ns..], 9) {
                    co.prefix_secure = true;
                } else if strn_equal(b"__Host-", &line[ns..], 7) {
                    co.prefix_host = true;
                }

                let ieq = |s: &[u8]| name.eq_ignore_ascii_case(s);
                if !have_name {
                    if !sep {
                        return Err(Cerr::NoSep);
                    }
                    co.name = memdup(name);
                    co.value = memdup(&line[vs..ve]);
                    have_name = true;
                    done = true;
                    if invalid_octets(&co.value) || invalid_octets(&co.name) {
                        log.push(b"invalid octets in name/value, cookie dropped".to_vec());
                        return Err(Cerr::InvalidOctet);
                    }
                } else if vl == 0 {
                    done = true;
                    if ieq(b"secure") {
                        if secure || !self.running {
                            co.secure = true;
                        } else {
                            return Err(Cerr::BadSecure);
                        }
                    } else if ieq(b"httponly") {
                        co.httponly = true;
                    } else if sep {
                        done = false;
                    }
                }
                if done {
                } else if ieq(b"path") {
                    let pth = memdup(&line[vs..ve]);
                    co.spath = Some(sanitize_cookie_path(&pth));
                    co.path = Some(pth);
                } else if ieq(b"domain") && vl > 0 {
                    if line[vs] == b'.' {
                        vs += 1;
                    }
                    let val = &line[vs..ve];
                    if bad_domain(val) {
                        domain = Some(b":");
                    }
                    let is_ip = host_is_ipnum(match domain {
                        Some(d) => d,
                        // not NUL-terminated in libcurl: rest of the line
                        None => &line[vs..],
                    });
                    let ok = match domain {
                        None => true,
                        Some(d) => {
                            (is_ip && strn_equal(val, d, val.len()) && val.len() == d.len())
                                || (!is_ip && cookie_tailmatch(val, d))
                        }
                    };
                    if ok {
                        co.domain = Some(memdup(val));
                        if !is_ip {
                            co.tailmatch = true;
                        }
                    } else {
                        let mut m = b"skipped cookie with bad tailmatch domain: ".to_vec();
                        m.extend_from_slice(&line[vs..]);
                        log.push(m);
                        return Err(Cerr::NoTailmatch);
                    }
                } else if ieq(b"version") {
                    // just ignore
                } else if ieq(b"max-age") && vl > 0 {
                    let mut mp = vs;
                    if line[mp] == b'"' {
                        mp += 1;
                    }
                    match str_number(line, &mut mp, CURL_OFF_T_MAX) {
                        Err(NumErr::Overflow) => co.expires = CURL_OFF_T_MAX,
                        Err(NumErr::NoNum) => co.expires = 1,
                        Ok(v) => {
                            co.expires = if v == 0 {
                                1
                            } else if CURL_OFF_T_MAX - now < v {
                                CURL_OFF_T_MAX
                            } else {
                                v + now
                            };
                        }
                    }
                    cap_expires(now, co);
                } else if ieq(b"expires") && vl > 0 && co.expires == 0 && vl < MAX_DATE_LENGTH {
                    co.expires = getdate_capped(&line[vs..ve], now);
                    if co.expires == 0 {
                        co.expires = 1;
                    } else if co.expires < 0 {
                        co.expires = 0;
                    }
                    cap_expires(now, co);
                }
            }
            if at(line, p) != b';' {
                break;
            }
            p += 1;
        }

        if co.domain.is_none()
            && let Some(d) = domain
        {
            co.domain = Some(memdup(d));
        }
        if co.path.is_none()
            && let Some(path) = path.map(cstr)
            && let Some(endslash) = path.iter().rposition(|&b| b == b'/')
        {
            let pth = memdup(&path[..=endslash]);
            co.spath = Some(sanitize_cookie_path(&pth));
            co.path = Some(pth);
        }
        if !have_name {
            return Err(Cerr::Bad);
        }
        *setcookies = setcookies.wrapping_add(1);
        Ok(())
    }

    fn parse_netscape(&self, co: &mut Cookie, line: &[u8], secure: bool) -> Result<(), Cerr> {
        let mut line = cstr(line);
        if line.starts_with(b"#HttpOnly_") {
            line = &line[10..];
            co.httponly = true;
        }
        if at(line, 0) == b'#' {
            return Err(Cerr::Comment);
        }
        let mut fields = 0;
        let mut next = Some(0usize);
        while let Some(ptr) = next {
            let len = strcspn(&line[ptr..], b"\t\r\n");
            next = if at(line, ptr + len) == b'\t' {
                Some(ptr + len + 1)
            } else {
                None
            };
            let field = &line[ptr..];
            let mut do_secure = false;
            match fields {
                0 => {
                    let (mut s, mut l) = (ptr, len);
                    if at(line, s) == b'.' {
                        s += 1;
                        l -= 1;
                    }
                    co.domain = Some(memdup(&line[s..s + l]));
                }
                1 => co.tailmatch = ncasecompare(field, b"TRUE", len),
                2 => {
                    if !strn_equal(b"TRUE", field, len) && !strn_equal(b"FALSE", field, len) {
                        let pth = memdup(&field[..len]);
                        co.spath = Some(sanitize_cookie_path(&pth));
                        co.path = Some(pth);
                    } else {
                        co.path = Some(b"/".to_vec());
                        co.spath = Some(b"/".to_vec());
                        fields += 1;
                        do_secure = true;
                    }
                }
                3 => do_secure = true,
                4 => {
                    let mut p = 0;
                    match str_number(field, &mut p, CURL_OFF_T_MAX) {
                        Ok(v) => co.expires = v,
                        Err(_) => return Err(Cerr::Range),
                    }
                }
                5 => {
                    co.name = memdup(&field[..len]);
                    if ncasecompare(b"__Secure-", &co.name, 9) {
                        co.prefix_secure = true;
                    } else if ncasecompare(b"__Host-", &co.name, 7) {
                        co.prefix_host = true;
                    }
                }
                6 => {
                    co.value = memdup(&field[..len]);
                }
                _ => {}
            }
            if do_secure {
                co.secure = false;
                if ncasecompare(field, b"TRUE", len) {
                    if secure || self.running {
                        co.secure = true;
                    } else {
                        return Err(Cerr::BadSecure);
                    }
                }
            }
            fields += 1;
        }
        if fields == 6 {
            co.value = Vec::new();
            fields += 1;
        }
        if fields != 7 {
            return Err(Cerr::Fields);
        }
        Ok(())
    }

    fn replace_existing(
        &mut self,
        co: &mut Cookie,
        secure: bool,
        log: &mut InfoLog,
    ) -> Result<bool, Cerr> {
        let myhash = cookiehash(co.domain.as_deref());
        let bucket = &self.cookielist[myhash];
        let mut replace_old = false;
        let mut replace_n: Option<usize> = None;
        for (idx, clist) in bucket.iter().enumerate() {
            if clist.name == co.name {
                let matching_domains = match (&clist.domain, &co.domain) {
                    (Some(a), Some(b)) => strcasecompare(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if matching_domains
                    && clist.secure
                    && !co.secure
                    && !secure
                    && let (Some(csp), Some(nsp)) = (&clist.spath, &co.spath)
                {
                    // strchr(clist->spath + 1, '/'); an empty spath is
                    // treated as having no further separator
                    let cllen = if csp.is_empty() {
                        0
                    } else {
                        match csp[1..].iter().position(|&b| b == b'/') {
                            Some(i) => i + 1,
                            None => csp.len(),
                        }
                    };
                    if ncasecompare(csp, nsp, cllen) {
                        let mut m = b"cookie '".to_vec();
                        m.extend_from_slice(&co.name);
                        m.extend_from_slice(b"' for domain '");
                        m.extend_from_slice(fmt_nil(&co.domain));
                        m.extend_from_slice(b"' dropped, would overlay an existing cookie");
                        log.push(m);
                        return Err(Cerr::BadSecure);
                    }
                }
            }
            if replace_n.is_none() && clist.name == co.name {
                match (&clist.domain, &co.domain) {
                    (Some(a), Some(b)) => {
                        if strcasecompare(a, b) && clist.tailmatch == co.tailmatch {
                            replace_old = true;
                        }
                    }
                    (None, None) => replace_old = true,
                    _ => {}
                }
                if replace_old {
                    match (&clist.spath, &co.spath) {
                        (Some(a), Some(b)) => {
                            if !strcasecompare(a, b) {
                                replace_old = false;
                            }
                        }
                        (None, None) => {}
                        _ => replace_old = false,
                    }
                }
                if replace_old && !co.livecookie && clist.livecookie {
                    return Err(Cerr::LiveWins);
                }
                if replace_old {
                    replace_n = Some(idx);
                }
            }
        }
        if let Some(idx) = replace_n {
            let repl = self.cookielist[myhash].remove(idx);
            co.creationtime = repl.creationtime;
        }
        Ok(replace_old)
    }

    /// Curl_cookie_add(): add a single cookie line to the jar.
    ///
    /// * `setcookies` is the per-request counter (`data->req.setcookies`),
    ///   reset to 0 by the engine in Curl_req_hard_reset() i.e. at every
    ///   Curl_connect(); once it reaches 50 no more cookies are added.
    /// * `httpheader`: TRUE for a Set-Cookie: style line (the text after
    ///   "Set-Cookie:"), FALSE for a Netscape jar line.
    /// * `noexpire`: skip remove_expired().
    /// * `domain`/`path`: request host and URL path (only used for header
    ///   lines), `secure`: TRUE if the origin is secure.
    ///
    /// Returns the added cookie, or None if rejected.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        setcookies: &mut u8,
        httpheader: bool,
        noexpire: bool,
        line: &[u8],
        domain: Option<&[u8]>,
        path: Option<&[u8]>,
        secure: bool,
        log: &mut InfoLog,
    ) -> Option<&Cookie> {
        if *setcookies >= MAX_SET_COOKIE_AMOUNT {
            return None;
        }
        let mut co = Cookie::default();
        let rc = if httpheader {
            self.parse_cookie_header(&mut co, line, domain, path, secure, setcookies, log)
        } else {
            self.parse_netscape(&mut co, line, secure)
        };
        if rc.is_err() {
            return None;
        }
        if co.prefix_secure && !co.secure {
            return None;
        }
        if co.prefix_host
            && !(co.secure && co.path.as_deref() == Some(b"/".as_slice()) && !co.tailmatch)
        {
            return None;
        }
        if !self.running && self.newsession && co.expires == 0 {
            return None;
        }
        co.livecookie = self.running;
        self.lastct = self.lastct.wrapping_add(1);
        co.creationtime = self.lastct;

        if !noexpire {
            self.remove_expired();
        }
        let replaces = self.replace_existing(&mut co, secure, log).ok()?;

        if self.running {
            let mut m = Vec::new();
            m.extend_from_slice(if replaces { b"Replaced" } else { b"Added" });
            m.extend_from_slice(b" cookie ");
            m.extend_from_slice(&co.name);
            m.extend_from_slice(b"=\"");
            m.extend_from_slice(&co.value);
            m.extend_from_slice(b"\" for domain ");
            m.extend_from_slice(fmt_nil(&co.domain));
            m.extend_from_slice(b", path ");
            m.extend_from_slice(fmt_nil(&co.path));
            m.extend_from_slice(b", expire ");
            m.extend_from_slice(co.expires.to_string().as_bytes());
            log.push(m);
        }
        if !replaces {
            self.numcookies = self.numcookies.wrapping_add(1);
        }
        if co.expires != 0 && co.expires < self.next_expiration {
            self.next_expiration = co.expires;
        }
        let myhash = cookiehash(co.domain.as_deref());
        self.cookielist[myhash].push(co);
        self.cookielist[myhash].last()
    }

    /// Handle a CURLOPT_COOKIELIST string that is not one of the keywords:
    /// a "Set-Cookie:" line (prefix matched case-insensitively) or a
    /// Netscape line. The jar must exist (create it with
    /// `CookieInfo::new_initialized(true)` first, like libcurl).
    pub fn add_cookielist_line(
        &mut self,
        setcookies: &mut u8,
        line: &[u8],
        log: &mut InfoLog,
    ) -> bool {
        let line = cstr(line);
        if ncasecompare(line, b"Set-Cookie:", 11) {
            self.add(setcookies, true, false, &line[11..], None, None, true, log)
                .is_some()
        } else {
            self.add(setcookies, false, false, line, None, None, true, log)
                .is_some()
        }
    }

    /// Curl_cookie_getlist(): the cookies to send to `host` for `path`, in
    /// libcurl's order. Returns None when libcurl returns non-zero (no cookie
    /// in the host's hash bucket), Some (possibly empty) otherwise.
    pub fn getlist(
        &mut self,
        host: &[u8],
        path: &[u8],
        secure: bool,
        log: &mut InfoLog,
    ) -> Option<Vec<Cookie>> {
        let host = cstr(host);
        let myhash = cookiehash(Some(host));
        if self.cookielist[myhash].is_empty() {
            return None;
        }
        self.remove_expired();
        let is_ip = host_is_ipnum(host);
        let mut list: Vec<Cookie> = Vec::new();
        for co in &self.cookielist[myhash] {
            if co.secure && !secure {
                continue;
            }
            let dom_ok = match &co.domain {
                None => true,
                Some(d) => {
                    (co.tailmatch && !is_ip && cookie_tailmatch(d, host))
                        || ((!co.tailmatch || is_ip) && strcasecompare(host, d))
                }
            };
            if !dom_ok {
                continue;
            }
            if co.spath.as_ref().is_none_or(|sp| pathmatch(sp, path)) {
                list.push(co.clone());
                if list.len() >= MAX_COOKIE_SEND_AMOUNT {
                    log.push(
                        format!(
                            "Included max number of cookies ({}) in request!",
                            list.len()
                        )
                        .into_bytes(),
                    );
                    break;
                }
            }
        }
        list.sort_by(cookie_sort);
        Some(list)
    }

    /// Curl_cookie_clearall().
    pub fn clearall(&mut self) {
        for b in self.cookielist.iter_mut() {
            b.clear();
        }
        self.numcookies = 0;
    }

    /// Curl_cookie_clearsess(): remove all session cookies.
    pub fn clearsess(&mut self) {
        let mut removed = 0u32;
        for b in self.cookielist.iter_mut() {
            b.retain(|c| {
                if c.expires == 0 {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
        self.numcookies = self.numcookies.wrapping_sub(removed);
    }

    /// cookie_list() for CURLINFO_COOKIELIST: one Netscape line per cookie
    /// that has a domain, in internal order. No remove_expired() is done.
    pub fn netscape_lines(&self) -> Vec<Vec<u8>> {
        if self.numcookies == 0 {
            return Vec::new();
        }
        self.cookies()
            .filter(|c| c.domain.is_some())
            .map(get_netscape_format)
            .collect()
    }

    /// The bytes cookie_output() writes (runs remove_expired() first, like
    /// libcurl): header comment, then cookies newest first.
    pub fn jar_contents(&mut self) -> Vec<u8> {
        self.remove_expired();
        let mut out = b"# Netscape HTTP Cookie File\n\
# https://curl.se/docs/http-cookies.html\n\
# This file was generated by libcurl! Edit at your own risk.\n\n"
            .to_vec();
        if self.numcookies != 0 {
            let mut array: Vec<&Cookie> = self.cookies().filter(|c| c.domain.is_some()).collect();
            array.sort_by_key(|a| std::cmp::Reverse(a.creationtime));
            for co in array {
                out.extend_from_slice(&get_netscape_format(co));
                out.push(b'\n');
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Writing the jar (cookie_output() + Curl_fopen())
// ---------------------------------------------------------------------------

fn dirslash(path: &[u8]) -> Vec<u8> {
    let mut n = path.len();
    while n > 0 && path[n - 1] != b'/' {
        n -= 1;
    }
    while n > 0 && path[n - 1] == b'/' {
        n -= 1;
    }
    let mut out = path[..n].to_vec();
    if n > 0 {
        out.push(b'/');
    }
    out
}

fn rand_alnum(num: usize) -> Vec<u8> {
    use std::hash::{BuildHasher, Hasher};
    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let state = std::collections::hash_map::RandomState::new();
    let mut out = Vec::with_capacity(num);
    let mut counter: u64 = 0;
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    while out.len() < num {
        let mut h = state.build_hasher();
        h.write_u64(t);
        h.write_u64(counter);
        h.write_u32(std::process::id());
        counter += 1;
        let r = h.finish();
        out.push(ALNUM[(r % ALNUM.len() as u64) as usize]);
    }
    out
}

#[cfg(unix)]
fn open_excl(path: &std::path::Path, mode: u32) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode & 0o7777)
        .open(path)
}

#[cfg(not(unix))]
fn open_excl(path: &std::path::Path, _mode: u32) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn file_mode(m: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    m.mode()
}

#[cfg(not(unix))]
fn file_mode(_m: &std::fs::Metadata) -> u32 {
    0o644
}

/// Write `content` to `filename` the way Curl_fopen() + cookie_output() do:
/// the target is first opened for writing (truncating it); if it is a
/// regular file, the data is written to a random temp file in the same
/// directory which is then renamed over it. Write errors are ignored like
/// libcurl does (fputs return values are not checked).
fn curl_fopen_write(filename: &[u8], content: &[u8]) -> Result<(), u32> {
    let path = bytes_to_path(filename);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|_| CURLE_WRITE_ERROR)?;
    let meta = match f.metadata() {
        Ok(m) if m.is_file() => m,
        _ => {
            let _ = f.write_all(content);
            return Ok(());
        }
    };
    drop(f);
    let mut tempstore = dirslash(filename);
    tempstore.extend_from_slice(&rand_alnum(40));
    tempstore.extend_from_slice(b".tmp");
    let temppath = bytes_to_path(&tempstore);
    let mut t = open_excl(&temppath, 0o600 | file_mode(&meta)).map_err(|_| CURLE_WRITE_ERROR)?;
    let _ = t.write_all(content);
    drop(t);
    if std::fs::rename(&temppath, &path).is_err() {
        let _ = std::fs::remove_file(&temppath);
        return Err(CURLE_WRITE_ERROR);
    }
    Ok(())
}

/// cookie_output(): write all cookies to `filename` ("-" = stdout).
/// With no jar (`None`) nothing is written and Ok is returned. Errors are
/// CURLcode values (CURLE_WRITE_ERROR).
pub fn cookie_output(ci: Option<&mut CookieInfo>, filename: &[u8]) -> Result<(), u32> {
    let Some(ci) = ci else {
        return Ok(());
    };
    let filename = cstr(filename);
    let content = ci.jar_contents();
    if filename == b"-" {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(&content);
        let _ = out.flush();
        return Ok(());
    }
    curl_fopen_write(filename, &content)
}

/// The jar-writing part of Curl_flush_cookies(): when a COOKIEJAR is set,
/// write the jar and log libcurl's warning on failure. (Freeing the jar on
/// cleanup — unless it belongs to a share — is the caller's job.)
pub fn flush_cookies(cookiejar: Option<&[u8]>, ci: Option<&mut CookieInfo>, log: &mut InfoLog) {
    if let Some(jar) = cookiejar
        && let Err(code) = cookie_output(ci, jar)
    {
        let mut m = b"WARNING: failed to save cookies in ".to_vec();
        m.extend_from_slice(cstr(jar));
        m.extend_from_slice(b": ");
        m.extend_from_slice(match code {
            CURLE_WRITE_ERROR => b"Failed writing received data to disk/application".as_slice(),
            _ => b"Out of memory".as_slice(),
        });
        log.push(m);
    }
}

/// Curl_cookie_loadfiles(): load every CURLOPT_COOKIEFILE name (in the order
/// they were set) into the jar, creating it if needed. Does nothing when the
/// list is empty. The caller sets `state.cookie_engine = TRUE` when the list
/// was non-empty.
pub fn load_files(
    cookies: &mut Option<CookieInfo>,
    files: &[Vec<u8>],
    cookiesession: bool,
    setcookies: &mut u8,
    log: &mut InfoLog,
) {
    for f in files {
        let ci = cookies.get_or_insert_with(CookieInfo::new);
        ci.init(Some(f), cookiesession, setcookies, log);
    }
}

// ---------------------------------------------------------------------------
// http.c helpers
// ---------------------------------------------------------------------------

/// The `secure_context` http.c computes for cookies: HTTPS/WSS, or a host
/// that is "localhost" (any case), "127.0.0.1" or "::1".
pub fn secure_context(https_or_wss: bool, host: &[u8]) -> bool {
    let host = cstr(host);
    https_or_wss || strcasecompare(b"localhost", host) || host == b"127.0.0.1" || host == b"::1"
}

/// http_cookies(): build the complete "Cookie: ...\r\n" request header line
/// (empty when libcurl would not send one).
///
/// * `cookies`/`cookie_engine`: `data->cookies` and
///   `data->state.cookie_engine` (the jar is only consulted when both).
/// * `host`: the cookie host (custom Host: header derived, see
///   [`cookiehost_from_host_header`], else `conn->host.name`).
/// * `path`: `data->state.up.path`.
/// * `addcookies`: CURLOPT_COOKIE, already set to None by the caller when
///   a custom "Cookie" header is present (`Curl_checkheaders`).
pub fn http_cookie_header(
    cookies: Option<&mut CookieInfo>,
    cookie_engine: bool,
    host: &[u8],
    path: &[u8],
    secure_context: bool,
    addcookies: Option<&[u8]>,
    log: &mut InfoLog,
) -> Vec<u8> {
    let mut r = Vec::new();
    let has_jar = cookies.is_some();
    if !has_jar && addcookies.is_none() {
        return r;
    }
    let mut count = 0usize;
    let mut linecap = false;
    let list = match cookies {
        Some(ci) if cookie_engine => ci.getlist(host, path, secure_context, log),
        _ => None,
    };
    if let Some(list) = list {
        let mut clen = 8usize;
        for co in &list {
            if count == 0 {
                r.extend_from_slice(b"Cookie: ");
            }
            let add = co.name.len() + co.value.len() + 1;
            if clen + add >= MAX_COOKIE_HEADER_LEN {
                let mut m = b"Restricted outgoing cookies due to header size, '".to_vec();
                m.extend_from_slice(&co.name);
                m.extend_from_slice(b"' not sent");
                log.push(m);
                linecap = true;
                break;
            }
            if count > 0 {
                r.extend_from_slice(b"; ");
            }
            r.extend_from_slice(&co.name);
            r.push(b'=');
            r.extend_from_slice(&co.value);
            clen += add + if count > 0 { 2 } else { 0 };
            count += 1;
        }
    }
    if let Some(add) = addcookies
        && !linecap
    {
        if count == 0 {
            r.extend_from_slice(b"Cookie: ");
        }
        if count > 0 {
            r.extend_from_slice(b"; ");
        }
        r.extend_from_slice(cstr(add));
        count += 1;
    }
    if count > 0 {
        r.extend_from_slice(b"\r\n");
    }
    r
}

/// The cookie host http.c derives from a custom "Host:" header (the whole
/// header line, e.g. `b"Host: example.com:8080"`), via
/// Curl_copy_header_value() and the bracket/port stripping (including
/// libcurl's memmove quirk for "[..."). None when libcurl keeps no
/// cookiehost (empty value / bad input).
pub fn cookiehost_from_host_header(header: &[u8]) -> Option<Vec<u8>> {
    let h = cstr(header);
    let nlen = strcspn(h, b";:");
    if nlen == 0 {
        return None;
    }
    let c = at(h, nlen);
    if c != b':' && c != b';' {
        return None;
    }
    let rest = &h[nlen + 1..];
    let vlen = rest
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(rest.len());
    let mut v = &rest[..vlen];
    while let Some((&f, r)) = v.split_first()
        && is_blank(f)
    {
        v = r;
    }
    while let Some((&l, r)) = v.split_last()
        && is_blank(l)
    {
        v = r;
    }
    if v.is_empty() {
        return None;
    }
    let mut ch = v.to_vec();
    if ch[0] == b'[' {
        let n = ch.len();
        ch.copy_within(1..n, 0);
        if let Some(i) = ch.iter().position(|&b| b == b']') {
            ch.truncate(i);
        }
    } else if let Some(i) = ch.iter().position(|&b| b == b':') {
        ch.truncate(i);
    }
    Some(ch)
}

/// What a CURLOPT_COOKIELIST string asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookieListCommand<'a> {
    /// "ALL": Curl_cookie_clearall()
    All,
    /// "SESS": Curl_cookie_clearsess()
    Sess,
    /// "FLUSH": Curl_flush_cookies(data, FALSE)
    Flush,
    /// "RELOAD": Curl_cookie_loadfiles()
    Reload,
    /// Anything else: a cookie line for [`CookieInfo::add_cookielist_line`]
    Line(&'a [u8]),
}

/// Classify a CURLOPT_COOKIELIST argument (keywords are case-insensitive).
pub fn parse_cookielist(arg: &[u8]) -> CookieListCommand<'_> {
    if strcasecompare(arg, b"ALL") {
        CookieListCommand::All
    } else if strcasecompare(arg, b"SESS") {
        CookieListCommand::Sess
    } else if strcasecompare(arg, b"FLUSH") {
        CookieListCommand::Flush
    } else if strcasecompare(arg, b"RELOAD") {
        CookieListCommand::Reload
    } else {
        CookieListCommand::Line(cstr(arg))
    }
}

// ---------------------------------------------------------------------------
// parsedate.c
// ---------------------------------------------------------------------------

const WKDAY: [&[u8]; 7] = [b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat", b"Sun"];
const WEEKDAY: [&[u8]; 7] = [
    b"Monday",
    b"Tuesday",
    b"Wednesday",
    b"Thursday",
    b"Friday",
    b"Saturday",
    b"Sunday",
];
const MONTH: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
];

const TZ_DAYZONE: i32 = -60;
const TZ: [(&[u8], i32); 69] = [
    (b"GMT", 0),
    (b"UT", 0),
    (b"UTC", 0),
    (b"WET", 0),
    (b"BST", TZ_DAYZONE),
    (b"WAT", 60),
    (b"AST", 240),
    (b"ADT", 240 + TZ_DAYZONE),
    (b"EST", 300),
    (b"EDT", 300 + TZ_DAYZONE),
    (b"CST", 360),
    (b"CDT", 360 + TZ_DAYZONE),
    (b"MST", 420),
    (b"MDT", 420 + TZ_DAYZONE),
    (b"PST", 480),
    (b"PDT", 480 + TZ_DAYZONE),
    (b"YST", 540),
    (b"YDT", 540 + TZ_DAYZONE),
    (b"HST", 600),
    (b"HDT", 600 + TZ_DAYZONE),
    (b"CAT", 600),
    (b"AHST", 600),
    (b"NT", 660),
    (b"IDLW", 720),
    (b"CET", -60),
    (b"MET", -60),
    (b"MEWT", -60),
    (b"MEST", -60 + TZ_DAYZONE),
    (b"CEST", -60 + TZ_DAYZONE),
    (b"MESZ", -60 + TZ_DAYZONE),
    (b"FWT", -60),
    (b"FST", -60 + TZ_DAYZONE),
    (b"EET", -120),
    (b"WAST", -420),
    (b"WADT", -420 + TZ_DAYZONE),
    (b"CCT", -480),
    (b"JST", -540),
    (b"EAST", -600),
    (b"EADT", -600 + TZ_DAYZONE),
    (b"GST", -600),
    (b"NZT", -720),
    (b"NZST", -720),
    (b"NZDT", -720 + TZ_DAYZONE),
    (b"IDLE", -720),
    (b"A", 60),
    (b"B", 2 * 60),
    (b"C", 3 * 60),
    (b"D", 4 * 60),
    (b"E", 5 * 60),
    (b"F", 6 * 60),
    (b"G", 7 * 60),
    (b"H", 8 * 60),
    (b"I", 9 * 60),
    (b"K", 10 * 60),
    (b"L", 11 * 60),
    (b"M", 12 * 60),
    (b"N", -60),
    (b"O", -2 * 60),
    (b"P", -3 * 60),
    (b"Q", -4 * 60),
    (b"R", -5 * 60),
    (b"S", -6 * 60),
    (b"T", -7 * 60),
    (b"U", -8 * 60),
    (b"V", -9 * 60),
    (b"W", -10 * 60),
    (b"X", -11 * 60),
    (b"Y", -12 * 60),
    (b"Z", 0),
];

fn checkday(check: &[u8]) -> i32 {
    let what: &[&[u8]; 7] = match check.len() {
        n if n > 3 => &WEEKDAY,
        3 => &WKDAY,
        _ => return -1,
    };
    for (i, w) in what.iter().enumerate() {
        if w.len() == check.len() && w.eq_ignore_ascii_case(check) {
            return i as i32;
        }
    }
    -1
}

fn checkmonth(check: &[u8]) -> i32 {
    if check.len() != 3 {
        return -1;
    }
    for (i, m) in MONTH.iter().enumerate() {
        if m.eq_ignore_ascii_case(check) {
            return i as i32;
        }
    }
    -1
}

fn checktz(check: &[u8]) -> i32 {
    if check.len() > 4 {
        return -1;
    }
    for (name, off) in TZ.iter() {
        if name.len() == check.len() && name.eq_ignore_ascii_case(check) {
            return off * 60;
        }
    }
    -1
}

fn time2epoch(sec: i32, min: i32, hour: i32, mday: i32, mon: i32, year: i32) -> i64 {
    const CUM: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut leap_days = year - (mon <= 1) as i32;
    leap_days = (leap_days / 4) - (leap_days / 100) + (leap_days / 400) - (1969 / 4) + (1969 / 100)
        - (1969 / 400);
    (((((year - 1970) as i64) * 365 + leap_days as i64 + CUM[mon as usize] as i64 + mday as i64
        - 1)
        * 24
        + hour as i64)
        * 60
        + min as i64)
        * 60
        + sec as i64
}

fn oneortwodigit(d: &[u8], i: usize) -> (i32, usize) {
    let num = (at(d, i) - b'0') as i32;
    if at(d, i + 1).is_ascii_digit() {
        (num * 10 + (at(d, i + 1) - b'0') as i32, i + 2)
    } else {
        (num, i + 1)
    }
}

fn match_time(d: &[u8], i: usize) -> Option<(i32, i32, i32, usize)> {
    let (hh, p) = oneortwodigit(d, i);
    if hh < 24 && at(d, p) == b':' && at(d, p + 1).is_ascii_digit() {
        let (mm, p) = oneortwodigit(d, p + 1);
        if mm < 60 {
            if at(d, p) == b':' && at(d, p + 1).is_ascii_digit() {
                let (ss, p) = oneortwodigit(d, p + 1);
                if ss <= 60 {
                    return Some((hh, mm, ss, p));
                }
            } else {
                return Some((hh, mm, 0, p));
            }
        }
    }
    None
}

enum ParseDate {
    Ok(i64),
    Fail,
    Later(i64),
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Assume {
    Mday,
    Year,
}

fn parsedate(date: &[u8]) -> ParseDate {
    const NAME_LEN: usize = 12;
    let d = cstr(date);
    let mut wdaynum = -1;
    let mut monnum = -1;
    let mut mdaynum = -1;
    let mut hournum = -1;
    let mut minnum = -1;
    let mut secnum = -1;
    let mut yearnum = -1;
    let mut tzoff = -1;
    let mut dignext = Assume::Mday;
    let mut i = 0usize;
    let mut part = 0;

    while at(d, i) != 0 && part < 6 {
        let mut found = false;
        while at(d, i) != 0 && !at(d, i).is_ascii_alphanumeric() {
            i += 1;
        }
        if at(d, i).is_ascii_alphabetic() {
            let mut len = 0;
            while at(d, i + len).is_ascii_alphabetic() && len < NAME_LEN {
                len += 1;
            }
            let word = &d[i..i + len];
            if len != NAME_LEN {
                if wdaynum == -1 {
                    wdaynum = checkday(word);
                    if wdaynum != -1 {
                        found = true;
                    }
                }
                if !found && monnum == -1 {
                    monnum = checkmonth(word);
                    if monnum != -1 {
                        found = true;
                    }
                }
                if !found && tzoff == -1 {
                    tzoff = checktz(word);
                    if tzoff != -1 {
                        found = true;
                    }
                }
            }
            if !found {
                return ParseDate::Fail;
            }
            i += len;
        } else if at(d, i).is_ascii_digit() {
            let mt = if secnum == -1 { match_time(d, i) } else { None };
            if let Some((h, m, s, end)) = mt {
                hournum = h;
                minnum = m;
                secnum = s;
                i = end;
            } else {
                let mut p = i;
                let lval = match str_number(d, &mut p, 99999999) {
                    Ok(v) => v,
                    Err(_) => return ParseDate::Fail,
                };
                let num_digits = p - i;
                let val = lval as u32;
                if tzoff == -1
                    && num_digits == 4
                    && val <= 1400
                    && i > 0
                    && (d[i - 1] == b'+' || d[i - 1] == b'-')
                {
                    found = true;
                    let off = ((val / 100 * 60 + val % 100) * 60) as i32;
                    tzoff = if d[i - 1] == b'+' { -off } else { off };
                } else if num_digits == 8 && yearnum == -1 && monnum == -1 && mdaynum == -1 {
                    found = true;
                    yearnum = (val / 10000) as i32;
                    monnum = ((val % 10000) / 100) as i32 - 1;
                    mdaynum = (val % 100) as i32;
                }
                if !found && dignext == Assume::Mday && mdaynum == -1 {
                    if val > 0 && val < 32 {
                        mdaynum = val as i32;
                        found = true;
                    }
                    dignext = Assume::Year;
                }
                if !found && dignext == Assume::Year && yearnum == -1 {
                    yearnum = val as i32;
                    found = true;
                    if yearnum < 100 {
                        if yearnum > 70 {
                            yearnum += 1900;
                        } else {
                            yearnum += 2000;
                        }
                    }
                    if mdaynum == -1 {
                        dignext = Assume::Mday;
                    }
                }
                if !found {
                    return ParseDate::Fail;
                }
                i = p;
            }
        }
        part += 1;
    }

    if secnum == -1 {
        secnum = 0;
        minnum = 0;
        hournum = 0;
    }
    if mdaynum == -1 || monnum == -1 || yearnum == -1 {
        return ParseDate::Fail;
    }
    if yearnum < 1583 {
        return ParseDate::Fail;
    }
    if mdaynum > 31 || monnum > 11 || hournum > 23 || minnum > 59 || secnum > 60 {
        return ParseDate::Fail;
    }
    let t = time2epoch(secnum, minnum, hournum, mdaynum, monnum, yearnum);
    if tzoff == -1 {
        tzoff = 0;
    }
    if tzoff > 0 && t > i64::MAX - tzoff as i64 {
        return ParseDate::Later(i64::MAX);
    }
    ParseDate::Ok(t + tzoff as i64)
}

/// curl_getdate(): parse a date string, -1 on failure (a valid date that
/// would be -1 is returned as 0).
pub fn curl_getdate(p: &[u8]) -> i64 {
    match parsedate(p) {
        ParseDate::Ok(t) => {
            if t == -1 {
                0
            } else {
                t
            }
        }
        _ => -1,
    }
}

/// Curl_getdate_capped(): like curl_getdate() but returns the maximum time
/// value instead of failing when the date is too far in the future. `now`
/// is ignored (libcurl's parser does not use it either).
pub fn getdate_capped(p: &[u8], _now: i64) -> i64 {
    match parsedate(p) {
        ParseDate::Ok(t) => {
            if t == -1 {
                0
            } else {
                t
            }
        }
        ParseDate::Later(t) => t,
        ParseDate::Fail => -1,
    }
}

#[cfg(test)]
mod tests {
    // Expected values below were all obtained from the real libcurl 8.14.1
    // (CURLOPT_COOKIELIST, CURLINFO_COOKIELIST, COOKIEJAR files, verbose
    // output and the Cookie: header sent to a local test server).
    use super::*;

    fn s(v: &[Vec<u8>]) -> Vec<String> {
        v.iter()
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect()
    }

    /// A jar as created by COOKIELIST on a fresh handle.
    fn jar(now: i64) -> CookieInfo {
        let mut ci = CookieInfo::new_initialized(true);
        ci.fixed_now = Some(now);
        ci
    }

    fn cl(ci: &mut CookieInfo, sc: &mut u8, line: &str, log: &mut InfoLog) {
        match parse_cookielist(line.as_bytes()) {
            CookieListCommand::All => ci.clearall(),
            CookieListCommand::Sess => ci.clearsess(),
            CookieListCommand::Line(l) => {
                ci.add_cookielist_line(sc, l, log);
            }
            _ => panic!(),
        }
    }

    /// Feed a response's Set-Cookie headers like http.c does.
    fn response(ci: &mut CookieInfo, host: &str, path: &str, hdrs: &[&str], log: &mut InfoLog) {
        let mut sc = 0u8; // Curl_req_hard_reset() at Curl_connect()
        let secure = secure_context(false, host.as_bytes());
        for h in hdrs {
            let line = format!("Set-Cookie:{}\r\n", h);
            ci.add(
                &mut sc,
                true,
                false,
                &line.as_bytes()[11..],
                Some(host.as_bytes()),
                Some(path.as_bytes()),
                secure,
                log,
            );
        }
    }

    fn header(
        ci: &mut CookieInfo,
        host: &str,
        path: &str,
        add: Option<&str>,
        log: &mut InfoLog,
    ) -> String {
        let secure = secure_context(false, host.as_bytes());
        let h = http_cookie_header(
            Some(ci),
            true,
            host.as_bytes(),
            path.as_bytes(),
            secure,
            add.map(|a| a.as_bytes()),
            log,
        );
        String::from_utf8_lossy(&h).into_owned()
    }

    const JAR_HEAD: &str = "# Netscape HTTP Cookie File\n# https://curl.se/docs/http-cookies.html\n# This file was generated by libcurl! Edit at your own risk.\n\n";

    #[test]
    fn cookielist_lines() {
        let now = 1792611612;
        let mut ci = jar(now);
        let mut sc = 0u8;
        let mut log = InfoLog::new();
        let input = [
            "Set-Cookie: a=b",
            "Set-Cookie: c=d; domain=example.com",
            "Set-Cookie: e=f; domain=.example.com; path=/foo/; secure; httponly",
            "example.com\tFALSE\t/\tFALSE\t0\tg\th",
            "#HttpOnly_.www.example.com\tTRUE\t/x\tTRUE\t1900000000\ti\tj",
            "Set-Cookie: k=l; domain=evil.org; path=/",
            "Set-Cookie: m=n; domain=1.2.3.4",
            "Set-Cookie: o=p; domain=1.2.3.4; path=/",
            "Set-Cookie: q=r; domain=localhost",
            "Set-Cookie: s=t; domain=com",
            "Set-Cookie: u=v; domain=:",
            "Set-Cookie: c=dd; domain=example.com",
            "Set-Cookie: __Secure-x=1",
            "Set-Cookie: __Secure-y=1; secure",
            "Set-Cookie: __Host-z=1; secure; path=/",
            "Set-Cookie: __Host-w=1; secure; path=/; domain=example.com",
            "Set-Cookie: tab=a\tb",
            "Set-Cookie: ctl=a\x01b",
            "Set-Cookie: noeq",
            "Set-Cookie: ma=1; max-age=100",
            "Set-Cookie: ma0=1; max-age=0",
            "Set-Cookie: man=1; max-age=-5",
            "Set-Cookie: mab=1; max-age=99999999999999999999999",
            "Set-Cookie: ex=1; expires=Wed, 09 Jun 2027 10:18:14 GMT",
            "Set-Cookie: ex2=1; expires=Wed, 09 Jun 2037 10:18:14 GMT",
            "Set-Cookie: ex3=1; expires=garbage",
            "Set-Cookie: ex4=1; expires=Thu, 01 Jan 1960 00:00:00 GMT",
            "Set-Cookie: ex5=1; expires=Thu, 01 Jan 1970 00:00:00 GMT",
            "Set-Cookie: ex6=1; expires=Thu, 01 Jan 1971 00:00:00 GMT; domain=example.com",
        ];
        for l in input {
            cl(&mut ci, &mut sc, l, &mut log);
        }
        assert_eq!(
            s(&log),
            [
                "Added cookie a=\"b\" for domain (nil), path (nil), expire 0",
                "Added cookie c=\"d\" for domain example.com, path (nil), expire 0",
                "Added cookie e=\"f\" for domain example.com, path /foo/, expire 0",
                "Added cookie g=\"h\" for domain example.com, path /, expire 0",
                "Added cookie i=\"j\" for domain www.example.com, path /x, expire 1900000000",
                "Added cookie k=\"l\" for domain evil.org, path /, expire 0",
                "Added cookie m=\"n\" for domain 1.2.3.4, path (nil), expire 0",
                "Added cookie o=\"p\" for domain 1.2.3.4, path /, expire 0",
                "Added cookie q=\"r\" for domain localhost, path (nil), expire 0",
                "skipped cookie with bad tailmatch domain: com",
                "Added cookie u=\"v\" for domain :, path (nil), expire 0",
                "Replaced cookie c=\"dd\" for domain example.com, path (nil), expire 0",
                "Added cookie __Secure-y=\"1\" for domain (nil), path (nil), expire 0",
                "Added cookie __Host-z=\"1\" for domain (nil), path /, expire 0",
                "cookie contains TAB, dropping",
                "invalid octets in name/value, cookie dropped",
                "Added cookie ma=\"1\" for domain (nil), path (nil), expire 1792611712",
                "Added cookie ma0=\"1\" for domain (nil), path (nil), expire 1",
                "Added cookie man=\"1\" for domain (nil), path (nil), expire 1",
                "Added cookie mab=\"1\" for domain (nil), path (nil), expire 1827171600",
                "Added cookie ex=\"1\" for domain (nil), path (nil), expire 1812536294",
                "Added cookie ex2=\"1\" for domain (nil), path (nil), expire 1827171600",
                "Added cookie ex3=\"1\" for domain (nil), path (nil), expire 0",
                "Added cookie ex4=\"1\" for domain (nil), path (nil), expire 0",
                "Added cookie ex5=\"1\" for domain (nil), path (nil), expire 1",
                "Added cookie ex6=\"1\" for domain example.com, path (nil), expire 31536000",
            ]
        );
        let p1 = [
            "1.2.3.4\tFALSE\t/\tFALSE\t0\tm\tn",
            ".1.2.3.4\tTRUE\t/\tFALSE\t0\to\tp",
            ".evil.org\tTRUE\t/\tFALSE\t0\tk\tl",
            ".:\tTRUE\t/\tFALSE\t0\tu\tv",
            ".localhost\tTRUE\t/\tFALSE\t0\tq\tr",
            "#HttpOnly_.example.com\tTRUE\t/foo/\tTRUE\t0\te\tf",
            "example.com\tFALSE\t/\tFALSE\t0\tg\th",
            "#HttpOnly_.www.example.com\tTRUE\t/x\tTRUE\t1900000000\ti\tj",
            ".example.com\tTRUE\t/\tFALSE\t0\tc\tdd",
            ".example.com\tTRUE\t/\tFALSE\t31536000\tex6\t1",
        ];
        assert_eq!(s(&ci.netscape_lines()), p1);
        let jar = String::from_utf8(ci.jar_contents()).unwrap();
        assert_eq!(
            jar,
            format!(
                "{JAR_HEAD}.:\tTRUE\t/\tFALSE\t0\tu\tv\n\
.localhost\tTRUE\t/\tFALSE\t0\tq\tr\n\
.1.2.3.4\tTRUE\t/\tFALSE\t0\to\tp\n\
1.2.3.4\tFALSE\t/\tFALSE\t0\tm\tn\n\
.evil.org\tTRUE\t/\tFALSE\t0\tk\tl\n\
#HttpOnly_.www.example.com\tTRUE\t/x\tTRUE\t1900000000\ti\tj\n\
example.com\tFALSE\t/\tFALSE\t0\tg\th\n\
#HttpOnly_.example.com\tTRUE\t/foo/\tTRUE\t0\te\tf\n\
.example.com\tTRUE\t/\tFALSE\t0\tc\tdd\n"
            )
        );
        // the jar output removed the expired cookie
        assert_eq!(s(&ci.netscape_lines()), p1[..9]);
    }

    #[test]
    fn http_flow() {
        let mut ci = jar(1792611815);
        ci.newsession = false;
        let mut log = InfoLog::new();
        response(
            &mut ci,
            "www.example.com",
            "/dir/page.html",
            &[
                " a=1",
                " b=2; path=/",
                " c=3; domain=example.com",
                " d=4; domain=evil.com; path=/",
                " e=5; secure",
                " aa=6; path=/dir/sub",
                " a=7; path=/",
                " a=8; domain=.www.example.com; path=/",
                "    sp = x y ; Path = / ;  HttpOnly",
                " x=1; path=\"/dir/\"",
                " y=1; path=dir",
                " z=1; path=",
                " q=1; domain=",
                " r=1; domain=WWW.example.COM",
            ],
            &mut log,
        );
        assert_eq!(
            s(&log),
            [
                "Added cookie a=\"1\" for domain www.example.com, path /dir/, expire 0",
                "Added cookie b=\"2\" for domain www.example.com, path /, expire 0",
                "Added cookie c=\"3\" for domain example.com, path /dir/, expire 0",
                "skipped cookie with bad tailmatch domain: evil.com; path=/\r\n",
                "Added cookie aa=\"6\" for domain www.example.com, path /dir/sub, expire 0",
                "Added cookie a=\"7\" for domain www.example.com, path /, expire 0",
                "Added cookie a=\"8\" for domain www.example.com, path /, expire 0",
                "Added cookie sp=\"x y\" for domain www.example.com, path /, expire 0",
                "Added cookie x=\"1\" for domain www.example.com, path \"/dir/\", expire 0",
                "Added cookie y=\"1\" for domain www.example.com, path dir, expire 0",
                "Added cookie z=\"1\" for domain www.example.com, path , expire 0",
                "Added cookie q=\"1\" for domain www.example.com, path /dir/, expire 0",
                "Added cookie r=\"1\" for domain WWW.example.COM, path /dir/, expire 0",
            ]
        );
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                "www.example.com\tFALSE\t/dir/\tFALSE\t0\ta\t1",
                "www.example.com\tFALSE\t/\tFALSE\t0\tb\t2",
                ".example.com\tTRUE\t/dir/\tFALSE\t0\tc\t3",
                "www.example.com\tFALSE\t/dir/sub\tFALSE\t0\taa\t6",
                "www.example.com\tFALSE\t/\tFALSE\t0\ta\t7",
                ".www.example.com\tTRUE\t/\tFALSE\t0\ta\t8",
                "#HttpOnly_www.example.com\tFALSE\t/\tFALSE\t0\tsp\tx y",
                "www.example.com\tFALSE\t\"/dir/\"\tFALSE\t0\tx\t1",
                "www.example.com\tFALSE\tdir\tFALSE\t0\ty\t1",
                "www.example.com\tFALSE\t\tFALSE\t0\tz\t1",
                "www.example.com\tFALSE\t/dir/\tFALSE\t0\tq\t1",
                ".WWW.example.COM\tTRUE\t/dir/\tFALSE\t0\tr\t1",
            ]
        );
        log.clear();
        let h = |ci: &mut CookieInfo, host, path, add, log: &mut InfoLog| {
            header(ci, host, path, add, log)
        };
        assert_eq!(
            h(&mut ci, "www.example.com", "/dir/page.html", None, &mut log),
            "Cookie: x=1; r=1; q=1; a=1; c=3; y=1; sp=x y; a=8; a=7; b=2; z=1\r\n"
        );
        assert_eq!(
            h(&mut ci, "www.example.com", "/dir/sub/x", None, &mut log),
            "Cookie: aa=6; x=1; r=1; q=1; a=1; c=3; y=1; sp=x y; a=8; a=7; b=2; z=1\r\n"
        );
        assert_eq!(h(&mut ci, "a.b.example.com", "/", None, &mut log), "");
        assert_eq!(
            h(&mut ci, "www.example.com", "/", None, &mut log),
            "Cookie: y=1; sp=x y; a=8; a=7; b=2; z=1\r\n"
        );
        response(
            &mut ci,
            "www.example.com",
            "/",
            &[" c=33; domain=example.com", " b=22; path=/"],
            &mut log,
        );
        assert_eq!(
            s(&log),
            [
                "Added cookie c=\"33\" for domain example.com, path /, expire 0",
                "Replaced cookie b=\"22\" for domain www.example.com, path /, expire 0",
            ]
        );
        log.clear();
        assert_eq!(
            h(&mut ci, "www.example.com", "/dir/", None, &mut log),
            "Cookie: x=1; r=1; q=1; a=1; c=3; y=1; sp=x y; a=8; a=7; b=22; c=33; z=1\r\n"
        );
        let extra = Some("extra=1; more=2");
        assert_eq!(
            h(&mut ci, "www.example.com", "/dir/", extra, &mut log),
            "Cookie: x=1; r=1; q=1; a=1; c=3; y=1; sp=x y; a=8; a=7; b=22; c=33; z=1; extra=1; more=2\r\n"
        );
        assert_eq!(
            h(&mut ci, "example.co.uk", "/", extra, &mut log),
            "Cookie: extra=1; more=2\r\n"
        );
        response(
            &mut ci,
            "example.co.uk",
            "/",
            &[" uk=1; domain=co.uk", " uk2=1; domain=example.co.uk"],
            &mut log,
        );
        response(
            &mut ci,
            "localhost",
            "/",
            &[
                " sec=1; secure",
                " __Secure-s=1; secure",
                " __Host-h=1; secure; path=/",
                " lh=1; domain=localhost",
            ],
            &mut log,
        );
        assert_eq!(
            h(&mut ci, "localhost", "/", extra, &mut log),
            "Cookie: __Secure-s=1; __Host-h=1; sec=1; lh=1; extra=1; more=2\r\n"
        );
        response(
            &mut ci,
            "127.0.0.1",
            "/a/b",
            &[
                " ip=1",
                " ip2=1; domain=127.0.0.1",
                " ip3=1; domain=0.0.1",
                " sec=1; secure",
            ],
            &mut log,
        );
        assert_eq!(
            h(&mut ci, "127.0.0.1", "/a/c", extra, &mut log),
            "Cookie: sec=1; ip2=1; ip=1; extra=1; more=2\r\n"
        );
        assert_eq!(
            s(&log),
            [
                "Added cookie uk=\"1\" for domain co.uk, path /, expire 0",
                "Added cookie uk2=\"1\" for domain example.co.uk, path /, expire 0",
                "Added cookie sec=\"1\" for domain localhost, path /, expire 0",
                "Added cookie __Secure-s=\"1\" for domain localhost, path /, expire 0",
                "Added cookie __Host-h=\"1\" for domain localhost, path /, expire 0",
                "Added cookie lh=\"1\" for domain localhost, path /, expire 0",
                "Added cookie ip=\"1\" for domain 127.0.0.1, path /a/, expire 0",
                "Added cookie ip2=\"1\" for domain 127.0.0.1, path /a/, expire 0",
                "skipped cookie with bad tailmatch domain: 0.0.1\r\n",
                "Added cookie sec=\"1\" for domain 127.0.0.1, path /a/, expire 0",
            ]
        );
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                "127.0.0.1\tFALSE\t/a/\tFALSE\t0\tip\t1",
                "127.0.0.1\tFALSE\t/a/\tFALSE\t0\tip2\t1",
                "127.0.0.1\tFALSE\t/a/\tTRUE\t0\tsec\t1",
                ".co.uk\tTRUE\t/\tFALSE\t0\tuk\t1",
                ".example.co.uk\tTRUE\t/\tFALSE\t0\tuk2\t1",
                "localhost\tFALSE\t/\tTRUE\t0\tsec\t1",
                "localhost\tFALSE\t/\tTRUE\t0\t__Secure-s\t1",
                "localhost\tFALSE\t/\tTRUE\t0\t__Host-h\t1",
                ".localhost\tTRUE\t/\tFALSE\t0\tlh\t1",
                "www.example.com\tFALSE\t/dir/\tFALSE\t0\ta\t1",
                ".example.com\tTRUE\t/dir/\tFALSE\t0\tc\t3",
                "www.example.com\tFALSE\t/dir/sub\tFALSE\t0\taa\t6",
                "www.example.com\tFALSE\t/\tFALSE\t0\ta\t7",
                ".www.example.com\tTRUE\t/\tFALSE\t0\ta\t8",
                "#HttpOnly_www.example.com\tFALSE\t/\tFALSE\t0\tsp\tx y",
                "www.example.com\tFALSE\t\"/dir/\"\tFALSE\t0\tx\t1",
                "www.example.com\tFALSE\tdir\tFALSE\t0\ty\t1",
                "www.example.com\tFALSE\t\tFALSE\t0\tz\t1",
                "www.example.com\tFALSE\t/dir/\tFALSE\t0\tq\t1",
                ".WWW.example.COM\tTRUE\t/dir/\tFALSE\t0\tr\t1",
                ".example.com\tTRUE\t/\tFALSE\t0\tc\t33",
                "www.example.com\tFALSE\t/\tFALSE\t0\tb\t22",
            ]
        );
        // no engine: only CURLOPT_COOKIE
        let hdr = http_cookie_header(
            Some(&mut ci),
            false,
            b"localhost",
            b"/",
            true,
            extra.map(str::as_bytes),
            &mut log,
        );
        assert_eq!(hdr, b"Cookie: extra=1; more=2\r\n");
        let hdr = http_cookie_header(None, false, b"localhost", b"/", true, None, &mut log);
        assert!(hdr.is_empty());
        let hdr = http_cookie_header(None, false, b"x", b"/", true, Some(b""), &mut log);
        assert_eq!(hdr, b"Cookie: \r\n");
    }

    const CF1: &[u8] = b"# Netscape HTTP Cookie File\n\n.example.com\tTRUE\t/\tFALSE\t0\tsess\t1\r\nwww.example.com\tFALSE\t/\tFALSE\t2000000000\tperm\t2\n#HttpOnly_example.com\tFALSE\t/p\tTRUE\t2000000000\thttp\t3\nexample.com\tFALSE\tFALSE\t2000000000\tnopath\t4\nexample.com\tFALSE\t/\tFALSE\t2000000000\tblank\nexample.com\tFALSE\t/\tFALSE\t100\texpired\t5\nexample.com\tFALSE\t/\tFALSE\tabc\tbadexp\t6\nexample.com\tFALSE\t/\tFALSE\t2000000000\textra\t7\t8\nset-cookie:   fromhdr=9; domain=example.com; path=/\nSet-Cookie: hsec=1; secure; domain=example.com\nSet-Cookie: hsess=1\n  \t\nexample.com\tTRUE\t\tTRUE\t2000000000\temptypath\t10\nexample.com\t\t/\t\t2000000000\temptyflags\t11\n.TRUE\tTR\t/\tT\t2000000000\tpartial\t12\nexample.com\tFALSE\t/\tFALSE\t-5\tneg\t13\nexample.com\tFALSE\t/\tFALSE\t99999999999999999999\tbig\t14\nexample.com\tFALSE\t/\tFALSE\t2000000000\t__Secure-ns\t15\nexample.com\tFALSE\t/\tTRUE\t2000000000\t__secure-ns2\t16\nexample.com\tFALSE\t/\tTRUE\t2000000000\t__host-ns3\t17\nlast\tFALSE\t/\tFALSE\t2000000000\tnonl\t18";

    fn tmpfile(name: &str, content: &[u8]) -> Vec<u8> {
        let mut p = std::env::temp_dir();
        p.push(format!("curl-cookie-test-{}-{}", std::process::id(), name));
        std::fs::write(&p, content).unwrap();
        p.to_str().unwrap().as_bytes().to_vec()
    }

    #[test]
    fn file_loading() {
        let f = tmpfile("cf1", CF1);
        let mut c = CookieInfo::new();
        c.fixed_now = Some(1792611848);
        let mut cookies = Some(c);
        let mut log = InfoLog::new();
        let mut sc = 0u8;
        let files = vec![f.clone()];
        // CURLOPT_COOKIEFILE + COOKIELIST RELOAD
        load_files(&mut cookies, &files, false, &mut sc, &mut log);
        let ci = cookies.as_mut().unwrap();
        assert!(log.is_empty());
        let all = [
            ".TRUE\tTRUE\t/\tTRUE\t2000000000\tpartial\t12",
            "last\tFALSE\t/\tFALSE\t2000000000\tnonl\t18",
            ".example.com\tTRUE\t/\tFALSE\t0\tsess\t1",
            "www.example.com\tFALSE\t/\tFALSE\t2000000000\tperm\t2",
            "#HttpOnly_example.com\tFALSE\t/p\tTRUE\t2000000000\thttp\t3",
            "example.com\tFALSE\t/\tFALSE\t2000000000\tnopath\t4",
            "example.com\tFALSE\t/\tFALSE\t2000000000\tblank\t",
            ".example.com\tTRUE\t/\tFALSE\t0\tfromhdr\t9",
            ".example.com\tTRUE\t/\tTRUE\t0\thsec\t1",
            ".example.com\tTRUE\t/\tTRUE\t2000000000\temptyflags\t11",
            "example.com\tFALSE\t/\tTRUE\t2000000000\t__secure-ns2\t16",
            "example.com\tFALSE\t/\tTRUE\t2000000000\t__host-ns3\t17",
        ];
        assert_eq!(s(&ci.netscape_lines()), all);
        ci.clearsess();
        let nosess: Vec<&str> = all
            .iter()
            .copied()
            .filter(|l| !l.contains("\t0\t"))
            .collect();
        assert_eq!(s(&ci.netscape_lines()), nosess);
        ci.clearall();
        assert!(ci.netscape_lines().is_empty());
        // CURLOPT_COOKIESESSION then RELOAD: session cookies are skipped
        load_files(&mut cookies, &files, true, &mut sc, &mut log);
        let ci = cookies.as_mut().unwrap();
        assert_eq!(s(&ci.netscape_lines()), nosess);
        // live cookies win over file cookies
        cl(
            ci,
            &mut sc,
            "Set-Cookie: live=1; domain=example.com; path=/",
            &mut log,
        );
        cl(
            ci,
            &mut sc,
            "example.com\tTRUE\t/\tFALSE\t0\tlive\tfromfile",
            &mut log,
        );
        cl(
            ci,
            &mut sc,
            "example.com\tTRUE\t/\tFALSE\t0\tfromhdr\toverwritten",
            &mut log,
        );
        assert_eq!(
            s(&log),
            [
                "Added cookie live=\"1\" for domain example.com, path /, expire 0",
                "Replaced cookie live=\"fromfile\" for domain example.com, path /, expire 0",
                "Added cookie fromhdr=\"overwritten\" for domain example.com, path /, expire 0",
            ]
        );
        load_files(&mut cookies, &files, true, &mut sc, &mut log);
        let ci = cookies.as_mut().unwrap();
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                ".TRUE\tTRUE\t/\tTRUE\t2000000000\tpartial\t12",
                "last\tFALSE\t/\tFALSE\t2000000000\tnonl\t18",
                ".example.com\tTRUE\t/\tFALSE\t0\tlive\tfromfile",
                ".example.com\tTRUE\t/\tFALSE\t0\tfromhdr\toverwritten",
                "www.example.com\tFALSE\t/\tFALSE\t2000000000\tperm\t2",
                "#HttpOnly_example.com\tFALSE\t/p\tTRUE\t2000000000\thttp\t3",
                "example.com\tFALSE\t/\tFALSE\t2000000000\tnopath\t4",
                "example.com\tFALSE\t/\tFALSE\t2000000000\tblank\t",
                ".example.com\tTRUE\t/\tTRUE\t2000000000\temptyflags\t11",
                "example.com\tFALSE\t/\tTRUE\t2000000000\t__secure-ns2\t16",
                "example.com\tFALSE\t/\tTRUE\t2000000000\t__host-ns3\t17",
            ]
        );
        // write the jar like curl_easy_cleanup() does
        let out = tmpfile("jar3", b"old content");
        let mut log = InfoLog::new();
        flush_cookies(Some(&out), cookies.as_mut(), &mut log);
        assert!(log.is_empty());
        let written = std::fs::read(bytes_to_path(&out)).unwrap();
        assert_eq!(
            String::from_utf8(written).unwrap(),
            format!(
                "{JAR_HEAD}.example.com\tTRUE\t/\tFALSE\t0\tfromhdr\toverwritten\n\
.example.com\tTRUE\t/\tFALSE\t0\tlive\tfromfile\n\
last\tFALSE\t/\tFALSE\t2000000000\tnonl\t18\n\
example.com\tFALSE\t/\tTRUE\t2000000000\t__host-ns3\t17\n\
example.com\tFALSE\t/\tTRUE\t2000000000\t__secure-ns2\t16\n\
.TRUE\tTRUE\t/\tTRUE\t2000000000\tpartial\t12\n\
.example.com\tTRUE\t/\tTRUE\t2000000000\temptyflags\t11\n\
example.com\tFALSE\t/\tFALSE\t2000000000\tblank\t\n\
example.com\tFALSE\t/\tFALSE\t2000000000\tnopath\t4\n\
#HttpOnly_example.com\tFALSE\t/p\tTRUE\t2000000000\thttp\t3\n\
www.example.com\tFALSE\t/\tFALSE\t2000000000\tperm\t2\n"
            )
        );
        let _ = std::fs::remove_file(bytes_to_path(&out));
        let _ = std::fs::remove_file(bytes_to_path(&f));
        // a jar in a missing directory
        let mut log = InfoLog::new();
        flush_cookies(
            Some(b"/nonexistent-dir-xyz/jar"),
            cookies.as_mut(),
            &mut log,
        );
        assert_eq!(
            s(&log),
            [
                "WARNING: failed to save cookies in /nonexistent-dir-xyz/jar: Failed writing received data to disk/application"
            ]
        );
        // no jar at all: nothing written, no error
        assert_eq!(cookie_output(None, b"/nonexistent-dir-xyz/jar"), Ok(()));
    }

    #[test]
    fn file_lines_limits() {
        let mut f6 = Vec::new();
        f6.extend_from_slice(b"example.com\tFALSE\t/\tFALSE\t0\tlong4999\t");
        f6.extend(std::iter::repeat_n(b'x', 4999 - 38));
        f6.push(b'\n');
        f6.extend_from_slice(b"example.com\tFALSE\t/\tFALSE\t0\tcrlf\t1\r\n");
        f6.extend_from_slice(b"Set-Cookie: hdr=1; domain=1.2.3.4\n");
        f6.extend_from_slice(b"Set-Cookie:\thdr2=1; domain=example.com; path=/p; expires=Thu, 01 Jan 1971 00:00:00 GMT\n");
        f6.extend_from_slice(b"Set-Cookie: hdr3=1; domain=example.com; path=/p; max-age=1000\n");
        f6.extend_from_slice(b"example.com\tFALSE\t/\tFALSE\t0\tlong5000\t");
        f6.extend(std::iter::repeat_n(b'x', 5000 - 38));
        f6.push(b'\n');
        f6.extend_from_slice(b"example.com\tFALSE\t/\tFALSE\t0\tafter\t1\n");
        let mut f7 =
            b"example.com\tFALSE\t/\tFALSE\t0\tseven\t1\nexample.com\tFALSE\t/\tFALSE\t0\tnonl\t"
                .to_vec();
        f7.extend(std::iter::repeat_n(b'y', 127 - 34));
        let mut f8 =
            b"example.com\tFALSE\t/\tFALSE\t0\teight\t1\nexample.com\tFALSE\t/\tFALSE\t0\tnonl8\t"
                .to_vec();
        f8.extend(std::iter::repeat_n(b'y', 128 - 35));
        let files = vec![
            tmpfile("cf6", &f6),
            tmpfile("cf7", &f7),
            tmpfile("cf8", &f8),
            b"/nonexistent-dir-xyz/doesnotexist.txt".to_vec(),
        ];
        let mut cookies = Some(jar(1792611927));
        let mut sc = 0;
        let mut log = InfoLog::new();
        load_files(&mut cookies, &files, false, &mut sc, &mut log);
        assert_eq!(
            s(&log),
            ["WARNING: failed to open cookie file \"/nonexistent-dir-xyz/doesnotexist.txt\""]
        );
        let lines = s(&cookies.as_ref().unwrap().netscape_lines());
        let long = format!(
            "example.com\tFALSE\t/\tFALSE\t0\tlong4999\t{}",
            "x".repeat(4999 - 38)
        );
        let n7 = format!("example.com\tFALSE\t/\tFALSE\t0\tnonl\t{}", "y".repeat(93));
        let n8 = format!("example.com\tFALSE\t/\tFALSE\t0\tnonl8\t{}", "y".repeat(93));
        assert_eq!(
            lines,
            [
                ".1.2.3.4\tTRUE\t/\tFALSE\t0\thdr\t1",
                long.as_str(),
                "example.com\tFALSE\t/\tFALSE\t0\tcrlf\t1",
                ".example.com\tTRUE\t/p\tFALSE\t1792612927\thdr3\t1",
                "example.com\tFALSE\t/\tFALSE\t0\tseven\t1",
                n7.as_str(),
                "example.com\tFALSE\t/\tFALSE\t0\teight\t1",
                n8.as_str(),
            ]
        );
        for f in &files[..3] {
            let _ = std::fs::remove_file(bytes_to_path(f));
        }
        // NUL bytes: a NUL inside a line glues the following line to it, a
        // NUL at the start of a line ends the file
        let mut ci = jar(0);
        ci.running = false;
        ci.newsession = false;
        let mut c4 = b"example.com\tFALSE\t/\tFALSE\t0\tfirst\t1\nexample.com\tFALSE\t/\tFALSE\t0\tnul\ta\x00b\n".to_vec();
        c4.extend_from_slice(b"example.com\tFALSE\t/\tFALSE\t0\tlong4999\t");
        c4.extend(std::iter::repeat_n(b'x', 4999 - 37));
        c4.extend_from_slice(b"\nexample.com\tFALSE\t/\tFALSE\t0\tafter\t1\n");
        ci.load_bytes(&c4, &mut sc, &mut log);
        ci.load_bytes(b"example.com\tFALSE\t/\tFALSE\t0\tfirst5\t1\n\x00example.com\tFALSE\t/\tFALSE\t0\tnul0\t1\nexample.com\tFALSE\t/\tFALSE\t0\tafter5\t1\n", &mut sc, &mut log);
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                "example.com\tFALSE\t/\tFALSE\t0\tfirst\t1",
                "example.com\tFALSE\t/\tFALSE\t0\tfirst5\t1",
            ]
        );
    }

    #[test]
    fn limits() {
        let mut ci = jar(1792611878);
        let mut log = InfoLog::new();
        let hdrs: Vec<String> = (0..55).map(|i| format!(" n{i}=v{i}")).collect();
        let hdrs: Vec<&str> = hdrs.iter().map(|s| s.as_str()).collect();
        response(&mut ci, "www.example.com", "/", &hdrs, &mut log);
        assert_eq!(log.len(), 50);
        assert_eq!(ci.netscape_lines().len(), 50);
        // the per-request counter also blocks COOKIELIST lines
        let mut sc = 50u8;
        ci.clearall();
        cl(
            &mut ci,
            &mut sc,
            "Set-Cookie: cl0=1; domain=example.com",
            &mut log,
        );
        assert!(ci.netscape_lines().is_empty());
        // 150 cookies max per request
        for p in ["m", "k", "j"] {
            let hdrs: Vec<String> = (0..45).map(|i| format!(" {p}{i:03}=v")).collect();
            let hdrs: Vec<&str> = hdrs.iter().map(|s| s.as_str()).collect();
            response(&mut ci, "www.example.com", "/", &hdrs, &mut log);
        }
        let hdrs: Vec<String> = (0..25).map(|i| format!(" i{i:03}=v")).collect();
        let hdrs: Vec<&str> = hdrs.iter().map(|s| s.as_str()).collect();
        response(&mut ci, "www.example.com", "/", &hdrs, &mut log);
        log.clear();
        let h = header(&mut ci, "www.example.com", "/", None, &mut log);
        assert_eq!(
            s(&log),
            ["Included max number of cookies (150) in request!"]
        );
        let mut exp: Vec<String> = Vec::new();
        for i in (0..15).rev() {
            exp.push(format!("i{i:03}=v"));
        }
        for p in ["j", "k", "m"] {
            for i in (0..45).rev() {
                exp.push(format!("{p}{i:03}=v"));
            }
        }
        assert_eq!(h, format!("Cookie: {}\r\n", exp.join("; ")));
        // Cookie: header size cap, CURLOPT_COOKIE is then not added
        ci.clearall();
        let hdrs: Vec<String> = (0..3)
            .map(|i| format!(" big{i}={}", ((b'a' + i) as char).to_string().repeat(3000)))
            .collect();
        let hdrs: Vec<&str> = hdrs.iter().map(|s| s.as_str()).collect();
        response(&mut ci, "www.example.com", "/", &hdrs, &mut log);
        log.clear();
        let h = header(&mut ci, "www.example.com", "/", Some("extra=1"), &mut log);
        assert_eq!(
            s(&log),
            ["Restricted outgoing cookies due to header size, 'big0' not sent"]
        );
        assert_eq!(
            h,
            format!(
                "Cookie: big2={}; big1={}\r\n",
                "c".repeat(3000),
                "b".repeat(3000)
            )
        );
        // name/value sizes
        ci.clearall();
        log.clear();
        let mut sc = 0;
        for (n, v) in [(4094, 1), (4093, 1), (4000, 97), (4000, 96)] {
            cl(
                &mut ci,
                &mut sc,
                &format!("Set-Cookie: {}={}", "n".repeat(n), "v".repeat(v)),
                &mut log,
            );
        }
        cl(
            &mut ci,
            &mut sc,
            &format!("Set-Cookie: x={}", "v".repeat(4094)),
            &mut log,
        );
        cl(
            &mut ci,
            &mut sc,
            &format!("Set-Cookie: y=1; {}", "p".repeat(4990)),
            &mut log,
        );
        cl(
            &mut ci,
            &mut sc,
            &format!("Set-Cookie: y2=1; {}", "p".repeat(4986)),
            &mut log,
        );
        let l = s(&log);
        assert_eq!(l.len(), 7);
        assert!(l[0].starts_with("Added cookie nnnn"));
        assert!(l[1].starts_with("Added cookie nnnn"));
        assert_eq!(l[2], "oversized cookie dropped, name/val 4000 + 97 bytes");
        assert!(l[3].starts_with("Added cookie nnnn"));
        assert!(l[4].starts_with("Added cookie x=\"vvv"));
        assert_eq!(l[5], "oversized cookie dropped, name/val 4990 + 0 bytes");
        assert_eq!(l[6], "oversized cookie dropped, name/val 4986 + 0 bytes");
        // too long line
        log.clear();
        cl(
            &mut ci,
            &mut sc,
            &format!("Set-Cookie: z=1; {}", "p".repeat(5000)),
            &mut log,
        );
        assert!(log.is_empty());
    }

    #[test]
    fn secure_overlay() {
        let mut ci = jar(1792611927);
        let mut sc = 0;
        let mut log = InfoLog::new();
        cl(
            &mut ci,
            &mut sc,
            "foo.local\tFALSE\t/login\tTRUE\t0\ts\tsecure1",
            &mut log,
        );
        cl(
            &mut ci,
            &mut sc,
            ".foo.local\tTRUE\t/\tTRUE\t0\troot\tsecure2",
            &mut log,
        );
        cl(
            &mut ci,
            &mut sc,
            "foo.local\tFALSE\t/\tTRUE\t0\temp\tsecure3",
            &mut log,
        );
        log.clear();
        response(
            &mut ci,
            "foo.local",
            "/",
            &[
                " s=2; path=/login/en",
                " s=3; path=/loginhelper",
                " s=4; path=/Login",
                " s=5; path=/login",
                " root=x; path=/any",
                " root=y; domain=foo.local; path=/any",
                " emp=1; path=/zzz",
                " s=6; secure",
            ],
            &mut log,
        );
        let m = |n: &str| {
            format!("cookie '{n}' for domain 'foo.local' dropped, would overlay an existing cookie")
        };
        assert_eq!(
            s(&log),
            [
                m("s"),
                m("s"),
                m("s"),
                m("s"),
                m("root"),
                m("root"),
                m("emp")
            ]
        );
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                "foo.local\tFALSE\t/login\tTRUE\t0\ts\tsecure1",
                ".foo.local\tTRUE\t/\tTRUE\t0\troot\tsecure2",
                "foo.local\tFALSE\t/\tTRUE\t0\temp\tsecure3",
            ]
        );
        // with a secure context (custom Host: localhost) the overlay is
        // allowed, and paths compare case-insensitively when replacing
        let mut ci = jar(1792611878);
        response(
            &mut ci,
            "localhost",
            "/login",
            &[" s=1; secure; path=/login", " t=1; secure; path=/"],
            &mut log,
        );
        log.clear();
        assert_eq!(
            header(&mut ci, "foo.local", "/", Some(""), &mut log),
            "Cookie: \r\n"
        );
        response(
            &mut ci,
            "foo.local",
            "/",
            &[" s=2; domain=localhost; path=/login/en"],
            &mut log,
        );
        assert_eq!(
            header(&mut ci, "localhost", "/", Some(""), &mut log),
            "Cookie: t=1; \r\n"
        );
        response(
            &mut ci,
            "localhost",
            "/",
            &[
                " s=2; path=/login/en",
                " s=3; path=/loginhelper",
                " s=4; path=/Login",
                " t=2; path=/foo",
                " u=1",
            ],
            &mut log,
        );
        assert_eq!(
            s(&log),
            [
                "skipped cookie with bad tailmatch domain: localhost; path=/login/en\r\n",
                "Added cookie s=\"2\" for domain localhost, path /login/en, expire 0",
                "Added cookie s=\"3\" for domain localhost, path /loginhelper, expire 0",
                "Replaced cookie s=\"4\" for domain localhost, path /Login, expire 0",
                "Added cookie t=\"2\" for domain localhost, path /foo, expire 0",
                "Added cookie u=\"1\" for domain localhost, path /, expire 0",
            ]
        );
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                "localhost\tFALSE\t/\tTRUE\t0\tt\t1",
                "localhost\tFALSE\t/login/en\tFALSE\t0\ts\t2",
                "localhost\tFALSE\t/loginhelper\tFALSE\t0\ts\t3",
                "localhost\tFALSE\t/Login\tFALSE\t0\ts\t4",
                "localhost\tFALSE\t/foo\tFALSE\t0\tt\t2",
                "localhost\tFALSE\t/\tFALSE\t0\tu\t1",
            ]
        );
    }

    #[test]
    fn cookiehost() {
        let c = |h: &str| {
            cookiehost_from_host_header(h.as_bytes()).map(|v| String::from_utf8(v).unwrap())
        };
        assert_eq!(c("Host: localhost:99").as_deref(), Some("localhost"));
        assert_eq!(c("Host: [::1]:99").as_deref(), Some("::1"));
        assert_eq!(c("Host: [abc").as_deref(), Some("abcc"));
        assert_eq!(
            c("Host:   spaced.example.com  ").as_deref(),
            Some("spaced.example.com")
        );
        assert_eq!(c("Host:").as_deref(), None);
        assert_eq!(c("Host;").as_deref(), None);
        assert!(secure_context(false, b"LocalHost"));
        assert!(secure_context(false, b"::1"));
        assert!(!secure_context(false, b"127.0.0.2"));
        assert!(secure_context(true, b"x"));
    }

    #[test]
    fn misc_header_parsing() {
        let now = 1792611927;
        let mut ci = jar(now);
        let mut sc = 0;
        let mut log = InfoLog::new();
        for l in [
            "Set-Cookie: v6=1; domain=::1",
            "Set-Cookie: v6=1; domain=[::1]",
            "Set-Cookie: dot=1; domain=.",
            "Set-Cookie: dot2=1; domain=example.com.",
            "Set-Cookie: q=1; path=\"/a b/\"; Max-Age=\"100\"x; Secure=; HttpOnly=; Version=1; foo=bar",
            "Set-Cookie: q2=1; expires=Wed, 09 Jun 2027 10:18:14 GMT; max-age=10",
            "Set-Cookie: q3=1; max-age=; expires=; secure=yes; path=/x; path=/y/",
            "Set-Cookie: q4=  spaced value  ;   ; ;domain=example.com",
            "Set-Cookie: =novalname",
            "Set-Cookie: q5=\"quoted\"",
            "Set-Cookie: a\tb=1",
            "Set-Cookie:q6=1;secure;httponly",
            "SET-COOKIE: q7=1",
            "Set-Cookie: q8=1; path=/\r\nSet-Cookie: q9=1",
            "Set-Cookie: q10=a\x7fb",
            "Set-Cookie: q12=1; domain=example.com; domain=www.example.com",
            "Set-Cookie: q13=1; dOmAiN=Example.Com; PATH=/",
            "Set-Cookie: q12=2; domain=www.example.com",
            ".example.com\tTRUE\t/\tFALSE\t0\tq12\t3",
            "www.example.com\tTRUE\t/\tFALSE\t0\tq12\t4",
        ] {
            cl(&mut ci, &mut sc, l, &mut log);
        }
        assert_eq!(
            s(&log),
            [
                "skipped cookie with bad tailmatch domain: ::1",
                "skipped cookie with bad tailmatch domain: [::1]",
                "skipped cookie with bad tailmatch domain: ",
                "Added cookie dot2=\"1\" for domain example.com., path (nil), expire 0",
                "Added cookie q=\"1\" for domain (nil), path \"/a b/\", expire 1792612027",
                "Added cookie q2=\"1\" for domain (nil), path (nil), expire 1792611937",
                "Added cookie q3=\"1\" for domain (nil), path /y/, expire 0",
                "Added cookie q4=\"spaced value\" for domain example.com, path (nil), expire 0",
                "Added cookie =\"novalname\" for domain (nil), path (nil), expire 0",
                "Added cookie q5=\"\"quoted\"\" for domain (nil), path (nil), expire 0",
                "Added cookie q6=\"1\" for domain (nil), path (nil), expire 0",
                "Added cookie q7=\"1\" for domain (nil), path (nil), expire 0",
                "Added cookie q8=\"1\" for domain (nil), path /, expire 0",
                "invalid octets in name/value, cookie dropped",
                "Added cookie q12=\"1\" for domain www.example.com, path (nil), expire 0",
                "Added cookie q13=\"1\" for domain Example.Com, path /, expire 0",
                "Replaced cookie q12=\"2\" for domain www.example.com, path (nil), expire 0",
                "Added cookie q12=\"3\" for domain example.com, path /, expire 0",
                "Added cookie q12=\"4\" for domain www.example.com, path /, expire 0",
            ]
        );
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                ".example.com.\tTRUE\t/\tFALSE\t0\tdot2\t1",
                ".example.com\tTRUE\t/\tFALSE\t0\tq4\tspaced value",
                ".Example.Com\tTRUE\t/\tFALSE\t0\tq13\t1",
                ".www.example.com\tTRUE\t/\tFALSE\t0\tq12\t2",
                ".example.com\tTRUE\t/\tFALSE\t0\tq12\t3",
                ".www.example.com\tTRUE\t/\tFALSE\t0\tq12\t4",
            ]
        );
        assert_eq!(
            header(&mut ci, "www.example.com", "/", None, &mut log),
            "Cookie: q12=4; q12=3; q13=1; q12=2; q4=spaced value\r\n"
        );
        // non-UTF-8 bytes are fine
        log.clear();
        ci.add_cookielist_line(&mut sc, b"Set-Cookie: q11=a\x80b", &mut log);
        assert_eq!(
            log,
            [b"Added cookie q11=\"a\x80b\" for domain (nil), path (nil), expire 0".to_vec()]
        );
    }

    #[test]
    fn hashing_and_ip() {
        assert!(host_is_ipnum(b"1.2.3.4"));
        assert!(!host_is_ipnum(b"1.2.3.4\n"));
        assert!(!host_is_ipnum(b"01.2.3.4"));
        assert!(!host_is_ipnum(b"1.2.3"));
        assert!(!host_is_ipnum(b"1.2.3.256"));
        assert!(host_is_ipnum(b"::1"));
        assert!(host_is_ipnum(b"::"));
        assert!(host_is_ipnum(b"::ffff:1.2.3.4"));
        assert!(host_is_ipnum(b"1:2:3:4:5:6:7:8"));
        assert!(!host_is_ipnum(b"1:2:3:4:5:6:7:8:9"));
        assert!(!host_is_ipnum(b"1:2:3:4:5:6:7::8"));
        assert!(!host_is_ipnum(b"[::1]"));
        assert!(!host_is_ipnum(b":"));
        assert!(!host_is_ipnum(b"12345::"));
        let pton: &[(&str, bool)] = &[
            ("0.0.0.0", true),
            ("1..2.3", false),
            ("1:2:3:4:5:6:1.2.3.4", true),
            ("1:2:3:4:5:6:7:1.2.3.4", false),
            ("::1.2.3.4", true),
            ("1::", true),
            ("1:::2", false),
            ("00000::1", false),
            ("0000::1", true),
            ("::1:", false),
        ];
        for (h, want) in pton {
            assert_eq!(host_is_ipnum(h.as_bytes()), *want, "{h}");
        }
        assert_eq!(cookiehash(Some(b"1.2.3.4")), 0);
        assert_eq!(cookiehash(None), 0);
        assert_eq!(get_top_domain(b"a.b.example.com"), b"example.com");
        assert_eq!(get_top_domain(b"example.com"), b"example.com");
        assert_eq!(get_top_domain(b"com"), b"com");
        assert_eq!(
            cookiehash(Some(b"www.example.com")),
            cookiehash(Some(b"EXAMPLE.com"))
        );
    }

    #[test]
    fn expiry_handling() {
        let mut ci = jar(1000);
        let mut sc = 0;
        let mut log = InfoLog::new();
        cl(
            &mut ci,
            &mut sc,
            "Set-Cookie: a=1; domain=example.com; max-age=10",
            &mut log,
        );
        cl(
            &mut ci,
            &mut sc,
            "Set-Cookie: b=1; domain=example.com",
            &mut log,
        );
        assert_eq!(ci.next_expiration, 1010);
        ci.fixed_now = Some(1011);
        // CURLINFO_COOKIELIST does not expire anything
        assert_eq!(ci.netscape_lines().len(), 2);
        assert_eq!(
            header(&mut ci, "example.com", "/", None, &mut log),
            "Cookie: b=1\r\n"
        );
        assert_eq!(ci.netscape_lines().len(), 1);
        assert_eq!(ci.numcookies, 1);
        assert_eq!(ci.next_expiration, i64::MAX);
    }

    #[test]
    fn more_attributes() {
        let mut ci = jar(1792612511);
        let mut sc = 0;
        let mut log = InfoLog::new();
        let lines: [&[u8]; 14] = [
            b"Set-Cookie: q11=a\x80b",
            b"Set-Cookie: q12=1; domain=.; path=/",
            b"Set-Cookie: q13=1; domain=1.2.3.4; secure",
            b"Set-Cookie:  q14 = 1 ; secure",
            b"Set-Cookie: q15=1; secure; httponly; domain=example.com",
            b"Set-Cookie: __Secure-q16=1; Secure; __Host-x=2",
            b"Set-Cookie: q17=1; Secure; __Host-x=2; path=/",
            b".exAmple.com\tfalse\t/\tfalse\t0\tq18\t1",
            b"Set-Cookie: q19=1; domain=example.com; max-age=\"5\"",
            b"Set-Cookie: q20=1; domain=example.com; max-age=0x10",
            b"Set-Cookie: q21=1; domain=example.com; expires=Wed, 09 Jun 2027 10:18:14 GMT; expires=Wed, 09 Jun 2026 10:18:14 GMT",
            b"Set-Cookie: q22=1; domain=example.com; expires=Wed, 09 Jun 2027 10:18:14 GMT; max-age=abc",
            b"Set-Cookie: q23=; domain=example.com",
            b"Set-Cookie: q24; domain=example.com",
        ];
        for l in lines {
            assert_eq!(parse_cookielist(l), CookieListCommand::Line(l));
            ci.add_cookielist_line(&mut sc, l, &mut log);
        }
        let want: [&[u8]; 12] = [
            b"Added cookie q11=\"a\x80b\" for domain (nil), path (nil), expire 0",
            b"skipped cookie with bad tailmatch domain: ; path=/",
            b"Added cookie q13=\"1\" for domain 1.2.3.4, path (nil), expire 0",
            b"Added cookie q14=\"1\" for domain (nil), path (nil), expire 0",
            b"Added cookie q15=\"1\" for domain example.com, path (nil), expire 0",
            b"Added cookie q17=\"1\" for domain (nil), path /, expire 0",
            b"Added cookie q18=\"1\" for domain exAmple.com, path /, expire 0",
            b"Added cookie q19=\"1\" for domain example.com, path (nil), expire 1792612516",
            b"Added cookie q20=\"1\" for domain example.com, path (nil), expire 1",
            b"Added cookie q21=\"1\" for domain example.com, path (nil), expire 1812536294",
            b"Added cookie q22=\"1\" for domain example.com, path (nil), expire 1",
            b"Added cookie q23=\"\" for domain example.com, path (nil), expire 0",
        ];
        assert_eq!(log, want);
        // q20 and q22 (expires 1) were expired by the following adds
        assert_eq!(
            s(&ci.netscape_lines()),
            [
                ".1.2.3.4\tTRUE\t/\tTRUE\t0\tq13\t1",
                "#HttpOnly_.example.com\tTRUE\t/\tTRUE\t0\tq15\t1",
                "exAmple.com\tFALSE\t/\tFALSE\t0\tq18\t1",
                ".example.com\tTRUE\t/\tFALSE\t1792612516\tq19\t1",
                ".example.com\tTRUE\t/\tFALSE\t1812536294\tq21\t1",
                ".example.com\tTRUE\t/\tFALSE\t0\tq23\t",
            ]
        );
        assert_eq!(
            String::from_utf8(ci.jar_contents()).unwrap(),
            format!(
                "{JAR_HEAD}.example.com\tTRUE\t/\tFALSE\t0\tq23\t\n\
.example.com\tTRUE\t/\tFALSE\t1812536294\tq21\t1\n\
.example.com\tTRUE\t/\tFALSE\t1792612516\tq19\t1\n\
exAmple.com\tFALSE\t/\tFALSE\t0\tq18\t1\n\
#HttpOnly_.example.com\tTRUE\t/\tTRUE\t0\tq15\t1\n\
.1.2.3.4\tTRUE\t/\tTRUE\t0\tq13\t1\n"
            )
        );
        // keywords
        assert_eq!(parse_cookielist(b"all"), CookieListCommand::All);
        assert_eq!(parse_cookielist(b"Sess"), CookieListCommand::Sess);
        assert_eq!(parse_cookielist(b"flush"), CookieListCommand::Flush);
        assert_eq!(parse_cookielist(b"RELOAD"), CookieListCommand::Reload);
        // an empty jar still gets the header
        let mut e = CookieInfo::new_initialized(true);
        assert_eq!(String::from_utf8(e.jar_contents()).unwrap(), JAR_HEAD);
        assert!(e.netscape_lines().is_empty());
    }

    #[test]
    fn dates() {
        let cases: &[(&str, i64)] = &[
            ("Sun, 06 Nov 1994 08:49:37 GMT", 784111777),
            ("Sunday, 06-Nov-94 08:49:37 GMT", 784111777),
            ("Sun Nov  6 08:49:37 1994", 784111777),
            ("06 Nov 1994 08:49:37 GMT", 784111777),
            ("06-Nov-94 08:49:37 GMT", 784111777),
            ("Nov  6 08:49:37 1994", 784111777),
            ("06 Nov 1994 08:49:37", 784111777),
            ("1994 Nov 6 08:49:37", 784111777),
            ("GMT 08:49:37 06-Nov-94 Sunday", 784111777),
            ("94 6 Nov 08:49:37", 784111777),
            ("1994 Nov 6", 784080000),
            ("06-Nov-94", 784080000),
            ("Sun Nov 6 94", 784080000),
            ("1994.Nov.6", 784080000),
            ("Sun/Nov/6/94/GMT", 784080000),
            ("Sun, 06 Nov 1994 08:49:37 CET", 784108177),
            ("06 Nov 1994 08:49:37 EST", 784129777),
            ("Sun, 12 Sep 2004 15:05:58 -0700", 1095026758),
            ("Sat, 11 Sep 2004 21:32:11 +0200", 1094931131),
            ("20040912 15:05:58 -0700", 1095026758),
            ("20040911 +0200", 1094853600),
            ("Thu, 01 Jan 1970 00:00:00 GMT", 0),
            ("Wed, 31 Dec 1969 23:59:59 GMT", 0),
            ("Thu, 01 Jan 1970 00:00:00 +0001", -60),
            ("1583 Jan 1", -12212553600),
            ("1582 Dec 31", -1),
            ("99999999", -1),
            ("20041300", -1),
            ("20040100", 1072828800),
            ("20040132", -1),
            ("20040231", 1078185600),
            ("Feb 31 2004", 1078185600),
            ("0 Jan 2004", -1),
            ("32 Jan 2004", -1),
            ("Jan 2004 1", 1072915200),
            ("2004 1 Jan", 1072915200),
            ("Mon, 31 Dec 2007 25:00:00 GMT", -1),
            ("Mon, 31 Dec 2007 23:60:00 GMT", -1),
            ("Mon, 31 Dec 2007 23:59:60 GMT", 1199145600),
            ("Mon, 31 Dec 2007 23:59:61 GMT", -1),
            ("12:30 Jan 1 2000", 946729800),
            ("1:2:3 Jan 1 2000", 946688523),
            ("1:2 Jan 1 2000", 946688520),
            ("Jan 1 2000 1:2:3:4", -1),
            ("Wednesdayy Jan 1 2000", -1),
            ("Thursday Jan 1 2000", 946684800),
            ("abcdefghijkl Jan 1 2000", -1),
            ("Jan 1 2000 J", -1),
            ("Jan 1 2000 z", 946684800),
            ("Jan 1 2000 MEST", 946677600),
            ("Jan 1 2000 +1400", 946634400),
            ("Jan 1 2000 +1401", -1),
            ("Jan 1 2000 -0000", 946684800),
            ("Jan 1 2000 +130", -1),
            ("Jan 1 2000 GMT+0100", -1),
            ("01 Jan 70 00:00:00", 3155760000),
            ("01 Jan 71", 31536000),
            ("01 Jan 69", 3124224000),
            ("01 Jan 00", 946684800),
            ("Jan 01 100", -1),
            ("", -1),
            ("garbage", -1),
            ("Jan Feb 1 2000", -1),
            ("Mon Tue Jan 1 2000", -1),
            ("1 2 3 Jan", -1),
            ("Jan 1 2000 1 2", -1),
            ("000000001 Jan 2000", 946684800),
            ("Jan 1 2000 extra", -1),
            ("Fri, 31 Dec 9999 23:59:59 GMT", 253402300799),
            ("19990101 1", -1),
            ("Tue, 19 Jan 2038 03:14:08 GMT", 2147483648),
            ("Jan 1 2000 a b c d e f", -1),
            ("2000-01-02T03:04:05Z", -1),
            ("2000-01-02 03:04:05", -1),
            ("Jan\t1\t2000", 946684800),
            ("  Jan 1 2000  ", 946684800),
            ("Sat, 01-Jan-2000 08:00:00 UT", 946713600),
            ("Sat, 01-Jan-2000 08:00:00 AHST", 946749600),
            ("1 Jan 2000 1:2:3 4", -1),
            ("6 Nov 1994 8:49:37", 784111777),
            ("1994-11-06", -1),
            ("11/06/1994", -1),
            ("Jan 1 99999999", 3155633001244800),
            ("Jan 1 2000 GMT EST", -1),
        ];
        for (d, want) in cases {
            assert_eq!(curl_getdate(d.as_bytes()), *want, "date {d:?}");
            assert_eq!(getdate_capped(d.as_bytes(), 0), *want, "date {d:?}");
        }
        // NUL ends the string
        assert_eq!(curl_getdate(b"Jan 1 2000\0 garbage"), 946684800);
    }
}
