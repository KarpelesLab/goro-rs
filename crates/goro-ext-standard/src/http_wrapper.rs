//! The http:// and https:// stream wrapper (ext/standard/http_fopen_wrapper.c):
//! request building from the "http" context options, response status and
//! header handling, redirects and chunked bodies. The whole body is read
//! (the server closes the connection: "Connection: close" is sent).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use std::cell::RefCell;
use std::rc::Rc;

use super::{STREAM_CONTEXTS, resolve_context_id, xp_socket};

/// The "http" (and "ssl") options of a context argument.
fn context_options(ctx: Option<&Value>) -> (PhpArray, Option<PhpArray>) {
    let mut http = PhpArray::new();
    let mut ssl = None;
    match ctx.map(|c| c.deref()) {
        Some(Value::Array(a)) => {
            let a = a.borrow();
            if let Some(Value::Array(h)) = a.get(&ArrayKey::String(PhpString::from_bytes(b"http")))
            {
                http = h.borrow().clone();
            }
            if let Some(Value::Array(s)) = a.get(&ArrayKey::String(PhpString::from_bytes(b"ssl"))) {
                ssl = Some(s.borrow().clone());
            }
        }
        Some(v @ Value::Resource(_)) => {
            if let Some(id) = resolve_context_id(&v) {
                STREAM_CONTEXTS.with(|c| {
                    if let Some(ctx) = c.borrow().get(&id) {
                        if let Some(m) = ctx.options.get(b"http".as_slice()) {
                            for (k, v) in m {
                                http.set(ArrayKey::String(PhpString::from_bytes(k)), v.clone());
                            }
                        }
                    }
                });
                ssl = xp_socket::ssl_options(id);
            }
        }
        _ => {}
    }
    (http, ssl)
}

fn opt<'a>(http: &'a PhpArray, name: &str) -> Option<&'a Value> {
    http.get(&ArrayKey::String(PhpString::from_string(name.to_string())))
}

struct Url {
    https: bool,
    host: String,
    port: Option<u16>,
    path: String,
    user: Option<String>,
    pass: Option<String>,
}

fn parse_url(url: &str) -> Option<Url> {
    let (https, rest) = if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        let l = url.to_ascii_lowercase();
        if l.starts_with("http://") {
            (false, &url[7..])
        } else if l.starts_with("https://") {
            (true, &url[8..])
        } else {
            return None;
        }
    };
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
        let e = h.find(']')?;
        let port = h[e + 1..].strip_prefix(':').and_then(|p| p.parse().ok());
        (h[..e].to_string(), port)
    } else {
        match hostport.rfind(':') {
            Some(i) => (hostport[..i].to_string(), hostport[i + 1..].parse().ok()),
            None => (hostport.to_string(), None),
        }
    };
    let path = match path.find('#') {
        Some(i) => &path[..i],
        None => path,
    };
    let path = if path.is_empty() || path.starts_with('?') {
        format!("/{}", path)
    } else {
        path.to_string()
    };
    let (user, pass) = match userinfo {
        Some(u) => match u.find(':') {
            Some(i) => (Some(u[..i].to_string()), Some(u[i + 1..].to_string())),
            None => (Some(u.to_string()), None),
        },
        None => (None, None),
    };
    Some(Url {
        https,
        host,
        port,
        path,
        user,
        pass,
    })
}

/// check_has_header(): `header` at the start of a line of the
/// (lower-cased) user headers.
fn has_header(headers: &str, header: &str) -> bool {
    let mut start = 0;
    while let Some(p) = headers[start..].find(header) {
        let at = start + p;
        if at == 0 || headers.as_bytes()[at - 1] == b'\n' {
            return true;
        }
        start = at + 1;
    }
    false
}

fn url_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'+' {
            out.push(b' ');
        } else if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
            out.push(b[i]);
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in input.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// php_trim(..., " \t\n\r\0\x0B", 3)
fn php_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

/// Build the request (php_stream_url_wrap_http_ex()).
fn build_request(
    vm: &mut Vm,
    func: &str,
    url: &Url,
    http: &PhpArray,
    method: &str,
    with_content: bool,
) -> Vec<u8> {
    let mut req = String::new();
    req.push_str(method);
    req.push(' ');
    req.push_str(&url.path);
    match opt(http, "protocol_version") {
        Some(v) => req.push_str(&format!(" HTTP/{:.1}\r\n", v.to_double())),
        None => req.push_str(" HTTP/1.1\r\n"),
    }
    // user headers
    let mut user_headers: Option<String> = None;
    if let Some(h) = opt(http, "header") {
        let joined = match h.deref() {
            Value::Array(a) => {
                let mut s = String::new();
                for v in a.borrow().values() {
                    if let Value::String(x) = v.deref() {
                        s.push_str(&x.to_string_lossy());
                        s.push_str("\r\n");
                    }
                }
                s
            }
            Value::String(x) => x.to_string_lossy(),
            _ => String::new(),
        };
        let t = php_trim(&joined);
        if !t.is_empty() {
            user_headers = Some(t.to_string());
        }
    }
    let lower = user_headers.as_deref().unwrap_or("").to_ascii_lowercase();
    let have = |h: &str| has_header(&lower, h);
    let have_ua = have("user-agent:");
    let have_host = have("host:");
    let have_from = have("from:");
    let have_auth = have("authorization:");
    let mut have_cl = have("content-length:");
    let have_ct = have("content-type:");
    let have_conn = have("connection:");
    if !have_auth {
        if let Some(u) = &url.user {
            let mut scratch = url_decode(u);
            scratch.push(b':');
            if let Some(p) = &url.pass {
                scratch.extend_from_slice(&url_decode(p));
            }
            req.push_str(&format!("Authorization: Basic {}\r\n", base64(&scratch)));
        }
    }
    if !have_from {
        if let Some(from) = vm
            .constants
            .get(b"from".as_slice())
            .map(|v| v.to_php_string().to_string_lossy())
            .filter(|s| !s.is_empty())
        {
            req.push_str(&format!("From: {}\r\n", from));
        }
    }
    if !have_host {
        req.push_str("Host: ");
        req.push_str(&url.host);
        if let Some(p) = url.port {
            if (url.https && p != 443 && p != 0) || (!url.https && p != 80 && p != 0) {
                req.push_str(&format!(":{}", p));
            }
        }
        req.push_str("\r\n");
    }
    if !have_conn {
        req.push_str("Connection: close\r\n");
    }
    let ua = match opt(http, "user_agent").map(|v| v.deref()) {
        Some(Value::String(s)) => Some(s.to_string_lossy()),
        _ => vm
            .constants
            .get(b"user_agent".as_slice())
            .map(|v| v.to_php_string().to_string_lossy()),
    };
    if !have_ua {
        if let Some(ua) = ua.filter(|u| !u.is_empty()) {
            req.push_str(&format!("User-Agent: {}\r\n", ua));
        }
    }
    let content: Option<Vec<u8>> = if with_content {
        match opt(http, "content").map(|v| v.deref()) {
            Some(Value::String(s)) if !s.as_bytes().is_empty() => Some(s.as_bytes().to_vec()),
            _ => None,
        }
    } else {
        None
    };
    let mut out = Vec::new();
    if let Some(uh) = &user_headers {
        if let Some(c) = &content {
            if !have_cl {
                req.push_str(&format!("Content-Length: {}\r\n", c.len()));
                have_cl = true;
            }
        }
        req.push_str(uh);
        req.push_str("\r\n");
    }
    match content {
        Some(c) => {
            if !have_cl {
                req.push_str(&format!("Content-Length: {}\r\n", c.len()));
            }
            if !have_ct {
                req.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
                vm.emit_notice_at(
                    &format!(
                        "{}(): Content-type not specified assuming application/x-www-form-urlencoded",
                        func
                    ),
                    vm.current_line,
                );
            }
            req.push_str("\r\n");
            out.extend_from_slice(req.as_bytes());
            out.extend_from_slice(&c);
        }
        None => {
            req.push_str("\r\n");
            out.extend_from_slice(req.as_bytes());
        }
    }
    out
}

fn fail(vm: &mut Vm, func: &str, url: &str, msg: &str) {
    vm.emit_warning(&format!(
        "{}({}): Failed to open stream: {}",
        func, url, msg
    ));
}

/// Send `req` and read the raw response.
fn roundtrip(
    vm: &mut Vm,
    func: &str,
    orig_url: &str,
    url: &Url,
    ssl: Option<PhpArray>,
    req: &[u8],
    timeout: f64,
) -> Option<Vec<u8>> {
    let port = url.port.unwrap_or(if url.https { 443 } else { 80 });
    let addr = format!("{}:{}", url.host, port);
    if url.https {
        return xp_socket::tls_roundtrip(vm, func, addr.as_bytes(), ssl, req);
    }
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    let addrs: Vec<std::net::SocketAddr> = match (url.host.as_str(), port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(_) => {
            vm.emit_warning(&format!(
                "{}(): php_network_getaddresses: getaddrinfo for {} failed: Name or service not known",
                func, url.host
            ));
            fail(
                vm,
                func,
                orig_url,
                "php_network_getaddresses: getaddrinfo for host failed",
            );
            return None;
        }
    };
    let to = if timeout > 0.0 {
        Some(std::time::Duration::from_secs_f64(timeout))
    } else {
        None
    };
    let mut last_err = String::from("Connection refused");
    let mut stream = None;
    for a in addrs {
        let r = match to {
            Some(t) => std::net::TcpStream::connect_timeout(&a, t),
            None => std::net::TcpStream::connect(a),
        };
        match r {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => {
                last_err = super::strip_os_error(&e.to_string());
            }
        }
    }
    let Some(mut s) = stream else {
        fail(vm, func, orig_url, &last_err);
        return None;
    };
    let _ = s.set_read_timeout(to);
    let _ = s.write_all(req);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    Some(buf)
}

/// The dechunk filter.
fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let line_end = match body[i..].windows(2).position(|w| w == b"\r\n") {
            Some(p) => i + p,
            None => break,
        };
        let size_s = String::from_utf8_lossy(&body[i..line_end]);
        let size_s = size_s.split(';').next().unwrap_or("").trim().to_string();
        let Ok(size) = usize::from_str_radix(&size_s, 16) else {
            break;
        };
        i = line_end + 2;
        if size == 0 {
            break;
        }
        let end = (i + size).min(body.len());
        out.extend_from_slice(&body[i..end]);
        i = end + 2;
    }
    out
}

/// Open an http(s):// URL: Some(body) on success, None after the
/// warnings. `$http_response_header` is set.
pub(super) fn http_open(
    vm: &mut Vm,
    func: &str,
    orig_url: &str,
    ctx: Option<&Value>,
) -> Option<Vec<u8>> {
    http_open_ex(vm, func, orig_url, ctx, false).map(|(b, _)| b)
}

/// php_stream_url_wrap_http_ex(); `headers_only` = STREAM_ONLY_GET_HEADERS.
pub(super) fn http_open_ex(
    vm: &mut Vm,
    func: &str,
    orig_url: &str,
    ctx: Option<&Value>,
    headers_only: bool,
) -> Option<(Vec<u8>, PhpArray)> {
    let (http, ssl) = context_options(ctx);
    let mut cur = orig_url.to_string();
    let mut method = match opt(&http, "method").map(|v| v.deref()) {
        Some(Value::String(s)) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => "GET".to_string(),
    };
    let ignore_errors = headers_only || opt(&http, "ignore_errors").is_some_and(|v| v.is_truthy());
    let mut redirect_max: i64 = match opt(&http, "max_redirects") {
        Some(v) => v.to_long(),
        None => 20,
    };
    let timeout = match opt(&http, "timeout") {
        Some(v) => v.to_double(),
        None => vm
            .constants
            .get(b"default_socket_timeout".as_slice())
            .map(|v| v.to_double())
            .unwrap_or(60.0),
    };
    let mut headers_arr = PhpArray::new();
    let mut first = true;
    loop {
        let Some(url) = parse_url(&cur) else {
            fail(vm, func, orig_url, "HTTP request failed!");
            return None;
        };
        let req = build_request(vm, func, &url, &http, &method, first);
        let raw = roundtrip(vm, func, orig_url, &url, ssl.clone(), &req, timeout)?;
        // status line
        let Some(sl_end) = raw.iter().position(|&b| b == b'\n') else {
            fail(vm, func, orig_url, "HTTP request failed!");
            set_response_header(vm, headers_arr);
            return None;
        };
        let mut pos = sl_end + 1;
        let mut status_line = raw[..pos].to_vec();
        let code_of = |l: &[u8]| -> i64 {
            if l.len() > 9 {
                let d: Vec<u8> = l[9..]
                    .iter()
                    .copied()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                String::from_utf8_lossy(&d).parse().unwrap_or(0)
            } else {
                0
            }
        };
        let mut response_code = code_of(&status_line);
        if (100..200).contains(&response_code) && response_code != 101 {
            // skip to the real status line
            while let Some(e) = raw[pos..].iter().position(|&b| b == b'\n') {
                let line = raw[pos..pos + e + 1].to_vec();
                pos += e + 1;
                if line.len() >= 6 && line[..6].eq_ignore_ascii_case(b"HTTP/1") {
                    status_line = line;
                    break;
                }
            }
            response_code = code_of(&status_line);
        }
        let reqok = ignore_errors || (200..400).contains(&response_code);
        let mut trimmed = status_line.clone();
        while matches!(trimmed.last(), Some(b'\n') | Some(b'\r')) {
            trimmed.pop();
        }
        headers_arr.push(Value::String(PhpString::from_bytes(&trimmed)));
        // headers
        let mut location: Option<String> = None;
        let mut follow_location = true;
        let mut chunked = false;
        loop {
            let Some(e) = raw[pos..].iter().position(|&b| b == b'\n') else {
                pos = raw.len();
                break;
            };
            let mut line = raw[pos..pos + e].to_vec();
            pos += e + 1;
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                break;
            }
            while matches!(line.last(), Some(b' ') | Some(b'\t')) {
                line.pop();
            }
            let l = String::from_utf8_lossy(&line).to_string();
            if l.len() >= 9 && l[..9].eq_ignore_ascii_case("location:") {
                match opt(&http, "follow_location") {
                    Some(v) => follow_location = v.is_truthy(),
                    None => {
                        if !((300..304).contains(&response_code)
                            || response_code == 307
                            || response_code == 308)
                        {
                            follow_location = false;
                        }
                    }
                }
                location = Some(l[9..].trim_start_matches([' ', '\t']).to_string());
            } else if l.len() >= 18 && l[..18].eq_ignore_ascii_case("transfer-encoding:") {
                let v = l[18..].trim().to_ascii_lowercase();
                let decode = opt(&http, "auto_decode").is_none_or(|v| v.is_truthy());
                if v == "chunked" && decode {
                    chunked = true;
                    continue; // the header is not stored
                }
            }
            headers_arr.push(Value::String(PhpString::from_bytes(&line)));
        }
        let body = &raw[pos.min(raw.len())..];
        if !reqok || (location.is_some() && follow_location) {
            if !follow_location || (ignore_errors && redirect_max <= 1) {
                if !reqok {
                    set_response_header(vm, headers_arr);
                    fail(
                        vm,
                        func,
                        orig_url,
                        &format!(
                            "HTTP request failed! {}",
                            String::from_utf8_lossy(&status_line)
                        ),
                    );
                    return None;
                }
            } else if let Some(loc) = location {
                if redirect_max <= 1 {
                    set_response_header(vm, headers_arr);
                    fail(vm, func, orig_url, "Redirection limit reached, aborting");
                    return None;
                }
                redirect_max -= 1;
                let l = loc.to_ascii_lowercase();
                cur = if l.starts_with("http://") || l.starts_with("https://") {
                    loc
                } else {
                    let scheme = if url.https { "https" } else { "http" };
                    let hostport = match url.port {
                        Some(p) if (url.https && p != 443) || (!url.https && p != 80) => {
                            format!("{}:{}", url.host, p)
                        }
                        _ => url.host.clone(),
                    };
                    let path = if loc.starts_with('/') {
                        loc
                    } else {
                        let base = match url.path.rfind('/') {
                            Some(i) => &url.path[..=i],
                            None => "/",
                        };
                        format!("{}{}", base, loc)
                    };
                    format!("{}://{}{}", scheme, hostport, path)
                };
                if !(response_code == 307 || response_code == 308) {
                    method = "GET".to_string();
                    first = false;
                }
                continue;
            } else if !reqok {
                set_response_header(vm, headers_arr);
                fail(
                    vm,
                    func,
                    orig_url,
                    &format!(
                        "HTTP request failed! {}",
                        String::from_utf8_lossy(&status_line)
                    ),
                );
                return None;
            }
        }
        set_response_header(vm, headers_arr.clone());
        let body = if chunked {
            dechunk(body)
        } else {
            body.to_vec()
        };
        return Some((body, headers_arr));
    }
}

/// get_headers()
pub(super) fn get_headers(vm: &mut Vm, args: &[Value]) -> Value {
    let url = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let assoc = args.get(1).is_some_and(|v| v.is_truthy());
    let Some((_, headers)) = http_open_ex(vm, "get_headers", &url, args.get(2), true) else {
        return Value::False;
    };
    if !assoc {
        return Value::Array(Rc::new(RefCell::new(headers)));
    }
    let mut out = PhpArray::new();
    for v in headers.values() {
        let line = v.to_php_string().to_string_lossy();
        match line.find(':') {
            Some(p) => {
                let k = ArrayKey::String(PhpString::from_string(line[..p].to_string()));
                let val = Value::String(PhpString::from_string(
                    line[p + 1..].trim_start_matches([' ', '\t']).to_string(),
                ));
                match out.get(&k).cloned() {
                    Some(Value::Array(a)) => a.borrow_mut().push(val),
                    Some(prev) => {
                        let mut a = PhpArray::new();
                        a.push(prev);
                        a.push(val);
                        out.set(k, Value::Array(Rc::new(RefCell::new(a))));
                    }
                    None => out.set(k, val),
                }
            }
            None => out.push(Value::String(PhpString::from_string(line))),
        }
    }
    Value::Array(Rc::new(RefCell::new(out)))
}

fn set_response_header(vm: &mut Vm, headers: PhpArray) {
    vm.set_global(
        b"http_response_header".to_vec(),
        Value::Array(Rc::new(RefCell::new(headers))),
    );
}
