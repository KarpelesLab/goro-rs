//! urlencode() and friends, the encoding part of http_build_query(),
//! parse_url(), base64_encode() and base64_decode().
//!
//! Behaviour sources: the PHP manual pages of these functions, RFC 3986
//! (URLs), RFC 1738 and RFC 4648 (base64), the tests
//! ext/standard/tests/url/*.phpt and http/http_build_query/*.phpt, and
//! observation of PHP 8.5 (tests/fixtures/url.txt and its driver url.php).

use crate::error::StdError;

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn encode(s: &[u8], raw: bool, out: &mut Vec<u8>) {
    for &c in s {
        if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.' || (raw && c == b'~') {
            out.push(c);
        } else if c == b' ' && !raw {
            out.push(b'+');
        } else {
            out.extend_from_slice(&[b'%', HEX[(c >> 4) as usize], HEX[(c & 15) as usize]]);
        }
    }
}

fn decode(s: &[u8], plus: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'+' && plus {
            out.push(b' ');
        } else if c == b'%' && i + 2 < s.len() {
            match (hex_val(s[i + 1]), hex_val(s[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h << 4 | l);
                    i += 3;
                    continue;
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// urlencode(): RFC 1738 style, space as `+` (PHP manual).
pub fn urlencode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    encode(s, false, &mut out);
    out
}

/// rawurlencode(): RFC 3986, `~` kept (PHP manual).
pub fn rawurlencode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    encode(s, true, &mut out);
    out
}

/// urldecode(): `+` becomes a space; malformed `%` sequences are kept.
pub fn urldecode(s: &[u8]) -> Vec<u8> {
    decode(s, true)
}

/// rawurldecode().
pub fn rawurldecode(s: &[u8]) -> Vec<u8> {
    decode(s, false)
}

/// `$encoding_type` of http_build_query().
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryEncoding {
    /// PHP_QUERY_RFC1738 (the default): urlencode().
    Rfc1738,
    /// PHP_QUERY_RFC3986: rawurlencode().
    Rfc3986,
}

impl QueryEncoding {
    pub const PHP_QUERY_RFC1738: i64 = 1;
    pub const PHP_QUERY_RFC3986: i64 = 2;

    /// From the PHP constant; other values act as RFC 1738 (observed on PHP 8.5).
    pub fn from_php(v: i64) -> QueryEncoding {
        if v == Self::PHP_QUERY_RFC3986 {
            QueryEncoding::Rfc3986
        } else {
            QueryEncoding::Rfc1738
        }
    }
}

/// An array key as http_build_query() sees it.
#[derive(Clone, Copy, Debug)]
pub enum QueryKey<'a> {
    Int(i64),
    Str(&'a [u8]),
}

/// Builds an http_build_query() result. The wiring layer walks the array:
/// for each element it makes the (already encoded) key name with
/// [`QueryBuilder::top_key`] or [`QueryBuilder::child_key`], recurses into
/// arrays and objects with that name as parent, skips nulls (and other
/// values PHP skips), and adds scalars, converted to their string form
/// (true "1", false "0", floats as PHP prints them), with
/// [`QueryBuilder::push`].
pub struct QueryBuilder {
    out: Vec<u8>,
    sep: Vec<u8>,
    enc: QueryEncoding,
}

impl QueryBuilder {
    /// `sep` is `$arg_separator`, or the ini arg_separator.output when null.
    pub fn new(sep: &[u8], enc: QueryEncoding) -> QueryBuilder {
        QueryBuilder {
            out: Vec::new(),
            sep: sep.to_vec(),
            enc,
        }
    }

    fn enc(&self, s: &[u8], out: &mut Vec<u8>) {
        encode(s, self.enc == QueryEncoding::Rfc3986, out);
    }

    /// Name of a top-level element: integer keys get `numeric_prefix`.
    pub fn top_key(&self, key: QueryKey, numeric_prefix: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        match key {
            QueryKey::Int(i) => {
                // The prefix is used as given (observed on PHP 8.5).
                out.extend_from_slice(numeric_prefix);
                out.extend_from_slice(i.to_string().as_bytes());
            }
            QueryKey::Str(s) => self.enc(s, &mut out),
        }
        out
    }

    /// Name of a nested element: `parent[key]`, brackets encoded.
    pub fn child_key(&self, parent: &[u8], key: QueryKey) -> Vec<u8> {
        let mut out = parent.to_vec();
        out.extend_from_slice(b"%5B");
        match key {
            QueryKey::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
            QueryKey::Str(s) => self.enc(s, &mut out),
        }
        out.extend_from_slice(b"%5D");
        out
    }

    /// Adds `name=value`; `value` is encoded here.
    pub fn push(&mut self, name: &[u8], value: &[u8]) {
        if !self.out.is_empty() {
            let sep = std::mem::take(&mut self.sep);
            self.out.extend_from_slice(&sep);
            self.sep = sep;
        }
        self.out.extend_from_slice(name);
        self.out.push(b'=');
        let mut v = Vec::with_capacity(value.len());
        self.enc(value, &mut v);
        self.out.extend_from_slice(&v);
    }

    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

/// parse_url() result; `None` fields are absent from the PHP array, which
/// lists them in this order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Url {
    pub scheme: Option<Vec<u8>>,
    pub host: Option<Vec<u8>>,
    pub port: Option<i64>,
    pub user: Option<Vec<u8>>,
    pub pass: Option<Vec<u8>>,
    pub path: Option<Vec<u8>>,
    pub query: Option<Vec<u8>>,
    pub fragment: Option<Vec<u8>>,
}

pub const PHP_URL_SCHEME: i64 = 0;
pub const PHP_URL_HOST: i64 = 1;
pub const PHP_URL_PORT: i64 = 2;
pub const PHP_URL_USER: i64 = 3;
pub const PHP_URL_PASS: i64 = 4;
pub const PHP_URL_PATH: i64 = 5;
pub const PHP_URL_QUERY: i64 = 6;
pub const PHP_URL_FRAGMENT: i64 = 7;

/// One component selected by parse_url()'s `$component`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrlPart {
    Str(Vec<u8>),
    Int(i64),
}

/// What parse_url() returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrlResult {
    /// false: a seriously malformed URL.
    False,
    /// The array (no component, or a negative one).
    Array(Url),
    /// One component; `None` is null (absent).
    Part(Option<UrlPart>),
}

/// parse_url($url, $component): a negative component returns the array
/// (test parse_url_error_002.phpt; observed on PHP 8.5 for values below -1),
/// one above PHP_URL_FRAGMENT throws a ValueError.
pub fn parse_url_component(s: &[u8], component: i64) -> Result<UrlResult, StdError> {
    if component > PHP_URL_FRAGMENT {
        return Err(StdError::ValueError(format!(
            "parse_url(): Argument #2 ($component) must be a valid URL component identifier, {component} given"
        )));
    }
    let Some(u) = parse_url(s) else {
        return Ok(UrlResult::False);
    };
    if component < 0 {
        return Ok(UrlResult::Array(u));
    }
    let st = |v: Option<Vec<u8>>| v.map(UrlPart::Str);
    Ok(UrlResult::Part(match component {
        PHP_URL_SCHEME => st(u.scheme),
        PHP_URL_HOST => st(u.host),
        PHP_URL_PORT => u.port.map(UrlPart::Int),
        PHP_URL_USER => st(u.user),
        PHP_URL_PASS => st(u.pass),
        PHP_URL_PATH => st(u.path),
        PHP_URL_QUERY => st(u.query),
        _ => st(u.fragment),
    }))
}

/// Control characters in the components become `_` (observed on PHP 8.5).
fn clean(s: &[u8]) -> Vec<u8> {
    s.iter()
        .map(|&c| if c < 0x20 || c == 0x7F { b'_' } else { c })
        .collect()
}

fn is_scheme_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.'
}

/// Parses a port like C's strtol(): leading whitespace, an optional sign,
/// digits, anything after ignored; `None` without digits or outside
/// 0..=65535 (observed on PHP 8.5: "http://h: 8" has port 8, "6.5" is 6).
fn parse_port(s: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let st = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i64).min(1 << 40);
        i += 1;
    }
    if i == st || (neg && v != 0) || v > 65535 {
        return None;
    }
    Some(v)
}

/// parse_url(): `None` for a URL PHP considers seriously malformed (false).
///
/// Observed on PHP 8.5 (tests ext/standard/tests/url/parse_url_*.phpt and
/// fuzzing): the first `:` before any `?` or `#`, followed by 1 to 5 digits
/// and the end or a `/`, makes a "host:port" URL without a scheme (the
/// port is taken from there even when a `/` comes before it); otherwise a
/// non-empty run of scheme characters before that `:` is the scheme.
pub fn parse_url(s: &[u8]) -> Option<Url> {
    if s == b":" {
        return None;
    }
    let mut u = Url::default();
    let n = s.len();
    let colon = s
        .iter()
        .position(|&c| c == b':' || c == b'?' || c == b'#')
        .filter(|&p| s[p] == b':');
    let mut port_at: Option<(usize, i64)> = None;
    if let Some(p) = colon {
        let mut j = p + 1;
        while j < n && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > p + 1 && j - (p + 1) < 6 && (j == n || s[j] == b'/') {
            port_at = Some((p, parse_port(&s[p + 1..j])?));
        }
    }
    let mut rest: &[u8] = s;
    let mut authority = false;
    if s.starts_with(b"//") {
        rest = &s[2..];
        authority = true;
        port_at = None;
    } else if port_at.is_some() {
        authority = true;
    } else if let Some(p) = colon.filter(|&p| p > 0 && s[..p].iter().all(|&c| is_scheme_char(c))) {
        u.scheme = Some(clean(&s[..p]));
        rest = &s[p + 1..];
        if rest.is_empty() {
            return Some(u);
        }
        if rest.starts_with(b"//") {
            rest = &rest[2..];
            authority = true;
            if u.scheme
                .as_deref()
                .is_some_and(|sc| sc.eq_ignore_ascii_case(b"file"))
                && rest.starts_with(b"/")
            {
                // file:///path: no host; "/C:/..." loses its slash.
                authority = false;
                if rest.len() > 2 && rest[2] == b':' {
                    rest = &rest[1..];
                }
            }
        }
    }
    if authority {
        let end = rest
            .iter()
            .position(|&c| c == b'/' || c == b'?' || c == b'#')
            .unwrap_or(rest.len());
        let auth = &rest[..end];
        rest = &rest[end..];
        let host_port = match auth.iter().rposition(|&c| c == b'@') {
            Some(at) => {
                let ui = &auth[..at];
                match ui.iter().position(|&c| c == b':') {
                    Some(c) => {
                        u.user = Some(clean(&ui[..c]));
                        u.pass = Some(clean(&ui[c + 1..]));
                    }
                    None => u.user = Some(clean(ui)),
                }
                &auth[at + 1..]
            }
            None => auth,
        };
        let mut host = host_port;
        // An IPv6 literal "[...]" can only be followed by the port.
        let port_from = match host_port.iter().position(|&c| c == b']') {
            Some(p) if host_port[0] == b'[' => p,
            _ => 0,
        };
        if let Some(c) = host_port[port_from..]
            .iter()
            .rposition(|&c| c == b':')
            .map(|c| c + port_from)
        {
            host = &host_port[..c];
            let ps = &host_port[c + 1..];
            if ps.len() > 5 {
                return None;
            }
            if !ps.is_empty() {
                u.port = Some(parse_port(ps)?);
            }
        } else if let Some((p, port)) = port_at
            && p >= end
        {
            u.port = Some(port);
        }
        if host.is_empty() {
            return None;
        }
        u.host = Some(clean(host));
    }
    let (before_frag, frag) = match rest.iter().position(|&c| c == b'#') {
        Some(h) => (&rest[..h], Some(&rest[h + 1..])),
        None => (rest, None),
    };
    let (path, query) = match before_frag.iter().position(|&c| c == b'?') {
        Some(q) => (&before_frag[..q], Some(&before_frag[q + 1..])),
        None => (before_frag, None),
    };
    if !path.is_empty() || (!authority && query.is_none() && frag.is_none() && u.scheme.is_none()) {
        u.path = Some(clean(path));
    }
    u.query = query.map(clean);
    u.fragment = frag.map(clean);
    Some(u)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// base64_encode() (RFC 4648, with padding).
pub fn base64_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len().div_ceil(3) * 4);
    for ch in s.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64[(v >> 18) as usize & 63]);
        out.push(B64[(v >> 12) as usize & 63]);
        out.push(if ch.len() > 1 {
            B64[(v >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if ch.len() > 2 {
            B64[v as usize & 63]
        } else {
            b'='
        });
    }
    out
}

fn b64_val(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// base64_decode(): `None` is false. The non-strict mode skips characters
/// outside the alphabet; the strict mode rejects them (whitespace aside)
/// and checks the padding (tests base64_decode_basic_*.phpt, bug52327.phpt,
/// bug55273.phpt; observed on PHP 8.5).
pub fn base64_decode(s: &[u8], strict: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut n = 0usize; // sextets in the current quantum
    let mut pad = 0usize;
    for &c in s {
        if c == b'=' {
            pad += 1;
            continue;
        }
        let Some(v) = b64_val(c) else {
            if strict && !matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                return None;
            }
            continue;
        };
        if pad > 0 && strict {
            return None;
        }
        acc = acc << 6 | v;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            acc = 0;
            n = 0;
        }
    }
    match n {
        0 => {}
        1 => {
            if strict {
                return None;
            }
        }
        2 => out.push((acc >> 4) as u8),
        _ => {
            out.push((acc >> 10) as u8);
            out.push((acc >> 2) as u8);
        }
    }
    if strict && pad > 0 && (n < 2 || n + pad != 4) {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests;
