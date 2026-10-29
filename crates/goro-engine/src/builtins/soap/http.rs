//! The HTTP transport of `SoapClient::__doRequest()` (RFC 9110/9112
//! POST requests; SOAP 1.1 section 6 `SOAPAction`, SOAP 1.2 part 2
//! section 7 `application/soap+xml`): request headers in the order PHP
//! sends them, Basic authentication, cookies, keep-alive, gzip/deflate
//! responses, chunked bodies, redirects (observed on PHP 8.5; tests with
//! a local server in ext/soap/tests).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use super::encode::base64;
use super::str_prop;
use crate::vm::{R, Vm};

/// Set while developing: refuse hosts other than loopback ones and report
/// the attempt on stderr (so tests that would reach the Internet are
/// found and listed as online tests).
const DEV_REFUSE_REMOTE: bool = true;

pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub user: Option<String>,
    pub pass: Option<String>,
}

pub fn parse_url(s: &str) -> Option<Url> {
    let (scheme, rest) = s.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let (auth, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => match rest.find('?') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        },
    };
    let (userinfo, hostport) = match auth.rfind('@') {
        Some(i) => (Some(&auth[..i]), &auth[i + 1..]),
        None => (None, auth),
    };
    let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
        let end = h.find(']')?;
        let host = &h[..end];
        let port = h[end + 1..].strip_prefix(':').and_then(|p| p.parse().ok());
        (host.to_string(), port)
    } else {
        match hostport.rfind(':') {
            Some(i) => (hostport[..i].to_string(), hostport[i + 1..].parse().ok()),
            None => (hostport.to_string(), None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = port.unwrap_or(if scheme == "https" { 443 } else { 80 });
    let (user, pass) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
            None => (Some(u.to_string()), None),
        },
        None => (None, None),
    };
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_string()
    };
    Some(Url {
        scheme,
        host,
        port,
        path,
        user,
        pass,
    })
}

/// While developing: refuses (and logs) hosts other than loopback ones.
pub fn refuse_remote(host: &str) -> bool {
    if !DEV_REFUSE_REMOTE || is_loopback(host) {
        return false;
    }
    let script = std::env::args().collect::<Vec<_>>().join(" ");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/claude-1000/goro-work/soap/remote.log")
    {
        let _ = writeln!(f, "{} {}", host, script);
    }
    true
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

type Fault = (&'static str, String);

/// Sends a request: the response body, or a fault (code, message).
pub fn send(
    vm: &mut Vm,
    client: &ObjRef,
    request: &[u8],
    location: &str,
    action: &str,
    version: i64,
    one_way: bool,
) -> R<Result<Vec<u8>, Fault>> {
    let mut location = location.to_string();
    let mut challenge: Option<Vec<(String, String)>> = None;
    let mut tried_digest = false;
    for _redirect in 0..10 {
        let Some(url) = parse_url(&location) else {
            return Ok(Err(("HTTP", "Unable to parse URL".into())));
        };
        if url.scheme == "https" {
            return Ok(Err(("HTTP", "SSL support is not available in this build".into())));
        }
        if refuse_remote(&url.host) {
            return Ok(Err(("HTTP", "Could not connect to host".into())));
        }
        let head = request_head(
            vm,
            client,
            &url,
            request.len(),
            action,
            version,
            challenge.as_deref(),
        );
        let trace = matches!(vm.get_prop_raw(client, b"trace"), Value::Bool(true));
        if trace {
            vm.set_prop_raw(
                client,
                b"__last_request_headers",
                Value::Str(PhpStr::from(head.clone())),
            );
        }
        let timeout = match vm.get_prop_raw(client, b"_connection_timeout") {
            Value::Int(n) if n > 0 => Duration::from_secs(n as u64),
            _ => Duration::from_secs(60),
        };
        let addrs: Vec<_> = match (url.host.as_str(), url.port).to_socket_addrs() {
            Ok(a) => a.collect(),
            Err(_) => return Ok(Err(("HTTP", "Could not connect to host".into()))),
        };
        let mut stream = None;
        for a in addrs {
            if let Ok(s) = TcpStream::connect_timeout(&a, timeout) {
                stream = Some(s);
                break;
            }
        }
        let Some(mut s) = stream else {
            return Ok(Err(("HTTP", "Could not connect to host".into())));
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
        let mut msg = head.into_bytes();
        msg.extend_from_slice(request);
        if s.write_all(&msg).is_err() {
            return Ok(Err(("HTTP", "Failed Sending HTTP SOAP request".into())));
        }
        let resp = match read_response(&mut s) {
            Some(r) => r,
            None => {
                return Ok(Err(("HTTP", "Error Fetching http headers".into())));
            }
        };
        if trace {
            vm.set_prop_raw(
                client,
                b"__last_response_headers",
                Value::Str(PhpStr::from(format!("{}\r\n", resp.head))),
            );
        }
        // Cookies.
        for (k, v) in &resp.headers {
            if k.eq_ignore_ascii_case("set-cookie") {
                store_cookie(vm, client, v);
            }
        }
        if resp.status == 401
            && !tried_digest
            && matches!(vm.get_prop_raw(client, b"_use_digest"), Value::Bool(true))
            && let Some(ch) = header(&resp.headers, "www-authenticate").and_then(|v| parse_challenge(&v))
        {
            tried_digest = true;
            challenge = Some(ch);
            continue;
        }
        if matches!(resp.status, 301 | 302 | 303 | 307)
            && let Some(loc) = header(&resp.headers, "location")
        {
            location = if loc.contains("://") {
                loc
            } else {
                format!("{}://{}:{}{}", url.scheme, url.host, url.port, loc)
            };
            continue;
        }
        let mut body = resp.body;
        if let Some(enc) = header(&resp.headers, "content-encoding") {
            let e = enc.trim().to_ascii_lowercase();
            let dec = match e.as_str() {
                "gzip" | "x-gzip" => compcol_gunzip(&body),
                "deflate" => compcol_inflate(&body),
                _ => {
                    return Ok(Err((
                        "HTTP",
                        format!("Unknown Content-Encoding '{enc}'"),
                    )));
                }
            };
            match dec {
                Some(d) => body = d,
                None => return Ok(Err(("HTTP", "Can't uncompress compressed response".into()))),
            }
        }
        if resp.status >= 400 && body.is_empty() {
            return Ok(Err(("HTTP", resp.reason.clone())));
        }
        // One-way calls read the response but return nothing (observed on
        // PHP 8.5; test bugs/bug49278.phpt).
        if one_way {
            return Ok(Ok(Vec::new()));
        }
        return Ok(Ok(body));
    }
    Ok(Err(("HTTP", "Redirection limit reached, aborting".into())))
}

fn compcol_gunzip(b: &[u8]) -> Option<Vec<u8>> {
    goro_ext_zlib::gzdecode(b, 0).ok()
}

fn compcol_inflate(b: &[u8]) -> Option<Vec<u8>> {
    goro_ext_zlib::zlib_decode(b, 0).ok()
}

fn header(h: &[(String, String)], name: &str) -> Option<String> {
    h.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

fn store_cookie(vm: &mut Vm, client: &ObjRef, v: &str) {
    let first = v.split(';').next().unwrap_or("");
    let Some((name, value)) = first.split_once('=') else {
        return;
    };
    let mut cookies = match vm.get_prop_raw(client, b"_cookies") {
        Value::Array(c) => c,
        _ => PhpArray::new(),
    };
    let mut e = PhpArray::new();
    let _ = e.push(Value::Str(PhpStr::from(value.trim())));
    cookies.insert(Key::from(name.trim()), Value::Array(e));
    vm.set_prop_raw(client, b"_cookies", Value::Array(cookies));
}

fn md5_hex(s: &str) -> String {
    let d = goro_ext_hash::hash(
        b"md5",
        s.as_bytes(),
        &goro_ext_hash::HashOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_default();
    String::from_utf8(goro_ext_hash::to_hex(&d)).unwrap_or_default()
}

/// Parses a `WWW-Authenticate: Digest k="v", ...` challenge.
fn parse_challenge(v: &str) -> Option<Vec<(String, String)>> {
    let rest = v.trim().strip_prefix("Digest")?;
    let mut out = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i] == b' ' || b[i] == b',') {
            i += 1;
        }
        let ks = i;
        while i < b.len() && b[i] != b'=' {
            i += 1;
        }
        let k = rest[ks..i].trim().to_string();
        i += 1;
        let val = if i < b.len() && b[i] == b'"' {
            i += 1;
            let vs = i;
            while i < b.len() && b[i] != b'"' {
                i += 1;
            }
            let v = rest[vs..i.min(b.len())].to_string();
            i += 1;
            v
        } else {
            let vs = i;
            while i < b.len() && b[i] != b',' {
                i += 1;
            }
            rest[vs.min(b.len())..i.min(b.len())].trim().to_string()
        };
        if !k.is_empty() {
            out.push((k, val));
        }
    }
    Some(out)
}

/// RFC 2617 Digest credentials for a challenge.
fn digest_header(user: &str, pass: &str, uri: &str, ch: &[(String, String)]) -> String {
    let get = |k: &str| {
        ch.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.clone())
    };
    let realm = get("realm").unwrap_or_default();
    let nonce = get("nonce").unwrap_or_default();
    let nc = "00000001";
    let cnonce = md5_hex(&format!(
        "{}{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        std::process::id()
    ));
    let ha1 = md5_hex(&format!("{user}:{realm}:{pass}"));
    let ha2 = md5_hex(&format!("POST:{uri}"));
    let qop = get("qop");
    let mut h = format!(
        "Authorization: Digest username=\"{user}\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{uri}\""
    );
    match &qop {
        Some(q) if q.split(',').any(|x| x.trim() == "auth") => {
            let resp = md5_hex(&format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}"));
            h.push_str(&format!(
                ", qop=auth, nc={nc}, cnonce=\"{cnonce}\", response=\"{resp}\""
            ));
        }
        _ => {
            let resp = md5_hex(&format!("{ha1}:{nonce}:{ha2}"));
            h.push_str(&format!(", response=\"{resp}\""));
        }
    }
    if let Some(o) = get("opaque") {
        h.push_str(&format!(", opaque=\"{o}\""));
    }
    h.push_str("\r\n");
    h
}

#[allow(clippy::too_many_arguments)]
fn request_head(
    vm: &mut Vm,
    client: &ObjRef,
    url: &Url,
    len: usize,
    action: &str,
    version: i64,
    challenge: Option<&[(String, String)]>,
) -> String {
    let mut h = format!("POST {} HTTP/1.1\r\n", url.path);
    let default_port = if url.scheme == "https" { 443 } else { 80 };
    if url.port == default_port {
        h.push_str(&format!("Host: {}\r\n", url.host));
    } else {
        h.push_str(&format!("Host: {}:{}\r\n", url.host, url.port));
    }
    let keep = !matches!(vm.get_prop_raw(client, b"_keep_alive"), Value::Bool(false));
    if keep {
        h.push_str("Connection: Keep-Alive\r\n");
    } else {
        h.push_str("Connection: close\r\n");
    }
    let ua = str_prop(vm, client, "_user_agent")
        .unwrap_or_else(|| "PHP-SOAP/8.5.4".to_string());
    h.push_str(&format!("User-Agent: {ua}\r\n"));
    if let Value::Int(c) = vm.get_prop_raw(client, b"compression")
        && c & 32 != 0
    {
        h.push_str("Accept-Encoding: gzip, deflate\r\n");
    }
    // A context's http.content_type replaces the type (observed on PHP 8.5;
    // test custom_content_type.phpt).
    let ctx = vm.get_prop_raw(client, b"_stream_context");
    let custom = match super::super::sockets::context_option(&ctx, "http", "content_type") {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string_lossy()),
        _ => None,
    };
    if version == 2 {
        match &custom {
            Some(ct) => h.push_str(&format!("Content-Type: {ct}; action=\"{action}\"\r\n")),
            None => h.push_str(&format!(
                "Content-Type: application/soap+xml; charset=utf-8; action=\"{action}\"\r\n"
            )),
        }
    } else {
        match &custom {
            Some(ct) => h.push_str(&format!("Content-Type: {ct}\r\n")),
            None => h.push_str("Content-Type: text/xml; charset=utf-8\r\n"),
        }
        h.push_str(&format!("SOAPAction: \"{action}\"\r\n"));
    }
    h.push_str(&format!("Content-Length: {len}\r\n"));
    // The stream context's http.header lines, except those the client
    // writes itself (observed on PHP 8.5; test bugs/bug73182.phpt).
    let ctx = vm.get_prop_raw(client, b"_stream_context");
    if let Some(hv) = super::super::sockets::context_option(&ctx, "http", "header") {
        let lines: Vec<String> = match hv {
            Value::Array(a) => a
                .iter()
                .filter_map(|(_, v)| match v.deref() {
                    Value::Str(s) => Some(s.to_string_lossy()),
                    _ => None,
                })
                .collect(),
            Value::Str(s) => s
                .to_string_lossy()
                .split("\r\n")
                .flat_map(|l| l.split('\n'))
                .map(|l| l.to_string())
                .collect(),
            _ => Vec::new(),
        };
        for l in lines {
            let l = l.trim();
            if l.is_empty() {
                continue;
            }
            let name = l.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "host"
                    | "connection"
                    | "user-agent"
                    | "content-length"
                    | "content-type"
                    | "soapaction"
                    | "authorization"
                    | "proxy-authorization"
            ) {
                continue;
            }
            h.push_str(l);
            h.push_str("\r\n");
        }
    }
    let login = str_prop(vm, client, "_login").or(url.user.clone());
    let digest = matches!(vm.get_prop_raw(client, b"_use_digest"), Value::Bool(true));
    if let Some(l) = login {
        let p = str_prop(vm, client, "_password")
            .or(url.pass.clone())
            .unwrap_or_default();
        if digest {
            // Digest after the server's challenge (RFC 2617; test
            // bugs/bug55639.phpt).
            if let Some(ch) = challenge {
                h.push_str(&digest_header(&l, &p, &url.path, ch));
            }
        } else {
            h.push_str(&format!(
                "Authorization: Basic {}\r\n",
                base64(format!("{l}:{p}").as_bytes())
            ));
        }
    }
    if let Value::Array(c) = vm.get_prop_raw(client, b"_cookies")
        && !c.is_empty()
    {
        let mut parts = Vec::new();
        for (k, v) in c.iter() {
            let val = match v.deref() {
                Value::Array(a) => a
                    .get_int(0)
                    .map(|x| x.deref())
                    .and_then(|x| match x {
                        Value::Str(s) => Some(s.to_string_lossy()),
                        _ => None,
                    })
                    .unwrap_or_default(),
                _ => String::new(),
            };
            parts.push(format!(
                "{}={}",
                super::encode::key_str(k).to_string_lossy(),
                val
            ));
        }
        h.push_str(&format!("Cookie: {}\r\n", parts.join("; ")));
    }
    h.push_str("\r\n");
    h
}

struct Resp {
    status: i64,
    reason: String,
    head: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_response(s: &mut TcpStream) -> Option<Resp> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        let n = s.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut rest = buf[head_end + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status_line = lines.next()?;
    let mut sp = status_line.splitn(3, ' ');
    let _proto = sp.next()?;
    let status: i64 = sp.next()?.parse().ok()?;
    let reason = sp.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let chunked = header(&headers, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let body = if chunked {
        loop {
            if let Some(b) = dechunk(&rest) {
                break b;
            }
            let n = s.read(&mut tmp).ok()?;
            if n == 0 {
                break dechunk_partial(&rest);
            }
            rest.extend_from_slice(&tmp[..n]);
        }
    } else if let Some(len) = header(&headers, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        while rest.len() < len {
            let n = s.read(&mut tmp).ok()?;
            if n == 0 {
                break;
            }
            rest.extend_from_slice(&tmp[..n]);
        }
        rest.truncate(len);
        rest
    } else {
        loop {
            match s.read(&mut tmp) {
                Ok(0) | Err(_) => break,
                Ok(n) => rest.extend_from_slice(&tmp[..n]),
            }
        }
        rest
    };
    Some(Resp {
        status,
        reason,
        head,
        headers,
        body,
    })
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// A complete chunked body, or None when more data is needed.
fn dechunk(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let e = find(&b[i..], b"\r\n")? + i;
        let line = std::str::from_utf8(&b[i..e]).ok()?;
        let size = usize::from_str_radix(line.split(';').next()?.trim(), 16).ok()?;
        i = e + 2;
        if size == 0 {
            return Some(out);
        }
        if b.len() < i + size + 2 {
            return None;
        }
        out.extend_from_slice(&b[i..i + size]);
        i += size + 2;
    }
}

fn dechunk_partial(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(e) = find(&b[i..], b"\r\n").map(|e| e + i) {
        let Ok(line) = std::str::from_utf8(&b[i..e]) else {
            break;
        };
        let Ok(size) = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
        else {
            break;
        };
        i = e + 2;
        if size == 0 || b.len() < i + size {
            break;
        }
        out.extend_from_slice(&b[i..i + size]);
        i += size + 2;
    }
    out
}
