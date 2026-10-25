//! The http:// and https:// stream wrapper (ext/standard/http_fopen_wrapper.c):
//! request building from the "http" context options, response status and
//! header handling, redirects and chunked bodies. The stream opened is the
//! socket stream of the transport, positioned at the body (with a dechunk
//! read filter for chunked responses).

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
    // php_stream_context_from_zval(NULL, 0): FG(default_context)
    let ctx = match ctx.map(|c| c.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => crate::misc::DEFAULT_STREAM_CTX
            .with(|d| d.get())
            .map(Value::resource),
        other => other,
    };
    match ctx {
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
        // (the brackets are kept: "[::1]")
        (format!("[{}]", &h[..e]), port)
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
        // at the start, or after a "\r\n" (a bare "\n" does not count)
        if at == 0 || (at >= 2 && headers.as_bytes()[at - 2..at] == *b"\r\n") {
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

thread_local! {
    /// BG(last_http_headers): the response headers of the last http://
    /// request, for http_get_last_response_headers().
    static LAST_HTTP_HEADERS: RefCell<Option<PhpArray>> = const { RefCell::new(None) };
}

/// http_get_last_response_headers()
pub(super) fn last_response_headers() -> Value {
    LAST_HTTP_HEADERS.with(|h| match &*h.borrow() {
        Some(a) => Value::Array(Rc::new(RefCell::new(a.clone()))),
        None => Value::Null,
    })
}

/// http_clear_last_response_headers()
pub(super) fn clear_last_response_headers() {
    LAST_HTTP_HEADERS.with(|h| *h.borrow_mut() = None);
}

// php_stream_notify codes and severities.
const NOTIFY_CONNECT: i64 = 2;
const NOTIFY_AUTH_REQUIRED: i64 = 3;
const NOTIFY_MIME_TYPE_IS: i64 = 4;
const NOTIFY_FILE_SIZE_IS: i64 = 5;
const NOTIFY_REDIRECTED: i64 = 6;
const NOTIFY_FAILURE: i64 = 9;
const NOTIFY_AUTH_RESULT: i64 = 10;
const SEVERITY_INFO: i64 = 0;
const SEVERITY_ERR: i64 = 2;

/// php_stream_notification_func(): call the context's "notification"
/// callback, if any.
fn notify(
    vm: &mut Vm,
    ctx: Option<&Value>,
    code: i64,
    severity: i64,
    msg: Option<&[u8]>,
    msg_code: i64,
    sofar: i64,
    max: i64,
) {
    // (after what the socket I/O queued)
    xp_socket::flush_notifications(vm);
    if vm.current_exception.is_some() {
        return;
    }
    let Some(id) = context_id(ctx) else {
        return;
    };
    let Some(cb) =
        STREAM_CONTEXTS.with(|c| c.borrow().get(&id).and_then(|c| c.notification.clone()))
    else {
        return;
    };
    let args = [
        cb,
        Value::Long(code),
        Value::Long(severity),
        match msg {
            Some(m) => Value::String(PhpString::from_bytes(m)),
            None => Value::Null,
        },
        Value::Long(msg_code),
        Value::Long(sofar),
        Value::Long(max),
    ];
    let _ = super::call_user_func(vm, &args);
}

/// php_stream_http_response_header_trim(): strip the line ending and
/// trailing blanks; true when blanks were stripped.
fn header_trim(line: &mut Vec<u8>) -> bool {
    while matches!(line.last(), Some(b'\n') | Some(b'\r')) {
        line.pop();
    }
    let space = matches!(line.last(), Some(b' ') | Some(b'\t'));
    while matches!(line.last(), Some(b' ') | Some(b'\t')) {
        line.pop();
    }
    space
}

const HTTP_HEADER_MAX_LOCATION_SIZE: usize = 8182;

/// php_stream_http_response_header_info
struct HeaderInfo {
    chunked: bool,
    file_size: i64,
    error: Option<String>,
    follow_location: bool,
    location: Option<Vec<u8>>,
}

/// php_stream_http_response_headers_parse(): fold `line` into the pending
/// header, or store the pending header. Returns the header still pending.
#[allow(clippy::too_many_arguments)]
fn headers_parse(
    vm: &mut Vm,
    ctx: Option<&Value>,
    http: &PhpArray,
    headers_only: bool,
    mut last: Vec<u8>,
    line: Option<&mut Vec<u8>>,
    response_code: i64,
    headers: &mut PhpArray,
    info: &mut HeaderInfo,
) -> Option<Vec<u8>> {
    if let Some(line) = line {
        if !matches!(line.first(), Some(b'\n') | Some(b'\r')) {
            if header_trim(line) && line.is_empty() {
                // Only blanks: an empty folding line.
                return Some(last);
            }
            if matches!(line.first(), Some(b' ') | Some(b'\t')) {
                let folded: Vec<u8> = line
                    .iter()
                    .copied()
                    .skip_while(|&c| c == b' ' || c == b'\t')
                    .collect();
                last.push(b' ');
                last.extend_from_slice(&folded);
                return Some(last);
            }
        }
    }
    let value_start = match last.iter().position(|&c| c == b':') {
        Some(colon) => {
            if last[1.min(colon)..colon]
                .iter()
                .any(|&c| c == b' ' || c == b'\t')
            {
                info.error = Some("HTTP invalid response format (space in header name)!".into());
                return None;
            }
            let mut v = colon + 1;
            while v + 1 < last.len() && (last[v] == b' ' || last[v] == b'\t') {
                v += 1;
            }
            v
        }
        None => {
            info.error = Some("HTTP invalid response format (no colon in header line)!".into());
            return None;
        }
    };
    let value = last[value_start.min(last.len())..].to_vec();
    let starts = |p: &[u8]| last.len() >= p.len() && last[..p.len()].eq_ignore_ascii_case(p);
    let mut store = true;
    if starts(b"Location:") {
        match opt(http, "follow_location") {
            Some(v) => info.follow_location = v.is_truthy(),
            None => {
                if !((300..304).contains(&response_code)
                    || response_code == 307
                    || response_code == 308)
                {
                    info.follow_location = false;
                }
            }
        }
        // The value ends at the first NUL (C string).
        let v: Vec<u8> = value.iter().copied().take_while(|&c| c != 0).collect();
        if v.len() > HTTP_HEADER_MAX_LOCATION_SIZE {
            info.error = Some(format!(
                "HTTP Location header size is over the limit of {} bytes",
                HTTP_HEADER_MAX_LOCATION_SIZE
            ));
            return None;
        }
        info.location = Some(v);
    } else if starts(b"Content-Type:") {
        notify(
            vm,
            ctx,
            NOTIFY_MIME_TYPE_IS,
            SEVERITY_INFO,
            Some(&value),
            0,
            0,
            0,
        );
    } else if starts(b"Content-Length:") {
        if value.first().is_some_and(|c| c.is_ascii_digit())
            && value.iter().all(|c| c.is_ascii_digit())
        {
            let n = std::str::from_utf8(&value)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(u64::MAX)
                .min(i64::MAX as u64) as i64;
            info.file_size = n;
            notify(
                vm,
                ctx,
                NOTIFY_FILE_SIZE_IS,
                SEVERITY_INFO,
                Some(&last),
                0,
                0,
                n,
            );
        }
    } else if starts(b"Transfer-Encoding:")
        && value.len() >= 7
        && value[..7].eq_ignore_ascii_case(b"Chunked")
        && !headers_only
        && opt(http, "auto_decode").is_none_or(|v| v.is_truthy())
    {
        info.chunked = true;
        store = false;
    }
    if store {
        headers.push(Value::String(PhpString::from_vec(last)));
    }
    None
}

fn fail(vm: &mut Vm, func: &str, url: &str, msg: &str) {
    vm.emit_warning(&format!(
        "{}({}): Failed to open stream: {}",
        func, url, msg
    ));
}

/// php_stream_handle_proxy_authorization_header(): the
/// "Proxy-Authorization: ..." line of user headers `s`, if any.
fn proxy_authorization_header(s: &[u8]) -> Option<Vec<u8>> {
    const NAME: &[u8] = b"Proxy-Authorization:";
    let mut i = 0;
    loop {
        while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
            i += 1;
        }
        let start = i;
        let mut p = i;
        while p < s.len() && !matches!(s[p], 0 | b':' | b'\r' | b'\n') {
            p += 1;
        }
        if p < s.len() && s[p] == b':' {
            p += 1;
            let is_auth = s[start..p].eq_ignore_ascii_case(NAME);
            while p < s.len() && !matches!(s[p], 0 | b'\r' | b'\n') {
                p += 1;
            }
            if is_auth {
                return Some(s[start..p].to_vec());
            }
        }
        i = p;
        while i < s.len() && (s[i] == b'\r' || s[i] == b'\n') {
            i += 1;
        }
        if i >= s.len() || s[i] == 0 {
            return None;
        }
    }
}

/// strip_header(): remove a header (and its line) from the user headers.
fn strip_header(user_headers: &mut String, header: &str) {
    let lower = user_headers.to_ascii_lowercase();
    let mut start = 0;
    while let Some(p) = lower[start..].find(header) {
        let at = start + p;
        if at == 0 || lower.as_bytes()[at - 1] == b'\n' {
            let rest = &lower[at..];
            let end = match rest.find('\n') {
                Some(e) => at + e + 1,
                None => lower.len(),
            };
            user_headers.replace_range(at..end, "");
            return;
        }
        start = at + 1;
    }
}

/// php_stream_url_wrap_http(): open an http(s):// URL. The socket stream
/// is returned positioned at the body (`headers_only` =
/// STREAM_ONLY_GET_HEADERS); `$http_response_header` and the last response
/// headers are set.
pub(super) fn http_open_stream(
    vm: &mut Vm,
    func: &str,
    orig_url: &str,
    ctx: Option<&Value>,
    mode: &str,
    headers_only: bool,
) -> Option<(i64, PhpArray)> {
    clear_last_response_headers();
    let mut headers: Option<PhpArray> = None;
    let r = http_wrap_ex(vm, func, orig_url, ctx, mode, headers_only, &mut headers);
    xp_socket::flush_notifications(vm);
    if let Some(h) = &headers {
        LAST_HTTP_HEADERS.with(|l| *l.borrow_mut() = Some(h.clone()));
        set_response_header(vm, h.clone());
    }
    r.map(|fid| (fid, headers.unwrap_or_default()))
}

/// php_stream_context_get_uri_parser("http") + php_uri_parse_to_struct():
/// the URL must be valid for the context's "uri_parser_class" (false after
/// the ValueError for an unknown parser, or the "operation failed" warning).
fn uri_parser_accepts(vm: &mut Vm, func: &str, http: &PhpArray, path: &str) -> bool {
    let parser = match opt(http, "uri_parser_class").map(|v| v.deref()) {
        None | Some(Value::Null) => goro_core::uri::get_parser(None),
        Some(Value::String(s)) => goro_core::uri::get_parser(Some(s.as_bytes())),
        Some(_) => None,
    };
    let Some(parser) = parser else {
        let msg = format!(
            "{}(): Provided stream context has invalid value for the \"uri_parser_class\" option",
            func
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return false;
    };
    let ok = if parser == goro_core::uri::Parser::PhpParseUrl {
        let parsed = match vm.functions.get(b"parse_url".as_slice()).copied() {
            Some(f) => f(vm, &[Value::String(PhpString::from_bytes(path.as_bytes()))])
                .unwrap_or(Value::False),
            None => Value::False,
        };
        matches!(parsed, Value::Array(_))
    } else {
        goro_core::uri::parse_components(
            vm,
            parser,
            path.as_bytes(),
            goro_core::uri::ReadMode::Raw,
            true,
        )
        .is_some()
    };
    if !ok {
        vm.emit_warning(&format!(
            "{}({}): Failed to open stream: operation failed",
            func, path
        ));
    }
    ok
}

/// The context resource of a context argument (FG(default_context) when
/// none is given).
fn context_id(ctx: Option<&Value>) -> Option<i64> {
    match ctx.map(|c| c.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => {
            crate::misc::DEFAULT_STREAM_CTX.with(|d| d.get())
        }
        Some(v @ Value::Resource(_)) => resolve_context_id(&v),
        _ => None,
    }
}

/// php_url_decode() in place on a string whose length is kept: the decoded
/// bytes, the terminating NUL, then what is left of the original.
fn url_decode_keep_len(s: &str) -> Vec<u8> {
    let mut d = url_decode(s);
    if d.len() < s.len() {
        d.push(0);
        d.extend_from_slice(&s.as_bytes()[d.len()..]);
    }
    d
}

/// php_stream_url_wrap_http_ex(), redirects included: the open stream.
#[allow(clippy::too_many_arguments)]
fn http_wrap_ex(
    vm: &mut Vm,
    func: &str,
    orig_url: &str,
    ctx: Option<&Value>,
    mode: &str,
    headers_only: bool,
    response_header: &mut Option<PhpArray>,
) -> Option<i64> {
    let (http, _ssl) = context_options(ctx);
    let ctx_id = context_id(ctx);
    let mut path = orig_url.to_string();
    let mut redirect_max: i64 = 20; // PHP_URL_REDIRECT_MAX
    let mut header_init = true;
    let mut redirected = false;
    let mut keep_method = false;
    // The declared size of each response followed: the `out:` path of
    // every (recursive) call runs once the innermost one returns.
    let mut levels: Vec<i64> = Vec::new();
    let (fid, chunked) = loop {
        if redirect_max < 1 {
            fail(vm, func, orig_url, "Redirection limit reached, aborting");
            return None;
        }
        if !uri_parser_accepts(vm, func, &http, &path) {
            return None;
        }
        let Some(url) = parse_url(&path) else {
            fail(vm, func, orig_url, "HTTP request failed!");
            return None;
        };
        if mode.contains(['a', 'w', 'x', '+']) {
            fail(
                vm,
                func,
                orig_url,
                "HTTP wrapper does not support writeable connections",
            );
            return None;
        }
        let request_fulluri = opt(&http, "request_fulluri").is_some_and(|v| v.is_truthy());
        let port = url.port.unwrap_or(if url.https { 443 } else { 80 });
        let proxy = match opt(&http, "proxy").map(|v| v.deref()) {
            Some(Value::String(s)) if !s.as_bytes().is_empty() => Some(s.to_string_lossy()),
            _ => None,
        };
        let transport = match &proxy {
            Some(p) => p.clone(),
            None => format!(
                "{}://{}:{}",
                if url.https { "ssl" } else { "tcp" },
                url.host,
                port
            ),
        };
        if request_fulluri && (path.contains('\n') || path.contains('\r')) {
            fail(
                vm,
                func,
                orig_url,
                "HTTP wrapper full URI path does not allow CR or LF characters",
            );
            return None;
        }
        // PHP_TIMEOUT_ULL_MAX / 1000000.0
        const TIMEOUT_MAX: f64 = u64::MAX as f64 / 1000000.0;
        let timeout = match opt(&http, "timeout") {
            Some(v) => {
                let d = v.to_double();
                if d > TIMEOUT_MAX {
                    fail(
                        vm,
                        func,
                        orig_url,
                        &format!("timeout must be lower than {}", TIMEOUT_MAX as u64),
                    );
                    return None;
                }
                d
            }
            None => vm
                .constants
                .get(b"default_socket_timeout".as_slice())
                .map(|v| v.to_double().trunc())
                .unwrap_or(60.0),
        };
        let fid = match xp_socket::http_connect(vm, func, transport.as_bytes(), timeout, ctx_id) {
            Ok(f) => f,
            Err(text) => {
                xp_socket::flush_notifications(vm);
                fail(
                    vm,
                    func,
                    orig_url,
                    text.as_deref().unwrap_or("operation failed"),
                );
                return None;
            }
        };
        // https through a proxy: a CONNECT tunnel, then TLS to the server.
        if proxy.is_some() && url.https {
            let mut connect = format!("CONNECT {}:{} HTTP/1.0\r\n", url.host, port).into_bytes();
            let auth = match opt(&http, "header").map(|v| v.deref()) {
                Some(Value::Array(a)) => a.borrow().values().find_map(|v| match v.deref() {
                    Value::String(s) => proxy_authorization_header(s.as_bytes()),
                    _ => None,
                }),
                Some(Value::String(s)) => proxy_authorization_header(s.as_bytes()),
                _ => None,
            };
            if let Some(a) = auth {
                connect.extend_from_slice(&a);
                connect.extend_from_slice(b"\r\n");
            }
            connect.extend_from_slice(b"\r\n");
            let has_peer = ctx_id.is_some_and(|id| {
                STREAM_CONTEXTS.with(|c| {
                    c.borrow()
                        .get(&id)
                        .and_then(|c| c.options.get(b"ssl".as_slice()))
                        .is_some_and(|o| o.contains_key(b"peer_name".as_slice()))
                })
            });
            let peer = if has_peer {
                None
            } else {
                Some(url.host.as_bytes())
            };
            if !xp_socket::http_proxy_tunnel(vm, func, fid, &connect, peer) {
                xp_socket::http_close(fid);
                xp_socket::flush_notifications(vm);
                fail(
                    vm,
                    func,
                    orig_url,
                    "Cannot connect to HTTPS server through proxy",
                );
                return None;
            }
        }
        xp_socket::flush_notifications(vm);
        notify(vm, ctx, NOTIFY_CONNECT, SEVERITY_INFO, None, 0, 0, 0);
        if header_init {
            if let Some(v) = opt(&http, "max_redirects") {
                redirect_max = v.to_long();
            }
        }
        // Request line.
        let mut req = String::new();
        let mut custom_method = false;
        if let Some(Value::String(m)) = opt(&http, "method").map(|v| v.deref()) {
            if !m.as_bytes().is_empty()
                && (!redirected || keep_method || m.as_bytes() == b"GET" || m.as_bytes() == b"HEAD")
            {
                custom_method = true;
                req.push_str(&m.to_string_lossy());
                req.push(' ');
            }
        }
        if !custom_method {
            req.push_str("GET ");
        }
        if request_fulluri {
            req.push_str(&path);
        } else {
            req.push_str(&url.path);
        }
        match opt(&http, "protocol_version") {
            Some(v) => req.push_str(&format!(" HTTP/{:.1}\r\n", v.to_double())),
            None => req.push_str(" HTTP/1.1\r\n"),
        }
        // User headers.
        let mut user_headers: Option<String> = None;
        if let Some(h) = opt(&http, "header") {
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
        if !header_init && !keep_method {
            if let Some(uh) = user_headers.as_mut() {
                // strip POST headers on redirect
                strip_header(uh, "content-length:");
                strip_header(uh, "content-type:");
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
                let mut scratch = url_decode_keep_len(u);
                scratch.push(b':');
                if let Some(p) = &url.pass {
                    scratch.extend_from_slice(&url_decode_keep_len(p));
                }
                req.push_str(&format!("Authorization: Basic {}\r\n", base64(&scratch)));
                notify(vm, ctx, NOTIFY_AUTH_REQUIRED, SEVERITY_INFO, None, 0, 0, 0);
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
            if (url.https && port != 443 && port != 0) || (!url.https && port != 80 && port != 0) {
                req.push_str(&format!(":{}", port));
            }
            req.push_str("\r\n");
        }
        if !have_conn {
            req.push_str("Connection: close\r\n");
        }
        let ua = match opt(&http, "user_agent").map(|v| v.deref()) {
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
        let content: Option<Vec<u8>> = if header_init || keep_method {
            match opt(&http, "content").map(|v| v.deref()) {
                Some(Value::String(s)) if !s.as_bytes().is_empty() => Some(s.as_bytes().to_vec()),
                _ => None,
            }
        } else {
            None
        };
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
        let mut out = Vec::new();
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
        // send it
        xp_socket::sock_write(vm, func, fid, &out);
        xp_socket::flush_notifications(vm);
        let headers = response_header.get_or_insert_with(PhpArray::new);

        // Status line.
        let mut reqok = false;
        let status_line: Vec<u8>;
        let ignore_errors;
        let first = crate::misc::stream_get_line_raw(fid, 126);
        xp_socket::flush_notifications(vm);
        let response_code = match first {
            Some(mut line) => {
                let code_of = |l: &[u8]| -> i64 {
                    if l.len() > 9 {
                        let d: Vec<u8> = l[9..]
                            .iter()
                            .copied()
                            .skip_while(|c| matches!(c, b' ' | b'\t'))
                            .take_while(|c| c.is_ascii_digit())
                            .collect();
                        String::from_utf8_lossy(&d).parse().unwrap_or(0)
                    } else {
                        0
                    }
                };
                let mut code = code_of(&line);
                ignore_errors = opt(&http, "ignore_errors").is_some_and(|v| v.is_truthy());
                if headers_only || ignore_errors {
                    reqok = true;
                }
                if (100..200).contains(&code) && code != 101 {
                    // skip to the real status line
                    while let Some(l) = crate::misc::stream_get_line_raw(fid, 126) {
                        let is_status = l.len() >= 6 && l[..6].eq_ignore_ascii_case(b"HTTP/1");
                        line = l;
                        if is_status {
                            break;
                        }
                    }
                    xp_socket::flush_notifications(vm);
                    code = code_of(&line);
                }
                if (200..400).contains(&code) {
                    reqok = true;
                } else {
                    let ncode = if code == 403 {
                        NOTIFY_AUTH_RESULT
                    } else {
                        NOTIFY_FAILURE
                    };
                    notify(vm, ctx, ncode, SEVERITY_ERR, Some(&line), code, 0, 0);
                }
                status_line = line.clone();
                let mut t = line;
                if t.last() == Some(&b'\n') {
                    t.pop();
                    if t.last() == Some(&b'\r') {
                        t.pop();
                    }
                } else {
                    // read and discard the rest of the status line
                    let _ = crate::misc::stream_get_line_raw(fid, 0);
                    xp_socket::flush_notifications(vm);
                }
                headers.push(Value::String(PhpString::from_vec(t)));
                code
            }
            None => {
                xp_socket::http_close(fid);
                fail(vm, func, orig_url, "HTTP request failed!");
                return None;
            }
        };

        // Headers.
        let mut info = HeaderInfo {
            chunked: false,
            file_size: 0,
            error: None,
            follow_location: true,
            location: None,
        };
        let mut last: Option<Vec<u8>> = None;
        loop {
            let line = crate::misc::stream_get_line_raw(fid, 0);
            xp_socket::flush_notifications(vm);
            let Some(mut line) = line else {
                break;
            };
            let last_line = match line.first() {
                Some(b'\r') => {
                    if line.get(1) != Some(&b'\n') {
                        xp_socket::http_close(fid);
                        fail(
                            vm,
                            func,
                            orig_url,
                            "HTTP invalid header name (cannot start with CR character)!",
                        );
                        return None;
                    }
                    true
                }
                Some(b'\n') => true,
                _ => false,
            };
            if let Some(l) = last.take() {
                last = headers_parse(
                    vm,
                    ctx,
                    &http,
                    headers_only,
                    l,
                    Some(&mut line),
                    response_code,
                    headers,
                    &mut info,
                );
                if last.is_none() {
                    if let Some(e) = info.error.take() {
                        xp_socket::http_close(fid);
                        fail(vm, func, orig_url, &e);
                        return None;
                    }
                } else {
                    // a folded header continues
                    continue;
                }
            } else if !last_line {
                if matches!(line.first(), Some(b' ') | Some(b'\t')) {
                    xp_socket::http_close(fid);
                    fail(
                        vm,
                        func,
                        orig_url,
                        "HTTP invalid response format (folding header at the start)!",
                    );
                    return None;
                }
                header_trim(&mut line);
            }
            if last_line {
                break;
            }
            last = Some(line);
        }
        if let Some(l) = last.take() {
            headers_parse(
                vm,
                ctx,
                &http,
                headers_only,
                l,
                None,
                response_code,
                headers,
                &mut info,
            );
            // (an invalid last line is dropped: the stream still opens,
            // without the wrapper's error being displayed)
            info.error = None;
        }
        levels.push(info.file_size);

        if !reqok || (info.location.is_some() && info.follow_location) {
            if !info.follow_location || ((headers_only || ignore_errors) && redirect_max <= 1) {
                break (fid, info.chunked);
            }
            if let Some(loc) = &info.location {
                notify(
                    vm,
                    ctx,
                    NOTIFY_REDIRECTED,
                    SEVERITY_INFO,
                    Some(loc),
                    0,
                    0,
                    0,
                );
            }
            xp_socket::http_close(fid);
            let Some(loc) = info.location.take() else {
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
            };
            let loc = String::from_utf8_lossy(&loc).to_string();
            let l = loc.to_ascii_lowercase();
            let new_path = if loc.len() < 8
                || !(l.starts_with("http://")
                    || l.starts_with("https://")
                    || l.starts_with("ftp://")
                    || l.starts_with("ftps://"))
            {
                let loc_path = if !loc.starts_with('/') {
                    if loc.len() > 1 {
                        let base = match url.path.split('?').next().unwrap_or("").rfind('/') {
                            Some(i) => url.path[..=i].to_string(),
                            None => "/".to_string(),
                        };
                        // (sic: "/dir/" + "/" + location, as php-src does)
                        if base == "/" {
                            format!("/{}", loc)
                        } else {
                            format!("{}/{}", base, loc)
                        }
                    } else {
                        format!("/{}", loc)
                    }
                } else {
                    loc.clone()
                };
                let scheme = if url.https { "https" } else { "http" };
                if (url.https && port != 443) || (!url.https && port != 80) {
                    format!("{}://{}:{}{}", scheme, url.host, port, loc_path)
                } else {
                    format!("{}://{}{}", scheme, url.host, loc_path)
                }
            } else {
                loc
            };
            // check for control characters in login, password & path
            match parse_url(&new_path) {
                Some(nu) => {
                    let bad = |s: &str| url_decode(s).iter().any(|c| c.is_ascii_control());
                    if nu.user.as_deref().is_some_and(bad)
                        || nu.pass.as_deref().is_some_and(bad)
                        || bad(&nu.path)
                    {
                        fail(
                            vm,
                            func,
                            orig_url,
                            &format!("Invalid redirect URL! {}", new_path),
                        );
                        return None;
                    }
                }
                None => {
                    fail(
                        vm,
                        func,
                        orig_url,
                        &format!("Invalid redirect URL! {}", new_path),
                    );
                    return None;
                }
            }
            keep_method = response_code == 307 || response_code == 308;
            redirected = true;
            header_init = false;
            redirect_max -= 1;
            path = new_path;
            continue;
        }
        break (fid, info.chunked);
    };
    // The `out:` path of each call, the innermost first: the progress is
    // started over with that response's size, and what the stream already
    // holds of the body counts.
    for (i, size) in levels.iter().enumerate().rev() {
        xp_socket::notify_progress_init(ctx_id, *size);
        if i + 1 == levels.len() && chunked {
            crate::misc::attach_read_filter(vm, fid, b"dechunk");
        }
        // as far as streams are concerned, we are now at the start of
        // the stream
        crate::misc::stream_reset_position(fid);
        let buffered = crate::misc::stream_buffered_len(fid);
        if buffered > 0 {
            xp_socket::notify_progress_increment(ctx_id, buffered as i64);
        }
    }
    xp_socket::flush_notifications(vm);
    Some(fid)
}

/// get_headers()
pub(super) fn get_headers(vm: &mut Vm, args: &[Value]) -> Value {
    let url = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let assoc = args.get(1).is_some_and(|v| v.is_truthy());
    let Some((fid, headers)) = http_open_stream(vm, "get_headers", &url, args.get(2), "r", true)
    else {
        return Value::False;
    };
    xp_socket::http_close(fid);
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
    vm.set_http_response_header(Value::Array(Rc::new(RefCell::new(headers))));
}
