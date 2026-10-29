//! libcurl's cookie engine: Set-Cookie parsing and matching (RFC 6265),
//! the Netscape cookie file format of CURLOPT_COOKIEFILE/COOKIEJAR and
//! CURLINFO_COOKIELIST, and the CURLOPT_COOKIELIST commands (libcurl
//! documentation of CURLOPT_COOKIELIST and of the cookie file format;
//! listing order and output observed on PHP 8.5 with libcurl 8.18).

use std::time::{SystemTime, UNIX_EPOCH};

/// One cookie.
#[derive(Clone, Debug)]
pub struct Cookie {
    /// The domain as listed (".example.com" for domain cookies).
    pub domain: String,
    /// Whether subdomains match too (the second field of the file format).
    pub tailmatch: bool,
    pub path: String,
    pub secure: bool,
    /// Expiry as a Unix time; 0 for a session cookie.
    pub expires: i64,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    pub httponly: bool,
    /// Creation order.
    pub created: u64,
    /// Creation order of the first cookie of its domain (listing groups).
    pub group: u64,
}

/// A cookie store.
#[derive(Clone, Debug, Default)]
pub struct CookieJar {
    pub cookies: Vec<Cookie>,
    counter: u64,
    /// Whether the engine is on (a cookie file or jar was set, or cookies
    /// were added through CURLOPT_COOKIELIST).
    pub enabled: bool,
}

/// The current Unix time.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

impl CookieJar {
    pub fn new() -> CookieJar {
        CookieJar::default()
    }

    fn next(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    fn insert(&mut self, mut c: Cookie) {
        let now = now();
        let dkey = lower(c.domain.trim_start_matches('.'));
        let same = |o: &Cookie| {
            o.name == c.name && lower(o.domain.trim_start_matches('.')) == dkey && o.path == c.path
        };
        let expired = c.expires != 0 && c.expires <= now;
        if let Some(i) = self.cookies.iter().position(same) {
            if expired {
                self.cookies.remove(i);
                return;
            }
            c.created = self.cookies[i].created;
            c.group = self.cookies[i].group;
            self.cookies[i] = c;
            return;
        }
        if expired {
            return;
        }
        c.created = self.next();
        c.group = self
            .cookies
            .iter()
            .find(|o| lower(o.domain.trim_start_matches('.')) == dkey)
            .map(|o| o.group)
            .unwrap_or(c.created);
        self.cookies.push(c);
    }

    /// Adds the cookie of a Set-Cookie header value received from `host`
    /// for a request to `path` (both empty when it comes from
    /// CURLOPT_COOKIELIST or a cookie file).
    pub fn add_set_cookie(&mut self, header: &[u8], host: &str, path: &[u8]) {
        let mut parts = header.split(|&c| c == b';');
        let Some(first) = parts.next() else { return };
        let Some(eq) = first.iter().position(|&c| c == b'=') else {
            return;
        };
        let name = trim(&first[..eq]).to_vec();
        let value = trim(&first[eq + 1..]).to_vec();
        if name.is_empty() {
            return;
        }
        let mut c = Cookie {
            domain: host.to_string(),
            tailmatch: false,
            path: String::new(),
            secure: false,
            expires: 0,
            name,
            value,
            httponly: false,
            created: 0,
            group: 0,
        };
        let mut max_age: Option<i64> = None;
        for attr in parts {
            let attr = trim(attr);
            let (k, v) = match attr.iter().position(|&ch| ch == b'=') {
                Some(p) => (trim(&attr[..p]), trim(&attr[p + 1..])),
                None => (attr, &b""[..]),
            };
            let k = String::from_utf8_lossy(k).to_ascii_lowercase();
            let vs = String::from_utf8_lossy(v).into_owned();
            match k.as_str() {
                "expires" => {
                    if let Some(t) = parse_cookie_date(v) {
                        c.expires = t.max(1);
                    }
                }
                "max-age" => {
                    if let Ok(n) = vs.trim().parse::<i64>() {
                        max_age = Some(n);
                    }
                }
                "domain" => {
                    let d = vs.trim_start_matches('.').to_ascii_lowercase();
                    if d.is_empty() {
                        continue;
                    }
                    if !host.is_empty() {
                        let h = host.to_ascii_lowercase();
                        if h != d && !h.ends_with(&format!(".{d}")) {
                            // Not a domain the host may set cookies for.
                            return;
                        }
                    }
                    if d.parse::<std::net::IpAddr>().is_ok() {
                        // An address only matches itself (observed on PHP
                        // 8.5: "127.0.0.1 FALSE").
                        c.domain = d;
                        c.tailmatch = false;
                    } else {
                        c.domain = format!(".{d}");
                        c.tailmatch = true;
                    }
                }
                "path" => {
                    if vs.starts_with('/') {
                        c.path = vs;
                    }
                }
                "secure" => c.secure = true,
                "httponly" => c.httponly = true,
                _ => {}
            }
        }
        if let Some(n) = max_age {
            c.expires = if n <= 0 { 1 } else { now() + n };
        }
        if c.path.is_empty() {
            c.path = default_path(path);
        }
        self.insert(c);
    }

    /// Adds a line of the Netscape cookie file format (comments and blank
    /// lines are ignored; "Set-Cookie:" lines are headers).
    pub fn add_line(&mut self, line: &[u8]) {
        let line = trim_eol(line);
        if line.len() > 11 && line[..11].eq_ignore_ascii_case(b"set-cookie:") {
            self.add_set_cookie(trim(&line[11..]), "", b"");
            return;
        }
        let (line, httponly) = match line.strip_prefix(b"#HttpOnly_") {
            Some(l) => (l, true),
            None => (line, false),
        };
        if line.is_empty() || line.starts_with(b"#") {
            return;
        }
        let f: Vec<&[u8]> = line.split(|&c| c == b'\t').collect();
        if f.len() < 6 {
            return;
        }
        let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let c = Cookie {
            domain: s(f[0]),
            tailmatch: f[1].eq_ignore_ascii_case(b"TRUE"),
            path: s(f[2]),
            secure: f[3].eq_ignore_ascii_case(b"TRUE"),
            expires: s(f[4]).trim().parse().unwrap_or(0),
            name: f[5].to_vec(),
            value: f.get(6).map(|v| v.to_vec()).unwrap_or_default(),
            httponly,
            created: 0,
            group: 0,
        };
        self.insert(c);
    }

    /// Loads a cookie file's contents.
    pub fn load(&mut self, data: &[u8]) {
        for line in data.split(|&c| c == b'\n') {
            self.add_line(line);
        }
    }

    /// Removes every cookie.
    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    /// Removes the session cookies.
    pub fn clear_session(&mut self) {
        self.cookies.retain(|c| c.expires != 0);
    }

    fn drop_expired(&mut self) {
        let now = now();
        self.cookies.retain(|c| c.expires == 0 || c.expires > now);
    }

    /// The cookies in listing order: domains newest first, each domain's
    /// cookies in the order they were added.
    fn listing(&mut self) -> Vec<&Cookie> {
        self.drop_expired();
        let mut v: Vec<&Cookie> = self.cookies.iter().collect();
        v.sort_by(|a, b| b.group.cmp(&a.group).then(a.created.cmp(&b.created)));
        v
    }

    /// CURLINFO_COOKIELIST: one Netscape-format line per cookie.
    pub fn lines(&mut self) -> Vec<Vec<u8>> {
        self.listing().into_iter().map(format_line).collect()
    }

    /// The cookie jar file contents (CURLOPT_COOKIEJAR).
    pub fn jar_file(&mut self) -> Vec<u8> {
        let mut out = b"# Netscape HTTP Cookie File\n# https://curl.se/docs/http-cookies.html\n# This file was generated by libcurl! Edit at your own risk.\n\n".to_vec();
        // Newest first (observed on PHP 8.5 with libcurl 8.18).
        for l in self.lines().into_iter().rev() {
            out.extend_from_slice(&l);
            out.push(b'\n');
        }
        out
    }

    /// The Cookie header value for a request (None when no cookie
    /// matches): longest paths first, then newest first.
    pub fn header_for(&mut self, host: &str, path: &[u8], secure: bool) -> Option<Vec<u8>> {
        self.drop_expired();
        let h = host.to_ascii_lowercase();
        let mut m: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| {
                let d = c.domain.trim_start_matches('.').to_ascii_lowercase();
                let dom_ok = if c.tailmatch {
                    h == d || h.ends_with(&format!(".{d}"))
                } else {
                    h == d
                };
                dom_ok && path_match(path, c.path.as_bytes()) && (!c.secure || secure)
            })
            .collect();
        if m.is_empty() {
            return None;
        }
        m.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then(b.created.cmp(&a.created))
        });
        let mut out = Vec::new();
        for (i, c) in m.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(b"; ");
            }
            out.extend_from_slice(&c.name);
            out.push(b'=');
            out.extend_from_slice(&c.value);
        }
        Some(out)
    }
}

fn format_line(c: &Cookie) -> Vec<u8> {
    let mut l = Vec::new();
    if c.httponly {
        l.extend_from_slice(b"#HttpOnly_");
    }
    l.extend_from_slice(
        format!(
            "{}\t{}\t{}\t{}\t{}\t",
            c.domain,
            if c.tailmatch { "TRUE" } else { "FALSE" },
            c.path,
            if c.secure { "TRUE" } else { "FALSE" },
            c.expires
        )
        .as_bytes(),
    );
    l.extend_from_slice(&c.name);
    l.push(b'\t');
    l.extend_from_slice(&c.value);
    l
}

/// RFC 6265 section 5.1.4.
fn path_match(req: &[u8], cookie: &[u8]) -> bool {
    let req = match req.iter().position(|&c| c == b'?') {
        Some(p) => &req[..p],
        None => req,
    };
    if req == cookie {
        return true;
    }
    req.starts_with(cookie) && (cookie.ends_with(b"/") || req.get(cookie.len()) == Some(&b'/'))
}

/// RFC 6265 section 5.1.4: the directory of the request path.
fn default_path(path: &[u8]) -> String {
    let p = match path.iter().position(|&c| c == b'?') {
        Some(q) => &path[..q],
        None => path,
    };
    if !p.starts_with(b"/") {
        return "/".into();
    }
    match p.iter().rposition(|&c| c == b'/') {
        Some(0) | None => "/".into(),
        Some(i) => String::from_utf8_lossy(&p[..i]).into_owned(),
    }
}

fn trim(s: &[u8]) -> &[u8] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && (s[a] == b' ' || s[a] == b'\t') {
        a += 1;
    }
    while b > a && matches!(s[b - 1], b' ' | b'\t' | b'\r' | b'\n') {
        b -= 1;
    }
    &s[a..b]
}

fn trim_eol(s: &[u8]) -> &[u8] {
    let mut b = s.len();
    while b > 0 && matches!(s[b - 1], b'\r' | b'\n') {
        b -= 1;
    }
    &s[..b]
}

/// The cookie-date algorithm of RFC 6265 section 5.1.1, as a Unix time.
pub fn parse_cookie_date(s: &[u8]) -> Option<i64> {
    let is_delim = |c: u8| {
        c == 0x09
            || (0x20..=0x2f).contains(&c)
            || (0x3b..=0x40).contains(&c)
            || (0x5b..=0x60).contains(&c)
            || (0x7b..=0x7e).contains(&c)
    };
    let mut time: Option<(i64, i64, i64)> = None;
    let mut day: Option<i64> = None;
    let mut month: Option<i64> = None;
    let mut year: Option<i64> = None;
    for tok in s.split(|&c| is_delim(c)).filter(|t| !t.is_empty()) {
        if time.is_none()
            && let Some(t) = parse_time(tok)
        {
            time = Some(t);
            continue;
        }
        let digits = tok.iter().take_while(|c| c.is_ascii_digit()).count();
        if day.is_none() && (1..=2).contains(&digits) {
            day = Some(num(&tok[..digits]));
            continue;
        }
        if month.is_none() && tok.len() >= 3 {
            let m = tok[..3].to_ascii_lowercase();
            let months: [&[u8]; 12] = [
                b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct",
                b"nov", b"dec",
            ];
            if let Some(i) = months.iter().position(|x| *x == &m[..]) {
                month = Some(i as i64 + 1);
                continue;
            }
        }
        if year.is_none() && (2..=4).contains(&digits) {
            year = Some(num(&tok[..digits]));
            continue;
        }
    }
    let mut y = year?;
    if (70..=99).contains(&y) {
        y += 1900;
    } else if (0..=69).contains(&y) {
        y += 2000;
    }
    let (h, mi, se) = time?;
    let d = day?;
    let mo = month?;
    if !(1..=31).contains(&d) || y < 1601 || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se)
}

fn num(s: &[u8]) -> i64 {
    s.iter().fold(0i64, |a, &c| a * 10 + (c - b'0') as i64)
}

fn parse_time(tok: &[u8]) -> Option<(i64, i64, i64)> {
    let parts: Vec<&[u8]> = tok.split(|&c| c == b':').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut v = [0i64; 3];
    for (i, p) in parts.iter().enumerate() {
        let d = p.iter().take_while(|c| c.is_ascii_digit()).count();
        if !(1..=2).contains(&d) || (i < 2 && d != p.len()) {
            return None;
        }
        v[i] = num(&p[..d]);
    }
    Some((v[0], v[1], v[2]))
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(
            parse_cookie_date(b"Thu, 31-Dec-2037 23:59:59 GMT"),
            Some(2145916799)
        );
        assert_eq!(
            parse_cookie_date(b"Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784111777)
        );
    }

    #[test]
    fn list_and_header() {
        let mut j = CookieJar::new();
        j.add_set_cookie(
            b"C1=v1; expires=Thu, 31-Dec-2037 23:59:59 GMT; path=/; domain=.php.net",
            "",
            b"",
        );
        j.add_set_cookie(
            b"C2=v2; expires=Thu, 31-Dec-2037 23:59:59 GMT; path=/; domain=.php.net",
            "",
            b"",
        );
        let l = j.lines();
        assert_eq!(l[0], b".php.net\tTRUE\t/\tFALSE\t2145916799\tC1\tv1");
        assert_eq!(l[1], b".php.net\tTRUE\t/\tFALSE\t2145916799\tC2\tv2");
        let mut j = CookieJar::new();
        j.add_set_cookie(b"a=1; path=/", "127.0.0.1", b"/setcookie");
        j.add_set_cookie(b"b=2", "127.0.0.1", b"/setcookie");
        j.add_set_cookie(b"c=3; HttpOnly", "127.0.0.1", b"/setcookie");
        assert_eq!(
            j.header_for("127.0.0.1", b"/again", false).unwrap(),
            b"c=3; b=2; a=1"
        );
        assert_eq!(
            j.lines()[2],
            b"#HttpOnly_127.0.0.1\tFALSE\t/\tFALSE\t0\tc\t3"
        );
    }
}
