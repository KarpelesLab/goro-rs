//! `Uri\Rfc3986\Uri`: strict RFC 3986 URI references.
//!
//! Parsing accepts exactly the `URI-reference` grammar of RFC 3986
//! (section 4.1, appendix A); everything else is "The specified URI is
//! malformed". Raw getters return the components as written; the
//! normalized ones apply RFC 3986 section 6.2.2 the way PHP 8.5 does
//! (observed on PHP 8.5, see `tests/fixtures/rfc3986_driver.php`):
//!
//! - scheme and host are lowercased; percent-encoded unreserved characters
//!   are decoded in every component and the other percent-encodings get
//!   uppercase hex digits (test ext/uri/tests/039.phpt);
//! - dot segments are removed from the path, with PHP's handling of
//!   references without a scheme (leading `..` segments are kept when the
//!   reference has an authority or a rootless path) and the `./` or `/.`
//!   prefixes that keep a path from reading as a scheme or an authority
//!   (tests rfc3986/modification/roundtrip_special_case*.phpt);
//! - an IPv6 host is lowercased by `getHost()` and written as eight
//!   four-digit groups by `toString()` and also by `toRawString()` (test
//!   047.phpt), while `getRawHost()` keeps the text as written;
//! - the port is read as an integer (`:0443` is 443; the `:` of an empty
//!   port is kept).
//!
//! `equals()` compares the normalized forms; reference resolution is
//! RFC 3986 section 5.2 (strict).

use std::borrow::Cow;

use crate::object::DebugValue;

/// How `equals()` treats fragments (`Uri\UriComparisonMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ComparisonMode {
    IncludeFragment,
    #[default]
    ExcludeFragment,
}

/// An error raised by `Uri\Rfc3986\Uri`. Every one is thrown as
/// `Uri\InvalidUriException` with [`Rfc3986Error::message`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rfc3986Error {
    Malformed,
    BaseNotAbsolute,
    PortOutOfRange,
    SchemeMalformed,
    UserInfoMalformed,
    HostMalformed,
    PortMalformed,
    PathMalformed,
    QueryMalformed,
    FragmentMalformed,
    UserInfoWithoutHost,
    PortWithoutHost,
    HostRemovalWithUserInfo,
    HostRemovalWithPort,
}

impl Rfc3986Error {
    /// The exception message (observed on PHP 8.5; tests 007, 023, 026,
    /// 027, 055, 058.phpt).
    pub fn message(self) -> &'static str {
        match self {
            Rfc3986Error::Malformed => "The specified URI is malformed",
            Rfc3986Error::BaseNotAbsolute => "The specified base URI must be absolute",
            Rfc3986Error::PortOutOfRange => "The port is out of range",
            Rfc3986Error::SchemeMalformed => "The specified scheme is malformed",
            Rfc3986Error::UserInfoMalformed => "The specified userinfo is malformed",
            Rfc3986Error::HostMalformed => "The specified host is malformed",
            Rfc3986Error::PortMalformed => "The specified port is malformed",
            Rfc3986Error::PathMalformed => "The specified path is malformed",
            Rfc3986Error::QueryMalformed => "The specified query is malformed",
            Rfc3986Error::FragmentMalformed => "The specified fragment is malformed",
            Rfc3986Error::UserInfoWithoutHost => "Cannot set a userinfo without having a host",
            Rfc3986Error::PortWithoutHost => "Cannot set a port without having a host",
            Rfc3986Error::HostRemovalWithUserInfo => {
                "Cannot remove the host from a URI that has a userinfo"
            }
            Rfc3986Error::HostRemovalWithPort => {
                "Cannot remove the host from a URI that has a port"
            }
        }
    }
}

impl std::fmt::Display for Rfc3986Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

// ---- character classes (RFC 3986 appendix A) ----

const UNRESERVED: u8 = 1;
const SUB_DELIM: u8 = 2;
const HEX: u8 = 4;
const SCHEME: u8 = 8;
const ALPHA: u8 = 16;

static CLASS: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let c = i as u8;
        let mut v = 0;
        if c.is_ascii_alphanumeric() || c == b'-' || c == b'.' || c == b'_' || c == b'~' {
            v |= UNRESERVED;
        }
        if matches!(
            c,
            b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
        ) {
            v |= SUB_DELIM;
        }
        if c.is_ascii_hexdigit() {
            v |= HEX;
        }
        if c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.' {
            v |= SCHEME;
        }
        if c.is_ascii_alphabetic() {
            v |= ALPHA;
        }
        t[i] = v;
        i += 1;
    }
    t
};

#[inline]
fn is(c: u8, class: u8) -> bool {
    CLASS[c as usize] & class != 0
}

#[inline]
fn is_unreserved(c: u8) -> bool {
    is(c, UNRESERVED)
}

#[inline]
fn is_hex(c: u8) -> bool {
    is(c, HEX)
}

/// `pchar` without the percent-encoding (unreserved / sub-delims / `:` /
/// `@`).
#[inline]
fn is_pchar(c: u8) -> bool {
    is(c, UNRESERVED | SUB_DELIM) || c == b':' || c == b'@'
}

/// Checks a run of `allowed` characters and percent-encodings.
fn valid_run(s: &[u8], allowed: impl Fn(u8) -> bool) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'%' {
            if i + 2 >= s.len() || !is_hex(s[i + 1]) || !is_hex(s[i + 2]) {
                return false;
            }
            i += 3;
        } else if allowed(c) {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

fn valid_userinfo(s: &[u8]) -> bool {
    valid_run(s, |c| is(c, UNRESERVED | SUB_DELIM) || c == b':')
}

fn valid_reg_name(s: &[u8]) -> bool {
    valid_run(s, |c| is(c, UNRESERVED | SUB_DELIM))
}

fn valid_path_chars(s: &[u8]) -> bool {
    valid_run(s, |c| is_pchar(c) || c == b'/')
}

fn valid_query(s: &[u8]) -> bool {
    valid_run(s, |c| is_pchar(c) || c == b'/' || c == b'?')
}

fn valid_scheme(s: &[u8]) -> bool {
    !s.is_empty() && is(s[0], ALPHA) && s[1..].iter().all(|&c| is(c, SCHEME))
}

/// RFC 3986 `dec-octet`.
fn dec_octet(s: &[u8]) -> Option<u8> {
    match s.len() {
        1 if s[0].is_ascii_digit() => Some(s[0] - b'0'),
        2 if s[0] != b'0' && s.iter().all(u8::is_ascii_digit) => {
            Some((s[0] - b'0') * 10 + s[1] - b'0')
        }
        3 if s[0] != b'0' && s.iter().all(u8::is_ascii_digit) => {
            let v = (s[0] - b'0') as u32 * 100 + (s[1] - b'0') as u32 * 10 + (s[2] - b'0') as u32;
            if v <= 255 { Some(v as u8) } else { None }
        }
        _ => None,
    }
}

/// RFC 3986 `IPv4address`.
fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = s.split(|&c| c == b'.');
    for o in out.iter_mut() {
        *o = dec_octet(parts.next()?)?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

/// RFC 3986 `IPv6address` (no zone identifier).
pub(crate) fn parse_ipv6(s: &[u8]) -> Option<[u16; 8]> {
    fn h16(p: &[u8]) -> Option<u16> {
        if p.is_empty() || p.len() > 4 || !p.iter().all(|&c| is_hex(c)) {
            return None;
        }
        let mut v = 0u16;
        for &c in p {
            v = v << 4 | (c as char).to_digit(16).unwrap() as u16;
        }
        Some(v)
    }
    // Parses a `:`-separated run of pieces, the last of which may be an
    // IPv4 address (when `allow_v4`), into `out`; returns the piece count.
    fn pieces(s: &[u8], allow_v4: bool, out: &mut Vec<u16>) -> Option<()> {
        if s.is_empty() {
            return Some(());
        }
        let parts: Vec<&[u8]> = s.split(|&c| c == b':').collect();
        for (i, p) in parts.iter().enumerate() {
            if i + 1 == parts.len() && allow_v4 && p.contains(&b'.') {
                let v4 = parse_ipv4(p)?;
                out.push((v4[0] as u16) << 8 | v4[1] as u16);
                out.push((v4[2] as u16) << 8 | v4[3] as u16);
            } else {
                out.push(h16(p)?);
            }
        }
        Some(())
    }
    let mut head = Vec::with_capacity(8);
    let mut tail = Vec::with_capacity(8);
    match find_sub(s, b"::") {
        Some(at) => {
            let (a, b) = (&s[..at], &s[at + 2..]);
            if find_sub(b, b"::").is_some() {
                return None;
            }
            // `a` cannot end and `b` cannot start with another ':'.
            if a.last() == Some(&b':') || b.first() == Some(&b':') {
                return None;
            }
            pieces(a, false, &mut head)?;
            pieces(b, true, &mut tail)?;
            if head.len() + tail.len() > 7 {
                return None;
            }
        }
        None => {
            pieces(s, true, &mut head)?;
            if head.len() != 8 {
                return None;
            }
        }
    }
    let mut out = [0u16; 8];
    out[..head.len()].copy_from_slice(&head);
    out[8 - tail.len()..].copy_from_slice(&tail);
    Some(out)
}

fn find_sub(s: &[u8], pat: &[u8]) -> Option<usize> {
    s.windows(pat.len()).position(|w| w == pat)
}

/// RFC 3986 `IPvFuture`.
fn valid_ipvfuture(s: &[u8]) -> bool {
    if s.len() < 4 || !(s[0] == b'v' || s[0] == b'V') {
        return false;
    }
    let Some(dot) = s.iter().position(|&c| c == b'.') else {
        return false;
    };
    dot > 1
        && s[1..dot].iter().all(|&c| is_hex(c))
        && dot + 1 < s.len()
        && s[dot + 1..]
            .iter()
            .all(|&c| is(c, UNRESERVED | SUB_DELIM) || c == b':')
}

// ---- the parsed form ----

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Span {
    start: u32,
    end: u32,
}

impl Span {
    #[inline]
    fn new(start: usize, end: usize) -> Span {
        Span {
            start: start as u32,
            end: end as u32,
        }
    }
    #[inline]
    fn get(self, buf: &[u8]) -> &[u8] {
        &buf[self.start as usize..self.end as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostKind {
    RegName,
    Ipv6([u16; 8]),
    IpFuture,
}

/// A `Uri\Rfc3986\Uri` value.
#[derive(Clone, Debug)]
pub struct Uri {
    /// The components as written, concatenated with their delimiters.
    buf: Box<[u8]>,
    scheme: Option<Span>,
    userinfo: Option<Span>,
    host: Option<Span>,
    host_kind: HostKind,
    /// The port digits (possibly empty: `http://h:`).
    port: Option<Span>,
    port_value: Option<i64>,
    path: Span,
    query: Option<Span>,
    fragment: Option<Span>,
    /// `toString()`, made on first use (the value never changes).
    normalized: std::cell::OnceCell<Box<[u8]>>,
}

impl goro_runtime::NativeData for Uri {}

/// The components of a URI being built (raw texts).
#[derive(Clone, Copy, Default)]
struct Parts<'a> {
    scheme: Option<&'a [u8]>,
    userinfo: Option<&'a [u8]>,
    host: Option<&'a [u8]>,
    port: Option<&'a [u8]>,
    path: &'a [u8],
    query: Option<&'a [u8]>,
    fragment: Option<&'a [u8]>,
}

/// Parses an authority (`[ userinfo "@" ] host [ ":" port ]`) at
/// `s[start..end]`; returns (userinfo, host, kind, port) spans.
#[allow(clippy::type_complexity)]
fn parse_authority(
    s: &[u8],
    start: usize,
    end: usize,
) -> Option<(Option<Span>, Span, HostKind, Option<Span>)> {
    let a = &s[start..end];
    let (userinfo, hstart) = match a.iter().position(|&c| c == b'@') {
        Some(at) => {
            if !valid_userinfo(&a[..at]) {
                return None;
            }
            (Some(Span::new(start, start + at)), at + 1)
        }
        None => (None, 0),
    };
    let rest = &a[hstart..];
    let (host_len, kind) = if rest.first() == Some(&b'[') {
        let close = rest.iter().position(|&c| c == b']')?;
        let inner = &rest[1..close];
        let kind = if inner.first().is_some_and(|&c| c == b'v' || c == b'V') {
            if !valid_ipvfuture(inner) {
                return None;
            }
            HostKind::IpFuture
        } else {
            HostKind::Ipv6(parse_ipv6(inner)?)
        };
        (close + 1, kind)
    } else {
        let n = rest.iter().position(|&c| c == b':').unwrap_or(rest.len());
        if !valid_reg_name(&rest[..n]) {
            return None;
        }
        (n, HostKind::RegName)
    };
    let host = Span::new(start + hstart, start + hstart + host_len);
    let after = &rest[host_len..];
    let port = match after.first() {
        None => None,
        Some(b':') => {
            if !after[1..].iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(Span::new(start + hstart + host_len + 1, end))
        }
        Some(_) => return None,
    };
    Some((userinfo, host, kind, port))
}

fn port_value(digits: &[u8]) -> Result<Option<i64>, Rfc3986Error> {
    if digits.is_empty() {
        return Ok(None);
    }
    // More than 20 digits is out of range even with leading zeros
    // (observed on PHP 8.5).
    if digits.len() > 20 {
        return Err(Rfc3986Error::PortOutOfRange);
    }
    let mut v: i64 = 0;
    for &c in digits {
        v = v
            .checked_mul(10)
            .and_then(|v| v.checked_add((c - b'0') as i64))
            .ok_or(Rfc3986Error::PortOutOfRange)?;
    }
    Ok(Some(v))
}

impl Uri {
    /// `Uri\Rfc3986\Uri::parse($uri)` / `new Uri\Rfc3986\Uri($uri)` without
    /// a base.
    pub fn parse(input: &[u8]) -> Result<Uri, Rfc3986Error> {
        let s = input;
        let n = s.len();
        if n > u32::MAX as usize {
            return Err(Rfc3986Error::Malformed);
        }
        let mut i = 0;
        let mut scheme = None;
        if n > 0 && is(s[0], ALPHA) {
            let mut j = 1;
            while j < n && is(s[j], SCHEME) {
                j += 1;
            }
            if j < n && s[j] == b':' {
                scheme = Some(Span::new(0, j));
                i = j + 1;
            }
        }
        let mut userinfo = None;
        let mut host = None;
        let mut host_kind = HostKind::RegName;
        let mut port = None;
        if s[i..].starts_with(b"//") {
            let astart = i + 2;
            let aend = s[astart..]
                .iter()
                .position(|&c| c == b'/' || c == b'?' || c == b'#')
                .map_or(n, |p| astart + p);
            let (u, h, k, p) = parse_authority(s, astart, aend).ok_or(Rfc3986Error::Malformed)?;
            userinfo = u;
            host = Some(h);
            host_kind = k;
            port = p;
            i = aend;
        }
        let pend = s[i..]
            .iter()
            .position(|&c| c == b'?' || c == b'#')
            .map_or(n, |p| i + p);
        let path = &s[i..pend];
        if !valid_path_chars(path) {
            return Err(Rfc3986Error::Malformed);
        }
        if scheme.is_none() && host.is_none() {
            // path-noscheme: no ':' in the first segment.
            let first = path.iter().position(|&c| c == b'/').unwrap_or(path.len());
            if path[..first].contains(&b':') {
                return Err(Rfc3986Error::Malformed);
            }
        }
        let path_span = Span::new(i, pend);
        i = pend;
        let mut query = None;
        if i < n && s[i] == b'?' {
            let qend = s[i..].iter().position(|&c| c == b'#').map_or(n, |p| i + p);
            if !valid_query(&s[i + 1..qend]) {
                return Err(Rfc3986Error::Malformed);
            }
            query = Some(Span::new(i + 1, qend));
            i = qend;
        }
        let mut fragment = None;
        if i < n {
            // s[i] == '#'
            if !valid_query(&s[i + 1..]) {
                return Err(Rfc3986Error::Malformed);
            }
            fragment = Some(Span::new(i + 1, n));
        }
        let port_value = match port {
            Some(p) => port_value(p.get(s))?,
            None => None,
        };
        Ok(Uri {
            buf: s.into(),
            scheme,
            userinfo,
            host,
            host_kind,
            port,
            port_value,
            path: path_span,
            query,
            fragment,
            normalized: Default::default(),
        })
    }

    /// `Uri\Rfc3986\Uri::parse($uri, $baseUrl)` and the constructor: parse
    /// `input` and, with a base, resolve it against the base (which must
    /// have a scheme).
    pub fn parse_with_base(input: &[u8], base: Option<&Uri>) -> Result<Uri, Rfc3986Error> {
        match base {
            None => Uri::parse(input),
            Some(b) => b.resolve(input),
        }
    }

    /// Builds a URI from component texts already known to be valid.
    fn build(p: Parts<'_>, kind: HostKind) -> Uri {
        let mut buf = Vec::with_capacity(
            p.scheme.map_or(0, |s| s.len() + 1)
                + p.userinfo.map_or(0, |s| s.len() + 1)
                + p.host.map_or(0, |s| s.len() + 2)
                + p.port.map_or(0, |s| s.len() + 1)
                + p.path.len()
                + p.query.map_or(0, |s| s.len() + 1)
                + p.fragment.map_or(0, |s| s.len() + 1),
        );
        let put = |buf: &mut Vec<u8>, s: &[u8]| {
            let st = buf.len();
            buf.extend_from_slice(s);
            Span::new(st, buf.len())
        };
        let scheme = p.scheme.map(|s| {
            let sp = put(&mut buf, s);
            buf.push(b':');
            sp
        });
        let mut userinfo = None;
        let mut host = None;
        let mut port = None;
        if let Some(h) = p.host {
            buf.extend_from_slice(b"//");
            userinfo = p.userinfo.map(|u| {
                let sp = put(&mut buf, u);
                buf.push(b'@');
                sp
            });
            host = Some(put(&mut buf, h));
            port = p.port.map(|d| {
                buf.push(b':');
                put(&mut buf, d)
            });
        }
        let path = put(&mut buf, p.path);
        let query = p.query.map(|q| {
            buf.push(b'?');
            put(&mut buf, q)
        });
        let fragment = p.fragment.map(|f| {
            buf.push(b'#');
            put(&mut buf, f)
        });
        let port_value = port.and_then(|sp| port_value(sp.get(&buf)).ok().flatten());
        Uri {
            buf: buf.into_boxed_slice(),
            scheme,
            userinfo,
            host,
            host_kind: kind,
            port,
            port_value,
            path,
            query,
            fragment,
            normalized: Default::default(),
        }
    }

    fn parts(&self) -> Parts<'_> {
        let b = &self.buf;
        Parts {
            scheme: self.scheme.map(|s| s.get(b)),
            userinfo: self.userinfo.map(|s| s.get(b)),
            host: self.host.map(|s| s.get(b)),
            port: self.port.map(|s| s.get(b)),
            path: self.path.get(b),
            query: self.query.map(|s| s.get(b)),
            fragment: self.fragment.map(|s| s.get(b)),
        }
    }

    // ---- raw getters ----

    pub fn raw_scheme(&self) -> Option<&[u8]> {
        self.scheme.map(|s| s.get(&self.buf))
    }
    pub fn raw_userinfo(&self) -> Option<&[u8]> {
        self.userinfo.map(|s| s.get(&self.buf))
    }
    /// The part of the userinfo before the first `:`.
    pub fn raw_username(&self) -> Option<&[u8]> {
        self.raw_userinfo()
            .map(|u| &u[..u.iter().position(|&c| c == b':').unwrap_or(u.len())])
    }
    /// The part of the userinfo after the first `:` (empty without one).
    pub fn raw_password(&self) -> Option<&[u8]> {
        self.raw_userinfo()
            .map(|u| match u.iter().position(|&c| c == b':') {
                Some(p) => &u[p + 1..],
                None => &u[u.len()..],
            })
    }
    pub fn raw_host(&self) -> Option<&[u8]> {
        self.host.map(|s| s.get(&self.buf))
    }
    pub fn port(&self) -> Option<i64> {
        self.port_value
    }
    pub fn raw_path(&self) -> &[u8] {
        self.path.get(&self.buf)
    }
    pub fn raw_query(&self) -> Option<&[u8]> {
        self.query.map(|s| s.get(&self.buf))
    }
    pub fn raw_fragment(&self) -> Option<&[u8]> {
        self.fragment.map(|s| s.get(&self.buf))
    }

    // ---- normalized getters ----

    pub fn scheme(&self) -> Option<Cow<'_, [u8]>> {
        self.raw_scheme().map(lowercase)
    }
    pub fn userinfo(&self) -> Option<Cow<'_, [u8]>> {
        self.raw_userinfo().map(|u| pct_normalize(u, false))
    }
    pub fn username(&self) -> Option<Cow<'_, [u8]>> {
        self.raw_username().map(|u| pct_normalize(u, false))
    }
    pub fn password(&self) -> Option<Cow<'_, [u8]>> {
        self.raw_password().map(|u| pct_normalize(u, false))
    }
    pub fn host(&self) -> Option<Cow<'_, [u8]>> {
        let h = self.raw_host()?;
        Some(match self.host_kind {
            HostKind::RegName => pct_normalize(h, true),
            HostKind::Ipv6(_) | HostKind::IpFuture => lowercase(h),
        })
    }
    pub fn path(&self) -> Cow<'_, [u8]> {
        let raw = self.raw_path();
        if raw.is_empty() {
            return Cow::Borrowed(raw);
        }
        let p = pct_normalize(raw, false);
        if !p.contains(&b'.') {
            return p;
        }
        Cow::Owned(normalize_path(
            &p,
            self.scheme.is_some(),
            self.host.is_some(),
        ))
    }
    pub fn query(&self) -> Option<Cow<'_, [u8]>> {
        self.raw_query().map(|q| pct_normalize(q, false))
    }
    pub fn fragment(&self) -> Option<Cow<'_, [u8]>> {
        self.raw_fragment().map(|f| pct_normalize(f, false))
    }

    // ---- serialization ----

    /// `toRawString()`.
    pub fn to_raw_string(&self) -> Vec<u8> {
        match self.host_kind {
            HostKind::Ipv6(a) => {
                let h = self.host.unwrap();
                let mut out = Vec::with_capacity(self.buf.len() + 32);
                out.extend_from_slice(&self.buf[..h.start as usize]);
                push_ipv6_full(&mut out, &a);
                out.extend_from_slice(&self.buf[h.end as usize..]);
                out
            }
            _ => self.buf.to_vec(),
        }
    }

    /// `toString()`: the normalized form.
    pub fn to_normalized_string(&self) -> Vec<u8> {
        self.normalized
            .get_or_init(|| self.normalized_string(true).into_boxed_slice())
            .to_vec()
    }

    fn normalized_string(&self, with_fragment: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.buf.len() + 8);
        if let Some(s) = self.raw_scheme() {
            out.extend(s.iter().map(u8::to_ascii_lowercase));
            out.push(b':');
        }
        if let Some(h) = self.raw_host() {
            out.extend_from_slice(b"//");
            if let Some(u) = self.raw_userinfo() {
                pct_normalize_into(u, false, &mut out);
                out.push(b'@');
            }
            match self.host_kind {
                HostKind::RegName => pct_normalize_into(h, true, &mut out),
                HostKind::Ipv6(a) => push_ipv6_full(&mut out, &a),
                HostKind::IpFuture => out.extend(h.iter().map(u8::to_ascii_lowercase)),
            }
            if self.port.is_some() {
                out.push(b':');
                if let Some(v) = self.port_value {
                    out.extend_from_slice(v.to_string().as_bytes());
                }
            }
        }
        out.extend_from_slice(&self.path());
        if let Some(q) = self.raw_query() {
            out.push(b'?');
            pct_normalize_into(q, false, &mut out);
        }
        if with_fragment && let Some(f) = self.raw_fragment() {
            out.push(b'#');
            pct_normalize_into(f, false, &mut out);
        }
        out
    }

    /// `equals()`: the normalized forms are compared.
    pub fn equals(&self, other: &Uri, mode: ComparisonMode) -> bool {
        let wf = mode == ComparisonMode::IncludeFragment;
        self.normalized_string(wf) == other.normalized_string(wf)
    }

    // ---- reference resolution (RFC 3986 section 5.2) ----

    /// `resolve($uri)`: parses `input` and resolves it against `self`.
    pub fn resolve(&self, input: &[u8]) -> Result<Uri, Rfc3986Error> {
        let r = Uri::parse(input)?;
        if self.scheme.is_none() {
            return Err(Rfc3986Error::BaseNotAbsolute);
        }
        Ok(self.resolve_parsed(&r))
    }

    fn resolve_parsed(&self, r: &Uri) -> Uri {
        let rp = r.parts();
        let bp = self.parts();
        let mut t = Parts::default();
        let path_buf: Vec<u8>;
        let kind;
        if rp.scheme.is_some() {
            t = rp;
            path_buf = dot_segments(rp.path, true, rp.host.is_some(), false);
            kind = r.host_kind;
        } else {
            if rp.host.is_some() {
                t.userinfo = rp.userinfo;
                t.host = rp.host;
                t.port = rp.port;
                path_buf = dot_segments(rp.path, true, true, false);
                t.query = rp.query;
                kind = r.host_kind;
            } else {
                if rp.path.is_empty() {
                    path_buf = bp.path.to_vec();
                    t.query = if rp.query.is_some() {
                        rp.query
                    } else {
                        bp.query
                    };
                } else {
                    if rp.path[0] == b'/' {
                        path_buf = dot_segments(rp.path, true, bp.host.is_some(), false);
                    } else {
                        // merge (5.2.3)
                        let mut m = Vec::with_capacity(bp.path.len() + rp.path.len() + 1);
                        if bp.host.is_some() && bp.path.is_empty() {
                            m.push(b'/');
                        } else if let Some(slash) = bp.path.iter().rposition(|&c| c == b'/') {
                            m.extend_from_slice(&bp.path[..=slash]);
                        }
                        m.extend_from_slice(rp.path);
                        path_buf = dot_segments(&m, true, bp.host.is_some(), true);
                    }
                    t.query = rp.query;
                }
                t.userinfo = bp.userinfo;
                t.host = bp.host;
                t.port = bp.port;
                kind = self.host_kind;
            }
            t.scheme = bp.scheme;
        }
        t.fragment = rp.fragment;
        t.path = &path_buf;
        Uri::build(t, kind)
    }

    // ---- withers ----

    /// `withScheme(?string $scheme)`.
    pub fn with_scheme(&self, scheme: Option<&[u8]>) -> Result<Uri, Rfc3986Error> {
        let mut p = self.parts();
        let fixed;
        match scheme {
            Some(s) => {
                if !valid_scheme(s) {
                    return Err(Rfc3986Error::SchemeMalformed);
                }
                p.scheme = Some(s);
            }
            None => {
                p.scheme = None;
                if p.host.is_none() && first_segment_has_colon(p.path) {
                    fixed = [b"./".as_slice(), p.path].concat();
                    p.path = &fixed;
                }
            }
        }
        Ok(Uri::build(p, self.host_kind))
    }

    /// `withUserInfo(?string $userinfo)`.
    pub fn with_userinfo(&self, userinfo: Option<&[u8]>) -> Result<Uri, Rfc3986Error> {
        let mut p = self.parts();
        match userinfo {
            Some(u) => {
                if p.host.is_none() {
                    return Err(Rfc3986Error::UserInfoWithoutHost);
                }
                if !valid_userinfo(u) {
                    return Err(Rfc3986Error::UserInfoMalformed);
                }
                p.userinfo = Some(u);
            }
            None => p.userinfo = None,
        }
        Ok(Uri::build(p, self.host_kind))
    }

    /// `withHost(?string $host)`.
    pub fn with_host(&self, host: Option<&[u8]>) -> Result<Uri, Rfc3986Error> {
        let mut p = self.parts();
        let fixed;
        let kind;
        match host {
            Some(h) => {
                kind = host_kind(h).ok_or(Rfc3986Error::HostMalformed)?;
                p.host = Some(h);
                if !p.path.is_empty() && p.path[0] != b'/' {
                    fixed = [b"/".as_slice(), p.path].concat();
                    p.path = &fixed;
                }
            }
            None => {
                if p.host.is_some() {
                    if p.userinfo.is_some() {
                        return Err(Rfc3986Error::HostRemovalWithUserInfo);
                    }
                    if p.port.is_some() {
                        return Err(Rfc3986Error::HostRemovalWithPort);
                    }
                }
                kind = HostKind::RegName;
                p.host = None;
                if p.path.starts_with(b"//") {
                    fixed = [b"/.".as_slice(), p.path].concat();
                    p.path = &fixed;
                }
            }
        }
        Ok(Uri::build(p, kind))
    }

    /// `withPort(?int $port)`.
    pub fn with_port(&self, port: Option<i64>) -> Result<Uri, Rfc3986Error> {
        let mut p = self.parts();
        let digits;
        match port {
            Some(v) => {
                if p.host.is_none() {
                    return Err(Rfc3986Error::PortWithoutHost);
                }
                if v < 0 {
                    return Err(Rfc3986Error::PortMalformed);
                }
                digits = v.to_string();
                p.port = Some(digits.as_bytes());
            }
            None => p.port = None,
        }
        Ok(Uri::build(p, self.host_kind))
    }

    /// `withPath(string $path)`.
    pub fn with_path(&self, path: &[u8]) -> Result<Uri, Rfc3986Error> {
        let mut p = self.parts();
        if !valid_path_chars(path) {
            return Err(Rfc3986Error::PathMalformed);
        }
        let fixed;
        if p.host.is_some() {
            if !path.is_empty() && path[0] != b'/' {
                return Err(Rfc3986Error::PathMalformed);
            }
            p.path = path;
        } else if path.starts_with(b"//") {
            fixed = [b"/.".as_slice(), path].concat();
            p.path = &fixed;
        } else if p.scheme.is_none() && first_segment_has_colon(path) {
            fixed = [b"./".as_slice(), path].concat();
            p.path = &fixed;
        } else {
            p.path = path;
        }
        Ok(Uri::build(p, self.host_kind))
    }

    /// `withQuery(?string $query)`.
    pub fn with_query(&self, query: Option<&[u8]>) -> Result<Uri, Rfc3986Error> {
        let mut p = self.parts();
        if let Some(q) = query
            && !valid_query(q)
        {
            return Err(Rfc3986Error::QueryMalformed);
        }
        p.query = query;
        Ok(Uri::build(p, self.host_kind))
    }

    /// `withFragment(?string $fragment)`.
    pub fn with_fragment(&self, fragment: Option<&[u8]>) -> Result<Uri, Rfc3986Error> {
        let mut p = self.parts();
        if let Some(f) = fragment
            && !valid_query(f)
        {
            return Err(Rfc3986Error::FragmentMalformed);
        }
        p.fragment = fragment;
        Ok(Uri::build(p, self.host_kind))
    }

    // ---- PHP object support ----

    /// The `"uri"` entry of `__serialize()`'s first array: the raw string
    /// (test 031.phpt).
    pub fn serialized_uri(&self) -> Vec<u8> {
        self.to_raw_string()
    }

    /// `__debugInfo()` (also what `var_dump()` and `print_r()` show): the
    /// raw components (test 043.phpt; observed on PHP 8.5).
    pub fn debug_info(&self) -> [(&'static str, DebugValue<'_>); 8] {
        fn s(v: Option<&[u8]>) -> DebugValue<'_> {
            v.map_or(DebugValue::Null, DebugValue::Str)
        }
        [
            ("scheme", s(self.raw_scheme())),
            ("username", s(self.raw_username())),
            ("password", s(self.raw_password())),
            ("host", s(self.raw_host())),
            (
                "port",
                self.port_value.map_or(DebugValue::Null, DebugValue::Int),
            ),
            ("path", DebugValue::Str(self.raw_path())),
            ("query", s(self.raw_query())),
            ("fragment", s(self.raw_fragment())),
        ]
    }

    /// [`Uri::debug_info`] as the PHP array.
    pub fn debug_array(&self) -> goro_runtime::PhpArray {
        crate::object::debug_array(self.debug_info())
    }
}

fn host_kind(h: &[u8]) -> Option<HostKind> {
    if h.first() == Some(&b'[') {
        let inner = h.strip_prefix(b"[")?.strip_suffix(b"]")?;
        if inner.first().is_some_and(|&c| c == b'v' || c == b'V') {
            return valid_ipvfuture(inner).then_some(HostKind::IpFuture);
        }
        return parse_ipv6(inner).map(HostKind::Ipv6);
    }
    valid_reg_name(h).then_some(HostKind::RegName)
}

fn first_segment_has_colon(path: &[u8]) -> bool {
    let first = path.iter().position(|&c| c == b'/').unwrap_or(path.len());
    path[..first].contains(&b':')
}

fn push_ipv6_full(out: &mut Vec<u8>, a: &[u16; 8]) {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    out.push(b'[');
    for (i, &g) in a.iter().enumerate() {
        if i > 0 {
            out.push(b':');
        }
        for sh in [12, 8, 4, 0] {
            out.push(HEXD[(g >> sh) as usize & 15]);
        }
    }
    out.push(b']');
}

fn lowercase(s: &[u8]) -> Cow<'_, [u8]> {
    if s.iter().any(u8::is_ascii_uppercase) {
        Cow::Owned(s.to_ascii_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}

#[inline]
fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    }
}

/// Percent-encoding normalization (RFC 3986 sections 6.2.2.1-2): decodes
/// unreserved characters, uppercases the hex digits of the others, and
/// lowercases ASCII letters when `lower` (hosts).
fn pct_normalize(s: &[u8], lower: bool) -> Cow<'_, [u8]> {
    let needs = s
        .iter()
        .any(|&c| c == b'%' || (lower && c.is_ascii_uppercase()));
    if !needs {
        return Cow::Borrowed(s);
    }
    let mut out = Vec::with_capacity(s.len());
    pct_normalize_into(s, lower, &mut out);
    Cow::Owned(out)
}

fn pct_normalize_into(s: &[u8], lower: bool, out: &mut Vec<u8>) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'%' && i + 2 < s.len() {
            let v = hex_val(s[i + 1]) << 4 | hex_val(s[i + 2]);
            if is_unreserved(v) {
                out.push(if lower { v.to_ascii_lowercase() } else { v });
            } else {
                out.push(b'%');
                out.push(s[i + 1].to_ascii_uppercase());
                out.push(s[i + 2].to_ascii_uppercase());
            }
            i += 3;
        } else {
            out.push(if lower { c.to_ascii_lowercase() } else { c });
            i += 1;
        }
    }
}

/// Dot-segment removal (observed on PHP 8.5; see the module
/// documentation). Used by `getPath()`/`toString()` on the
/// percent-normalized path and, with `has_scheme`, by reference resolution
/// on the raw target path (`merge` when the path came from merging the
/// reference with the base path, RFC 3986 section 5.2.3).
fn normalize_path(path: &[u8], has_scheme: bool, has_host: bool) -> Vec<u8> {
    dot_segments(path, has_scheme, has_host, false)
}

fn dot_segments(path: &[u8], has_scheme: bool, has_host: bool, merge: bool) -> Vec<u8> {
    if path.is_empty() {
        return Vec::new();
    }
    // With an authority the leading '/' is a separator; without one a
    // leading '/' makes the path absolute.
    let absolute = !has_host && path[0] == b'/';
    let body = if has_host || absolute {
        &path[1..]
    } else {
        path
    };
    let segs: Vec<&[u8]> = body.split(|&c| c == b'/').collect();
    let keep_leading_dotdot = !has_scheme && !absolute;
    let mut out: Vec<&[u8]> = Vec::with_capacity(segs.len() + 1);
    for (i, &seg) in segs.iter().enumerate() {
        let last = i + 1 == segs.len();
        if seg == b"." {
            if last {
                if has_host || !out.is_empty() {
                    out.push(b"");
                }
            } else if out.is_empty()
                && ((!has_host && segs[i + 1].is_empty())
                    || (!has_scheme && !absolute && segs[i + 1].contains(&b':')))
            {
                out.push(b".");
            }
            continue;
        }
        if seg == b".." {
            if out.last().is_some_and(|p| *p != b"..") {
                out.pop();
                if last {
                    out.push(b"");
                }
            } else if keep_leading_dotdot {
                out.push(b"..");
            } else if last && has_host {
                out.push(b"");
            }
            continue;
        }
        out.push(seg);
    }
    // After a merge, a path left starting with an empty segment gets a "."
    // in front (observed on PHP 8.5: "s:/a/b" + ".." is "s:/./",
    // "http://h" + ".//" is "http://h/.//").
    if merge
        && out.first().is_some_and(|s| s.is_empty())
        && ((!has_host && absolute) || (out.len() >= 2 && out.last().is_some_and(|s| s.is_empty())))
    {
        out.insert(0, b".");
    }
    let mut r = Vec::with_capacity(path.len());
    if (has_host && !out.is_empty()) || absolute {
        r.push(b'/');
    }
    for (i, s) in out.iter().enumerate() {
        if i > 0 {
            r.push(b'/');
        }
        r.extend_from_slice(s);
    }
    r
}
