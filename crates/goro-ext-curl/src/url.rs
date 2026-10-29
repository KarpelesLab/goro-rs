//! URLs as libcurl takes them (libcurl's URL syntax documentation,
//! curl_url_set(3); RFC 3986 for the generic syntax and the resolution of
//! relative references): a scheme guessed from the host name when missing,
//! userinfo, IPv6 literals, ports, and the error texts libcurl reports
//! (observed on PHP 8.5 with libcurl 8.18).

/// A parsed URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    /// The scheme in lower case ("http").
    pub scheme: String,
    /// The user and password of the userinfo, percent-decoded.
    pub user: Option<Vec<u8>>,
    pub password: Option<Vec<u8>>,
    /// The host: a name, an IPv4 address, or an IPv6 address without its
    /// brackets.
    pub host: String,
    /// Whether the host was written in brackets.
    pub ipv6: bool,
    /// The port written in the URL, if any.
    pub port: Option<u16>,
    /// The path ("/" at least) and the query (with its "?"), without the
    /// fragment.
    pub path: Vec<u8>,
    /// The query part with its "?" (empty when none).
    pub query: Vec<u8>,
}

/// Why a URL was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrlError {
    /// No URL (CURLE_URL_MALFORMAT "No URL set").
    Empty,
    /// A malformed URL: the text after "URL rejected: ".
    Malformed(&'static str),
}

/// The default port of a scheme.
pub fn default_port(scheme: &str) -> Option<u16> {
    Some(match scheme {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        "ftp" => 21,
        "ftps" => 990,
        "dict" => 2628,
        "gopher" => 70,
        "imap" => 143,
        "imaps" => 993,
        "pop3" => 110,
        "pop3s" => 995,
        "smtp" => 25,
        "smtps" => 465,
        "telnet" => 23,
        "tftp" => 69,
        "ldap" => 389,
        "ldaps" => 636,
        "rtsp" => 554,
        "scp" | "sftp" => 22,
        "smb" | "smbs" => 445,
        "mqtt" => 1883,
        _ => return None,
    })
}

/// The scheme of `url` ("http" for "HTTP://x"), when it has one.
pub fn scheme_of(url: &[u8]) -> Option<String> {
    let p = url.windows(3).position(|w| w == b"://")?;
    let s = &url[..p];
    if s.is_empty()
        || !s[0].is_ascii_alphabetic()
        || !s
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some(String::from_utf8_lossy(s).to_ascii_lowercase())
}

/// The scheme libcurl guesses for a URL without one, from the first label
/// of the host name (libcurl documentation of CURLOPT_URL: "ftp." gives
/// FTP, "dict." DICT, ... anything else HTTP).
fn guess_scheme(rest: &[u8], default: &str) -> String {
    let l: Vec<u8> = rest.iter().map(|c| c.to_ascii_lowercase()).collect();
    for (p, s) in [
        (&b"ftp."[..], "ftp"),
        (b"dict.", "dict"),
        (b"ldap.", "ldap"),
        (b"imap.", "imap"),
        (b"smtp.", "smtp"),
        (b"pop3.", "pop3"),
    ] {
        if l.starts_with(p) {
            return s.to_string();
        }
    }
    default.to_string()
}

/// Decodes %XX escapes (invalid ones are kept as they are).
pub fn unescape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%'
            && i + 2 < s.len()
            && s[i + 1].is_ascii_hexdigit()
            && s[i + 2].is_ascii_hexdigit()
        {
            out.push((hexval(s[i + 1]) << 4) | hexval(s[i + 2]));
            i += 3;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

fn hexval(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    }
}

/// curl_easy_escape(3): every byte but the RFC 3986 unreserved characters
/// as %XX (upper case).
pub fn escape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3);
    for &c in s {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~') {
            out.push(c);
        } else {
            out.extend_from_slice(format!("%{c:02X}").as_bytes());
        }
    }
    out
}

impl Url {
    /// Parses a URL; `default_scheme` is used when it has none (and its
    /// host name does not suggest one).
    pub fn parse(input: &[u8], default_scheme: &str) -> Result<Url, UrlError> {
        if input.is_empty() {
            return Err(UrlError::Empty);
        }
        if input.iter().any(|&c| c < b' ' || c == 0x7f) {
            return Err(UrlError::Malformed("Malformed input to a URL function"));
        }
        let (scheme, rest) = match scheme_of(input) {
            Some(s) => {
                let p = input.windows(3).position(|w| w == b"://").unwrap_or(0);
                (s, &input[p + 3..])
            }
            None => {
                // "scheme:" without slashes, or no scheme at all.
                (guess_scheme(input, default_scheme), input)
            }
        };
        // The fragment is never sent.
        let rest = match rest.iter().position(|&c| c == b'#') {
            Some(p) => &rest[..p],
            None => rest,
        };
        let auth_end = rest
            .iter()
            .position(|&c| c == b'/' || c == b'?')
            .unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(auth_end);
        let (userinfo, hostport) = match authority.iter().rposition(|&c| c == b'@') {
            Some(p) => (Some(&authority[..p]), &authority[p + 1..]),
            None => (None, authority),
        };
        let (user, password) = match userinfo {
            None => (None, None),
            Some(u) => match u.iter().position(|&c| c == b':') {
                Some(p) => (Some(unescape(&u[..p])), Some(unescape(&u[p + 1..]))),
                None => (Some(unescape(u)), None),
            },
        };
        let (host, ipv6, port_part) = if let Some(r) = hostport.strip_prefix(b"[") {
            let Some(end) = r.iter().position(|&c| c == b']') else {
                return Err(UrlError::Malformed("Bad IPv6 address"));
            };
            let h = &r[..end];
            if h.is_empty()
                || !h
                    .iter()
                    .all(|&c| c.is_ascii_hexdigit() || c == b':' || c == b'.' || c == b'%')
            {
                return Err(UrlError::Malformed("Bad IPv6 address"));
            }
            let after = &r[end + 1..];
            let port = match after.strip_prefix(b":") {
                Some(p) => Some(p),
                None if after.is_empty() => None,
                None => return Err(UrlError::Malformed("Bad IPv6 address")),
            };
            (String::from_utf8_lossy(h).into_owned(), true, port)
        } else {
            match hostport.iter().rposition(|&c| c == b':') {
                Some(p) => (
                    String::from_utf8_lossy(&hostport[..p]).into_owned(),
                    false,
                    Some(&hostport[p + 1..]),
                ),
                None => (String::from_utf8_lossy(hostport).into_owned(), false, None),
            }
        };
        let port = match port_part {
            None => None,
            Some([]) => None,
            Some(p) => {
                if !p.iter().all(|c| c.is_ascii_digit()) || p.len() > 5 {
                    return Err(UrlError::Malformed(
                        "Port number was not a decimal number between 0 and 65535",
                    ));
                }
                let n: u32 = std::str::from_utf8(p)
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(70000);
                if n > 65535 {
                    return Err(UrlError::Malformed(
                        "Port number was not a decimal number between 0 and 65535",
                    ));
                }
                Some(n as u16)
            }
        };
        let (path, query) = match tail.iter().position(|&c| c == b'?') {
            Some(p) => (tail[..p].to_vec(), tail[p..].to_vec()),
            None => (tail.to_vec(), Vec::new()),
        };
        let mut path = if path.is_empty() { b"/".to_vec() } else { path };
        if scheme != "file" {
            path = remove_dot_segments(&path);
        }
        if scheme == "file" {
            let h = host.to_ascii_lowercase();
            if !(h.is_empty() || h == "localhost" || h == "127.0.0.1") {
                return Err(UrlError::Malformed(
                    "Invalid file://hostname/, expected localhost or 127.0.0.1 or none",
                ));
            }
        } else if host.is_empty() {
            return Err(UrlError::Malformed("No host part in the URL"));
        }
        if !ipv6
            && host.bytes().any(|c| {
                matches!(
                    c,
                    b' ' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}' | b'"'
                )
            })
        {
            return Err(UrlError::Malformed("Bad hostname"));
        }
        Ok(Url {
            scheme,
            user,
            password,
            host,
            ipv6,
            port,
            path,
            query,
        })
    }

    /// The port to connect to.
    pub fn port_or_default(&self) -> u16 {
        self.port
            .or_else(|| default_port(&self.scheme))
            .unwrap_or(0)
    }

    /// The host as written in a URL or Host header ("[::1]" for IPv6).
    pub fn host_display(&self) -> String {
        if self.ipv6 {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        }
    }

    /// The Host header value: the host and a non-default port.
    pub fn host_header(&self) -> String {
        match self.port {
            Some(p) if Some(p) != default_port(&self.scheme) => {
                format!("{}:{}", self.host_display(), p)
            }
            _ => self.host_display(),
        }
    }

    /// The request target: path and query.
    pub fn target(&self) -> Vec<u8> {
        let mut t = self.path.clone();
        t.extend_from_slice(&self.query);
        t
    }

    /// The URL as text (without the userinfo, as libcurl reports the
    /// effective URL).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = format!("{}://", self.scheme).into_bytes();
        s.extend_from_slice(self.host_display().as_bytes());
        if let Some(p) = self.port {
            s.extend_from_slice(format!(":{p}").as_bytes());
        }
        s.extend_from_slice(&self.target());
        s
    }

    /// Resolves a reference (a Location header) against this URL
    /// (RFC 3986 section 5.2).
    pub fn resolve(&self, reference: &[u8]) -> Vec<u8> {
        let r = trim(reference);
        if scheme_of(r).is_some() {
            return r.to_vec();
        }
        let r = match r.iter().position(|&c| c == b'#') {
            Some(p) => &r[..p],
            None => r,
        };
        let base_auth = {
            let mut a = format!("{}://", self.scheme).into_bytes();
            if let Some(u) = &self.user {
                a.extend_from_slice(&escape_userinfo(u));
                if let Some(p) = &self.password {
                    a.push(b':');
                    a.extend_from_slice(&escape_userinfo(p));
                }
                a.push(b'@');
            }
            a.extend_from_slice(self.host_display().as_bytes());
            if let Some(p) = self.port {
                a.extend_from_slice(format!(":{p}").as_bytes());
            }
            a
        };
        if let Some(rest) = r.strip_prefix(b"//") {
            let mut s = format!("{}://", self.scheme).into_bytes();
            s.extend_from_slice(rest);
            return s;
        }
        let mut out = base_auth;
        if r.is_empty() {
            out.extend_from_slice(&self.target());
            return out;
        }
        if r.starts_with(b"?") {
            out.extend_from_slice(&self.path);
            out.extend_from_slice(r);
            return out;
        }
        let (rpath, rquery) = match r.iter().position(|&c| c == b'?') {
            Some(p) => (&r[..p], &r[p..]),
            None => (r, &b""[..]),
        };
        let merged = if rpath.starts_with(b"/") {
            rpath.to_vec()
        } else {
            let mut m = match self.path.iter().rposition(|&c| c == b'/') {
                Some(p) => self.path[..=p].to_vec(),
                None => b"/".to_vec(),
            };
            m.extend_from_slice(rpath);
            m
        };
        out.extend_from_slice(&remove_dot_segments(&merged));
        out.extend_from_slice(rquery);
        out
    }
}

fn escape_userinfo(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &c in s {
        if c.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=".contains(&c) {
            out.push(c);
        } else {
            out.extend_from_slice(format!("%{c:02X}").as_bytes());
        }
    }
    out
}

fn trim(s: &[u8]) -> &[u8] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && s[a].is_ascii_whitespace() {
        a += 1;
    }
    while b > a && s[b - 1].is_ascii_whitespace() {
        b -= 1;
    }
    &s[a..b]
}

/// RFC 3986 section 5.2.4.
pub fn remove_dot_segments(path: &[u8]) -> Vec<u8> {
    let mut input = path.to_vec();
    let mut out: Vec<u8> = Vec::new();
    while !input.is_empty() {
        if input.starts_with(b"../") {
            input.drain(..3);
        } else if input.starts_with(b"./") || input.starts_with(b"/./") {
            input.drain(..2);
        } else if input == b"/." {
            input = b"/".to_vec();
        } else if input.starts_with(b"/../") {
            input.drain(..3);
            pop_segment(&mut out);
        } else if input == b"/.." {
            input = b"/".to_vec();
            pop_segment(&mut out);
        } else if input == b"." || input == b".." {
            input.clear();
        } else {
            let start = usize::from(input[0] == b'/');
            let end = input[start..]
                .iter()
                .position(|&c| c == b'/')
                .map(|p| p + start)
                .unwrap_or(input.len());
            out.extend_from_slice(&input[..end]);
            input.drain(..end);
        }
    }
    out
}

fn pop_segment(out: &mut Vec<u8>) {
    match out.iter().rposition(|&c| c == b'/') {
        Some(p) => out.truncate(p),
        None => out.clear(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let u = Url::parse(b"localhost:8080/get.inc?test=1#frag", "http").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "localhost");
        assert_eq!(u.port, Some(8080));
        assert_eq!(u.target(), b"/get.inc?test=1");
        assert_eq!(u.host_header(), "localhost:8080");
        let u = Url::parse(b"HTTP://u:p%40@[::1]/", "http").unwrap();
        assert_eq!(u.host, "::1");
        assert!(u.ipv6);
        assert_eq!(u.password.as_deref(), Some(&b"p@"[..]));
        assert_eq!(u.host_header(), "[::1]");
        assert_eq!(
            Url::parse(b"http://", "http"),
            Err(UrlError::Malformed("No host part in the URL"))
        );
        assert_eq!(
            Url::parse(b"ftp.example.com/x", "http").unwrap().scheme,
            "ftp"
        );
    }

    #[test]
    fn resolve_refs() {
        let u = Url::parse(b"http://a/b/c/d;p?q", "http").unwrap();
        assert_eq!(u.resolve(b"g"), b"http://a/b/c/g");
        assert_eq!(u.resolve(b"../g"), b"http://a/b/g");
        assert_eq!(u.resolve(b"/g?x"), b"http://a/g?x");
        assert_eq!(u.resolve(b"//other/x"), b"http://other/x");
        assert_eq!(u.resolve(b"?y"), b"http://a/b/c/d;p?y");
        assert_eq!(u.resolve(b"https://z/"), b"https://z/");
    }

    #[test]
    fn escapes() {
        assert_eq!(escape(b"a\0b c/~-._"), b"a%00b%20c%2F~-._");
        assert_eq!(unescape(b"a%zz%2%41+b%"), b"a%zz%2A+b%");
        assert_eq!(unescape(b"%4"), b"%4");
    }
}
