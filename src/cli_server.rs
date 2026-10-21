//! The built-in web server (`php -S`), ported from sapi/cli/php_cli_server.c.
//!
//! A single-threaded poll() loop accepts clients, feeds their requests to
//! the HTTP parser and dispatches complete requests: PHP scripts (and the
//! router script) run in a fresh engine, static files are sent as they are.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::engine::{self, IniSettings};
use crate::getopt::GetOpt;
use crate::http_parser::{HttpParser, Method, ParserCallbacks, ParserInfo};

const LOG_PROCESS: i32 = 1;
const LOG_ERROR: i32 = 2;
const LOG_MESSAGE: i32 = 3;

thread_local! {
    static LOG_LEVEL: std::cell::Cell<i32> = const { std::cell::Cell::new(3) };
    static WORKERS_MAX: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    /// The request headers of the request being executed, in their original
    /// case (apache_request_headers()).
    static REQUEST_HEADERS: RefCell<Vec<(Vec<u8>, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

const CSS: &str = "<style>\n\
body { background-color: #fcfcfc; color: #333333; margin: 0; padding:0; }\n\
h1 { font-size: 1.5em; font-weight: normal; background-color: #9999cc; min-height:2em; line-height:2em; border-bottom: 1px inset black; margin: 0; }\n\
h1, p { padding-left: 10px; }\n\
code.url { background-color: #eeeeee; font-family:monospace; padding:0 2px;}\n\
</style>\n";

/// main/http_status_codes.h
fn status_string(code: i32) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Request Entity Too Large",
        414 => "Request-URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Requested Range Not Satisfiable",
        417 => "Expectation Failed",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        511 => "Network Authentication Required",
        _ => "Unknown Status Code",
    }
}

fn template_string(code: i32) -> Option<&'static str> {
    match code {
        400 => Some(
            "<h1>%s</h1><p>Your browser sent a request that this server could not understand.</p>",
        ),
        404 => Some(
            "<h1>%s</h1><p>The requested resource <code class=\"url\">%s</code> was not found on this server.</p>",
        ),
        405 => Some("<h1>%s</h1><p>Requested method not allowed.</p>"),
        500 => Some("<h1>%s</h1><p>The server is temporarily unavailable.</p>"),
        501 => Some("<h1>%s</h1><p>Request method not supported.</p>"),
        _ => None,
    }
}

fn mime_type(ext: &[u8]) -> Option<&'static str> {
    let lower = String::from_utf8_lossy(ext).to_ascii_lowercase();
    crate::mime_types::MIME_TYPES
        .binary_search_by(|(e, _)| (*e).cmp(lower.as_str()))
        .ok()
        .map(|i| crate::mime_types::MIME_TYPES[i].1)
}

/// The local time like asctime() without the trailing newline.
fn system_time() -> String {
    unsafe {
        let mut t: libc::time_t = 0;
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return "unknown time, can't be fetched".to_string();
        }
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "{} {} {:2} {:02}:{:02}:{:02} {}",
            DAYS[tm.tm_wday.clamp(0, 6) as usize],
            MONTHS[tm.tm_mon.clamp(0, 11) as usize],
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            1900 + tm.tm_year
        )
    }
}

/// sapi_cli_server_log_write()
fn log_write(ty: i32, msg: &str) {
    if LOG_LEVEL.with(|l| l.get()) < ty {
        return;
    }
    let line = if WORKERS_MAX.with(|w| w.get()) > 1 {
        format!("[{}] [{}] {}\n", std::process::id(), system_time(), msg)
    } else {
        format!("[{}] {}\n", system_time(), msg)
    };
    let mut e = std::io::stderr().lock();
    let _ = e.write_all(line.as_bytes());
    let _ = e.flush();
}

fn strerror(errno: i32) -> String {
    unsafe {
        let p = libc::strerror(errno);
        if p.is_null() {
            return "Unknown error".to_string();
        }
        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

struct Server {
    host: String,
    port: u16,
    document_root: Vec<u8>,
    router: Option<Vec<u8>>,
    ini: IniSettings,
}

#[derive(Default)]
struct Request {
    method: Option<Method>,
    protocol_version: u16,
    request_uri: Option<Vec<u8>>,
    vpath: Option<Vec<u8>>,
    path_translated: Option<Vec<u8>>,
    path_info: Option<Vec<u8>>,
    query_string: Option<Vec<u8>>,
    /// Lower-cased names, in insertion order.
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    headers_original_case: Vec<(Vec<u8>, Vec<u8>)>,
    content: Option<Vec<u8>>,
    ext: Vec<u8>,
    size: u64,
}

impl Request {
    fn header(&self, lc: &[u8]) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(k, _)| k == lc)
            .map(|(_, v)| v.as_slice())
    }
    fn method(&self) -> Method {
        self.method.unwrap_or(Method::Get)
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum HeaderElement {
    None,
    Field,
    Value,
}

/// The parse state of one request (the php_cli_server_client fields used
/// by the parser callbacks).
struct ReqParse<'a> {
    server: &'a Server,
    request: &'a mut Request,
    request_read: &'a mut bool,
    current_header_name: Option<Vec<u8>>,
    current_header_value: Option<Vec<u8>>,
    last_header_element: HeaderElement,
}

fn upsert(list: &mut Vec<(Vec<u8>, Vec<u8>)>, key: Vec<u8>, val: Vec<u8>) {
    if let Some(e) = list.iter_mut().find(|(k, _)| *k == key) {
        e.1 = val;
    } else {
        list.push((key, val));
    }
}

impl ReqParse<'_> {
    fn save_header(&mut self) {
        let name = self.current_header_name.take().unwrap_or_default();
        let value = self.current_header_value.take().unwrap_or_default();
        let lc = name.to_ascii_lowercase();
        let with_comma = lc != b"set-cookie";
        let entry = self.request.header(&lc).map(|v| v.to_vec());
        let newval = match entry {
            Some(cur) if with_comma => {
                let mut n = cur;
                n.extend_from_slice(b", ");
                n.extend_from_slice(&value);
                n
            }
            _ => value,
        };
        upsert(&mut self.request.headers, lc, newval.clone());
        upsert(&mut self.request.headers_original_case, name, newval);
    }
}

impl ParserCallbacks for ReqParse<'_> {
    fn on_path(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
        if self.request.vpath.is_some() {
            return true;
        }
        self.request.vpath = Some(normalize_vpath(at));
        false
    }
    fn on_query_string(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
        self.request
            .query_string
            .get_or_insert_with(Vec::new)
            .extend_from_slice(at);
        false
    }
    fn on_url(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        match &mut self.request.request_uri {
            None => {
                self.request.method = Some(p.method);
                self.request.request_uri = Some(at.to_vec());
            }
            Some(u) => u.extend_from_slice(at),
        }
        false
    }
    fn on_header_field(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
        match self.last_header_element {
            HeaderElement::Value => {
                self.save_header();
                self.current_header_name = Some(at.to_vec());
            }
            HeaderElement::None => self.current_header_name = Some(at.to_vec()),
            HeaderElement::Field => self
                .current_header_name
                .get_or_insert_with(Vec::new)
                .extend_from_slice(at),
        }
        self.last_header_element = HeaderElement::Field;
        false
    }
    fn on_header_value(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
        match self.last_header_element {
            HeaderElement::Field => self.current_header_value = Some(at.to_vec()),
            HeaderElement::Value => self
                .current_header_value
                .get_or_insert_with(Vec::new)
                .extend_from_slice(at),
            HeaderElement::None => {}
        }
        self.last_header_element = HeaderElement::Value;
        false
    }
    fn on_headers_complete(&mut self, _p: &ParserInfo) -> bool {
        match self.last_header_element {
            HeaderElement::None => {}
            HeaderElement::Field => {
                self.current_header_value = Some(Vec::new());
                self.save_header();
            }
            HeaderElement::Value => self.save_header(),
        }
        self.last_header_element = HeaderElement::None;
        false
    }
    fn on_body(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
        self.request
            .content
            .get_or_insert_with(Vec::new)
            .extend_from_slice(at);
        false
    }
    fn on_message_complete(&mut self, p: &ParserInfo) -> bool {
        self.request.protocol_version = p.http_major * 100 + p.http_minor;
        translate_vpath(self.request, &self.server.document_root);
        if let Some(vpath) = &self.request.vpath {
            self.request.ext = match vpath.iter().rposition(|&c| c == b'.') {
                Some(dot) => vpath[dot + 1..].to_vec(),
                None => Vec::new(),
            };
        }
        *self.request_read = true;
        false
    }
}

/// php_raw_url_decode()
fn raw_url_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' && i + 2 < s.len() {
            let h = |c: u8| (c as char).to_digit(16);
            if let (Some(a), Some(b)) = (h(s[i + 1]), h(s[i + 2])) {
                out.push((a * 16 + b) as u8);
                i += 3;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// normalize_vpath(): url-decode, collapse slashes and resolve . and ..
fn normalize_vpath(vpath: &[u8]) -> Vec<u8> {
    let mut d = raw_url_decode(vpath);
    // The decoded path is cut at a NUL byte only by later C string uses;
    // keep the bytes (dispatch checks for embedded NULs).
    let mut end = d.len();
    let mut p = 0usize;
    if p < end && d[p] == b'/' {
        let mut n = p;
        while n < end && d[n] == b'/' {
            n += 1;
        }
        p += 1;
        d.copy_within(n..end, p);
        end -= n - p;
    }
    while p < end {
        let mut n = p;
        while n < end && d[n] != b'/' {
            n += 1;
        }
        if n - p == 2 && d[p] == b'.' && d[p + 1] == b'.' {
            if p > 0 {
                p -= 1;
                loop {
                    if p == 0 {
                        if d[p] == b'/' {
                            p += 1;
                        }
                        break;
                    }
                    p -= 1;
                    if d[p] == b'/' {
                        p += 1;
                        break;
                    }
                }
            }
            while n < end && d[n] == b'/' {
                n += 1;
            }
            d.copy_within(n..end, p);
            end -= n - p;
        } else if n - p == 1 && d[p] == b'.' {
            while n < end && d[n] == b'/' {
                n += 1;
            }
            d.copy_within(n..end, p);
            end -= n - p;
        } else if n < end {
            let mut nn = n;
            while nn < end && d[nn] == b'/' {
                nn += 1;
            }
            p = n + 1;
            d.copy_within(nn..end, p);
            end -= nn - p;
        } else {
            p = n;
        }
    }
    d.truncate(end);
    d
}

fn stat(path: &[u8]) -> Option<std::fs::Metadata> {
    // Paths with NUL bytes cannot be stat()ed.
    let cut = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    std::fs::metadata(std::ffi::OsStr::from_bytes(&path[..cut])).ok()
}

/// php_cli_server_request_translate_vpath()
fn translate_vpath(request: &mut Request, document_root: &[u8]) {
    const INDEX_FILES: [&[u8]; 2] = [b"index.php", b"index.html"];
    let vpath = request.vpath.clone().unwrap_or_default();
    let mut buf: Vec<u8> = document_root.to_vec();
    let vpath_start = buf.len();
    if !vpath.is_empty() {
        if vpath[0] != b'/' {
            buf.push(b'/');
        }
        buf.extend_from_slice(&vpath);
    }
    let p = buf.len();
    let mut q = p;
    let mut prev_path: Option<Vec<u8>> = None;
    let mut size = 0u64;
    let mut restore_at: Option<usize> = None;
    while q > 0 {
        let cur = &buf[..q];
        if let Some(sb) = stat(cur) {
            if sb.is_dir() {
                if buf[q - 1] != b'/' {
                    buf.truncate(q);
                    buf.push(b'/');
                    q += 1;
                } else {
                    buf.truncate(q);
                }
                let mut found = false;
                for f in INDEX_FILES {
                    buf.truncate(q);
                    buf.extend_from_slice(f);
                    if let Some(isb) = stat(&buf) {
                        if isb.is_file() {
                            q += f.len();
                            size = isb.len();
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    return;
                }
            } else {
                size = sb.len();
            }
            break;
        }
        if prev_path.is_some() {
            if let Some(r) = restore_at.take() {
                buf[r] = b'/';
            }
        }
        while q > 0 {
            q -= 1;
            if buf[q] == b'/' {
                break;
            }
        }
        prev_path = Some(buf[q..p].to_vec());
        restore_at = Some(q);
        // the C code writes a NUL at q; stat() above only looks at buf[..q]
    }
    if let Some(pp) = prev_path {
        request.path_info = Some(pp);
    }
    let q = q.min(buf.len());
    request.vpath = Some(buf[vpath_start.min(q)..q].to_vec());
    request.path_translated = Some(buf[..q].to_vec());
    request.size = size;
}

struct Client {
    stream: TcpStream,
    addr_str: String,
    parser: HttpParser,
    request_read: bool,
    current_header_name: Option<Vec<u8>>,
    current_header_value: Option<Vec<u8>>,
    last_header_element: HeaderElement,
    request: Request,
    /// Pending response bytes, and a file whose contents follow them.
    out: Vec<u8>,
    out_pos: usize,
    file: Option<std::fs::File>,
    sending: bool,
}

impl Client {
    fn new(stream: TcpStream, addr: SocketAddr) -> Self {
        let addr_str = match addr {
            SocketAddr::V4(a) => format!("{}:{}", a.ip(), a.port()),
            SocketAddr::V6(a) => format!("[{}]:{}", a.ip(), a.port()),
        };
        Client {
            stream,
            addr_str,
            parser: HttpParser::new_request(),
            request_read: false,
            current_header_name: None,
            current_header_value: None,
            last_header_element: HeaderElement::None,
            request: Request::default(),
            out: Vec::new(),
            out_pos: 0,
            file: None,
            sending: false,
        }
    }

    /// php_cli_server_client_read_request(): -1 error, 0 again, 1 done.
    fn read_request(&mut self, server: &Server) -> Result<bool, String> {
        if self.request_read {
            return Ok(true);
        }
        let mut buf = [0u8; 16383];
        let n = match self.stream.read(&mut buf) {
            Ok(0) => return Err("Unexpected EOF".to_string()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(false),
            Err(e) => return Err(strerror(e.raw_os_error().unwrap_or(0))),
        };
        let mut cb = ReqParse {
            server,
            request: &mut self.request,
            request_read: &mut self.request_read,
            current_header_name: self.current_header_name.take(),
            current_header_value: self.current_header_value.take(),
            last_header_element: self.last_header_element,
        };
        let consumed = self.parser.execute(&mut cb, &buf[..n]);
        self.current_header_name = cb.current_header_name.take();
        self.current_header_value = cb.current_header_value.take();
        self.last_header_element = cb.last_header_element;
        if consumed != n {
            if buf[0] & 0x80 != 0 || buf[0] == 0x16 {
                return Err("Unsupported SSL request".to_string());
            }
            return Err("Malformed HTTP request".to_string());
        }
        Ok(self.request_read)
    }

    /// php_cli_server_log_response()
    fn log_response(&self, status: i32, message: Option<&str>, last_error: Option<&LastError>) {
        let mut line = format!(
            "{} [{}]: {} {}",
            self.addr_str,
            status,
            self.request.method().as_str(),
            String::from_utf8_lossy(self.request.request_uri.as_deref().unwrap_or_default())
        );
        if let Some(m) = message {
            line.push_str(" - ");
            line.push_str(m);
        }
        if let Some(e) = last_error {
            if e.fatal {
                line.push_str(&format!(
                    " - {} in {} on line {}",
                    e.message, e.file, e.line
                ));
            }
        }
        log_write(LOG_MESSAGE, &line);
    }

    /// Write everything pending (blocking), then the file contents.
    fn send_all(&mut self) {
        let _ = self.stream.set_nonblocking(false);
        let _ = self
            .stream
            .set_write_timeout(Some(std::time::Duration::from_secs(10)));
        if self.out_pos < self.out.len() {
            let _ = self.stream.write_all(&self.out[self.out_pos..]);
        }
        self.out.clear();
        self.out_pos = 0;
        if let Some(mut f) = self.file.take() {
            let mut chunk = vec![0u8; 131072];
            loop {
                match f.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if self.stream.write_all(&chunk[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
        }
        let _ = self.stream.flush();
    }
}

struct LastError {
    fatal: bool,
    message: String,
    file: String,
    line: u32,
}

/// The status line and the "essential headers" (Host, Date, Connection).
fn append_status_line(buf: &mut Vec<u8>, protocol_version: u16, code: i32) {
    let code = if code == 0 { 200 } else { code };
    buf.extend_from_slice(
        format!(
            "HTTP/{}.{} {} {}\r\n",
            protocol_version / 100,
            protocol_version % 100,
            code,
            status_string(code)
        )
        .as_bytes(),
    );
}

fn http_date_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02}",
        wd,
        d,
        MONTHS[(m - 1) as usize],
        y,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn append_essential_headers(buf: &mut Vec<u8>, request: &Request, append_date: bool) {
    if let Some(host) = request.header(b"host") {
        buf.extend_from_slice(b"Host: ");
        buf.extend_from_slice(host);
        buf.extend_from_slice(b"\r\n");
    }
    if append_date {
        buf.extend_from_slice(format!("Date: {} GMT\r\n", http_date_now()).as_bytes());
    }
    buf.extend_from_slice(b"Connection: close\r\n");
}

/// htmlspecialchars(ENT_QUOTES, double_encode = false) of the request URI.
fn escape_html(s: &[u8]) -> String {
    let text = String::from_utf8_lossy(s);
    let mut out = String::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'&' => {
                // do not double encode an existing entity
                let rest = &text[i + 1..];
                let ent_len = rest.find(';').filter(|&e| {
                    e > 0
                        && e <= 32
                        && (rest[..e].chars().all(|c| c.is_ascii_alphanumeric())
                            || (rest.starts_with('#')
                                && rest[1..e].chars().all(|c| c.is_ascii_alphanumeric())))
                });
                if ent_len.is_some() {
                    out.push('&');
                } else {
                    out.push_str("&amp;");
                }
            }
            b'"' => out.push_str("&quot;"),
            b'\'' => out.push_str("&#039;"),
            b'<' => out.push_str("&lt;"),
            b'>' => out.push_str("&gt;"),
            _ => {
                let ch = text[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }
        }
        i += 1;
    }
    out
}

/// php_cli_server_send_error_page()
fn send_error_page(client: &mut Client, status: i32) {
    let errstr = strerror(last_errno());
    let status_string = status_string(status);
    let template = template_string(status).unwrap_or("<h1>%s</h1>");
    let mut body = String::new();
    if client.request.method() != Method::Head {
        let uri = escape_html(client.request.request_uri.as_deref().unwrap_or_default());
        body.push_str(&format!(
            "<!doctype html><html><head><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{} {}</title>",
            status, status_string
        ));
        body.push_str(CSS);
        body.push_str("</head><body>");
        let mut content = template.replacen("%s", status_string, 1);
        content = content.replacen("%s", &uri, 1);
        body.push_str(&content);
        body.push_str("</body></html>");
    }
    let mut head = Vec::new();
    append_status_line(&mut head, client.request.protocol_version, status);
    append_essential_headers(&mut head, &client.request, true);
    head.extend_from_slice(format!("X-Powered-By: PHP/{}\r\n", crate::PHP_VERSION).as_bytes());
    head.extend_from_slice(b"Content-Type: text/html; charset=UTF-8\r\n");
    head.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    if status == 405 {
        head.extend_from_slice(b"Allow: GET, HEAD, POST\r\n");
    }
    head.extend_from_slice(b"\r\n");
    head.extend_from_slice(body.as_bytes());
    client.out = head;
    client.out_pos = 0;
    client.sending = true;
    client.log_response(status, Some(&errstr), None);
}

/// php_cli_server_begin_send_static()
fn begin_send_static(client: &mut Client) {
    let m = client.request.method();
    if m == Method::Delete || m == Method::Put || m == Method::Patch {
        send_error_page(client, 405);
        return;
    }
    if let Some(pt) = &client.request.path_translated {
        if pt.contains(&0) {
            send_error_page(client, 400);
            return;
        }
    }
    let file = client.request.path_translated.as_ref().and_then(|pt| {
        let f = std::fs::File::open(std::ffi::OsStr::from_bytes(pt));
        if let Err(e) = &f {
            unsafe {
                *libc::__errno_location() = e.raw_os_error().unwrap_or(0);
            }
        }
        f.ok()
    });
    let Some(file) = file else {
        send_error_page(client, 404);
        return;
    };
    // Opening a directory succeeds; reading it fails like in C.
    let mut head = Vec::new();
    append_status_line(&mut head, client.request.protocol_version, 200);
    append_essential_headers(&mut head, &client.request, true);
    if let Some(mt) = mime_type(&client.request.ext) {
        head.extend_from_slice(b"Content-Type: ");
        head.extend_from_slice(mt.as_bytes());
        if mt.starts_with("text/") {
            head.extend_from_slice(b"; charset=UTF-8");
        }
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(format!("Content-Length: {}\r\n", client.request.size).as_bytes());
    head.extend_from_slice(b"\r\n");
    client.out = head;
    client.out_pos = 0;
    if m != Method::Head {
        client.file = Some(file);
    }
    client.sending = true;
    client.log_response(200, None, None);
}

/// php_handle_auth_data(): PHP_AUTH_USER / PHP_AUTH_PW / PHP_AUTH_DIGEST.
fn auth_vars(auth: &[u8], out: &mut Vec<(Vec<u8>, Vec<u8>)>) {
    if auth.len() >= 6 && auth[..6].eq_ignore_ascii_case(b"Basic ") {
        if let Some(user) = base64_decode(&auth[6..]) {
            let user = match user.iter().position(|&c| c == 0) {
                Some(z) => user[..z].to_vec(),
                None => user,
            };
            if let Some(colon) = user.iter().position(|&c| c == b':') {
                out.push((b"PHP_AUTH_USER".to_vec(), user[..colon].to_vec()));
                let pass = &user[colon + 1..];
                if !pass.is_empty() {
                    out.push((b"PHP_AUTH_PW".to_vec(), pass.to_vec()));
                }
                return;
            }
        }
    }
    if auth.len() >= 7 && auth[..7].eq_ignore_ascii_case(b"Digest ") {
        out.push((b"PHP_AUTH_DIGEST".to_vec(), auth[7..].to_vec()));
    }
}

/// php_base64_decode() (non-strict: skips invalid characters).
fn base64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in s {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// php_register_variable_safe(): the variable name as PHP registers it
/// ('.' and ' ' become '_'; names with '[' are not plain keys).
fn mangle_var_name(name: &[u8]) -> Option<Vec<u8>> {
    let mut start = 0;
    while start < name.len() && name[start] == b' ' {
        start += 1;
    }
    let name = &name[start..];
    if name.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(name.len());
    for &c in name {
        match c {
            b' ' | b'.' => out.push(b'_'),
            b'[' => break,
            _ => out.push(c),
        }
    }
    Some(out)
}

/// sapi_cli_server_register_variables()
fn server_variables(server: &Server, client: &Client) -> Vec<(Vec<u8>, Vec<u8>)> {
    let req = &client.request;
    let mut v: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut add = |k: &[u8], val: &[u8]| {
        if let Some(k) = mangle_var_name(k) {
            if let Some(e) = v.iter_mut().find(|(a, _)| *a == k) {
                e.1 = val.to_vec();
            } else {
                v.push((k, val.to_vec()));
            }
        }
    };
    add(b"DOCUMENT_ROOT", &server.document_root);
    let addr = client.addr_str.as_bytes();
    if let Some(colon) = addr.iter().rposition(|&c| c == b':') {
        let mut a = &addr[..colon];
        if a.first() == Some(&b'[') {
            a = &a[1..];
        }
        if a.last() == Some(&b']') {
            a = &a[..a.len() - 1];
        }
        let port = &addr[colon + 1..];
        let port = &port[..port.len().min(7)];
        add(b"REMOTE_ADDR", a);
        add(b"REMOTE_PORT", port);
    } else {
        add(b"REMOTE_ADDR", addr);
    }
    add(
        b"SERVER_SOFTWARE",
        format!("PHP/{} (Development Server)", crate::PHP_VERSION).as_bytes(),
    );
    add(
        b"SERVER_PROTOCOL",
        format!(
            "HTTP/{}.{}",
            req.protocol_version / 100,
            req.protocol_version % 100
        )
        .as_bytes(),
    );
    add(b"SERVER_NAME", server.host.as_bytes());
    add(b"SERVER_PORT", server.port.to_string().as_bytes());
    add(
        b"REQUEST_URI",
        req.request_uri.as_deref().unwrap_or_default(),
    );
    add(b"REQUEST_METHOD", req.method().as_str().as_bytes());
    let vpath = req.vpath.clone().unwrap_or_default();
    add(b"SCRIPT_NAME", &vpath);
    if let Some(pt) = &req.path_translated {
        add(b"SCRIPT_FILENAME", cstr(pt));
    } else if let Some(r) = &server.router {
        add(b"SCRIPT_FILENAME", r);
    }
    if let Some(pi) = &req.path_info {
        add(b"PATH_INFO", cstr(pi));
    }
    match &req.path_info {
        Some(pi) if !pi.is_empty() => {
            let mut s = cstr(&vpath).to_vec();
            s.extend_from_slice(cstr(pi));
            add(b"PHP_SELF", &s);
        }
        _ => add(b"PHP_SELF", cstr(&vpath)),
    }
    if let Some(qs) = &req.query_string {
        add(b"QUERY_STRING", cstr(qs));
    }
    for (k, val) in &req.headers {
        let key: Vec<u8> = k
            .iter()
            .map(|&c| {
                if c == b'-' {
                    b'_'
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        if key == b"CONTENT_TYPE" || key == b"CONTENT_LENGTH" {
            add(&key, cstr(val));
        }
        let mut real = b"HTTP_".to_vec();
        real.extend_from_slice(&key);
        add(&real, cstr(val));
    }
    if let Some(auth) = req.header(b"authorization") {
        let mut a = Vec::new();
        auth_vars(cstr(auth), &mut a);
        v.extend(a);
    }
    v
}

/// The bytes up to the first NUL (C string view).
fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(z) => &b[..z],
        None => b,
    }
}

/// apache_request_headers() / getallheaders()
fn apache_request_headers(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    REQUEST_HEADERS.with(|h| {
        for (k, v) in h.borrow().iter() {
            arr.set(
                ArrayKey::String(PhpString::from_bytes(k)),
                Value::String(PhpString::from_bytes(v)),
            );
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// The response being built from the script's output (the SAPI headers
/// are sent when the request ends, since output is collected).
struct ScriptResponse {
    head: Vec<u8>,
    body: Vec<u8>,
}

/// sapi_cli_server_send_headers() (with sapi_send_headers()'s default
/// content type).
fn build_script_headers(client: &Client, sapi_headers: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let status_line = goro_ext_standard::misc::take_status_line();
    let code = goro_ext_standard::misc::take_response_code() as i32;
    let mut buf = Vec::new();
    match status_line {
        Some(l) => {
            buf.extend_from_slice(&l);
            buf.extend_from_slice(b"\r\n");
        }
        None => append_status_line(&mut buf, client.request.protocol_version, code),
    }
    let has_date = sapi_headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case(b"Date") && !v.is_empty());
    append_essential_headers(&mut buf, &client.request, !has_date);
    let mut has_ct = false;
    for (k, v) in sapi_headers {
        if k.eq_ignore_ascii_case(b"Content-Type") {
            has_ct = true;
            // sapi_header_op(): "Content-type: " + mimetype with the default charset
            buf.extend_from_slice(b"Content-type: ");
            buf.extend_from_slice(&apply_default_charset(v));
        } else {
            buf.extend_from_slice(k);
            buf.extend_from_slice(b": ");
            buf.extend_from_slice(v);
        }
        buf.extend_from_slice(b"\r\n");
    }
    if !has_ct && code != 304 {
        buf.extend_from_slice(b"Content-type: text/html; charset=UTF-8\r\n");
    }
    buf.extend_from_slice(b"\r\n");
    buf
}

/// sapi_apply_default_charset()
fn apply_default_charset(mimetype: &[u8]) -> Vec<u8> {
    let mut m = mimetype.to_vec();
    if m.starts_with(b"text/") && !m.windows(8).any(|w| w == b"charset=") {
        m.extend_from_slice(b";charset=UTF-8");
    }
    m
}

/// Run the router or a script for the client's request. Returns the
/// router's decision (true = declined) and the response.
struct PhpRun {
    vm: Vm,
}

impl PhpRun {
    #[allow(clippy::field_reassign_with_default)] // fields filled in step by step
    fn startup(server: &Server, client: &Client) -> PhpRun {
        let mut vm = engine::new_vm(&server.ini, "cli-server");
        vm.register_function(b"apache_request_headers", apache_request_headers);
        vm.register_function(b"getallheaders", apache_request_headers);
        REQUEST_HEADERS.with(|h| *h.borrow_mut() = client.request.headers_original_case.clone());
        let _ = goro_ext_standard::misc::take_sent_headers();
        let _ = goro_ext_standard::misc::take_status_line();
        let _ = goro_ext_standard::misc::take_response_code();
        let req = &client.request;
        let mut info = goro_ext_standard::request::RequestInfo::default();
        info.cgi = true;
        info.request_method = Some(req.method().as_str().as_bytes().to_vec());
        info.query_string = req.query_string.clone().unwrap_or_default();
        info.cookie_data = req.header(b"cookie").map(|c| c.to_vec());
        info.content_type = req.header(b"content-type").map(|c| c.to_vec());
        info.content_length = req.content.as_ref().map(|c| c.len()).unwrap_or(0) as i64;
        info.body = req.content.clone().unwrap_or_default();
        info.script_filename = req
            .path_translated
            .clone()
            .or_else(|| server.router.clone())
            .unwrap_or_default();
        info.env = std::env::vars_os()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect();
        info.server_vars = Some(server_variables(server, client));
        goro_ext_standard::misc::set_translated_path(
            req.path_translated
                .as_ref()
                .map(|p| String::from_utf8_lossy(cstr(p)).into_owned()),
        );
        engine::request_startup(&mut vm, &info, &server.ini);
        PhpRun { vm }
    }

    /// Compile and run `path` as a main script piece.
    fn run_file(&mut self, path: &[u8]) -> Result<Value, VmError> {
        let src = match std::fs::read(std::ffi::OsStr::from_bytes(cstr(path))) {
            Ok(s) => s,
            Err(_) => {
                let p = String::from_utf8_lossy(path).into_owned();
                self.vm.display_error(
                    "Warning",
                    "Unknown: Failed to open stream: No such file or directory",
                    "Unknown",
                    0,
                );
                let _ = p;
                return Ok(Value::Null);
            }
        };
        let name = std::fs::canonicalize(std::ffi::OsStr::from_bytes(cstr(path)))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(path).into_owned());
        self.vm.current_file = name.clone();
        match engine::compile(&src, &name, true) {
            Ok(mut c) => {
                engine::load(&mut self.vm, &mut c);
                self.vm.execute_chunk(&c.op_array)
            }
            Err(e) => {
                engine::report_compile_error(&mut self.vm, &e, &name);
                self.vm.fatal_emitted = true;
                self.vm.last_error = Some((4, e.message.clone(), name, e.line));
                Err(VmError {
                    message: e.message,
                    line: e.line,
                })
            }
        }
    }

    fn last_error(&self) -> Option<LastError> {
        self.vm
            .last_error
            .as_ref()
            .map(|(ty, msg, file, line)| LastError {
                // E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR | E_RECOVERABLE_ERROR | E_PARSE
                fatal: ty & (1 | 16 | 64 | 256 | 4096 | 4) != 0,
                message: msg.clone(),
                file: file.clone(),
                line: *line,
            })
    }

    /// php_request_shutdown(): shutdown functions, destructors, output.
    fn shutdown(mut self, result: Result<Value, VmError>, client: &Client) -> ScriptResponse {
        let headers_sent_before_error = self.vm.headers_sent;
        let r = self.vm.finish_request(result);
        let failed = r.is_err() || self.vm.fatal_emitted;
        if let Err(e) = r {
            if self.vm.current_exception.is_some() || !self.vm.fatal_emitted {
                self.vm.report_uncaught(&e);
            }
            self.vm.run_shutdown_destructors();
        }
        // php_error_cb(): a fatal error with display_errors off turns a
        // 200 response that has not started into a 500.
        if failed && !headers_sent_before_error {
            let display = self
                .vm
                .constants
                .get(b"display_errors".as_slice())
                .map(|v| v.to_php_string().to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let off = matches!(display.trim(), "" | "0" | "off" | "no" | "false" | "none");
            if off && goro_ext_standard::misc::peek_response_code() == 200 {
                goro_ext_standard::misc::set_status_line(b"HTTP/1.0 500 Internal Server Error");
            }
        }
        let body = self.vm.take_output();
        let headers = goro_ext_standard::misc::take_sent_headers();
        let head = build_script_headers(client, &headers);
        goro_ext_standard::request::shutdown();
        ScriptResponse { head, body }
    }
}

fn with_cwd<R>(dir: Option<std::path::PathBuf>, f: impl FnOnce() -> R) -> R {
    let old = std::env::current_dir().ok();
    if let Some(d) = &dir {
        let _ = std::env::set_current_dir(d);
    }
    let r = f();
    if dir.is_some() {
        if let Some(o) = old {
            let _ = std::env::set_current_dir(o);
        }
    }
    r
}

/// php_cli_server_dispatch()
fn dispatch(server: &Server, client: &mut Client) {
    let ext = &client.request.ext;
    let is_static = !(ext.len() == 3 && ext.eq_ignore_ascii_case(b"php"))
        || client.request.path_translated.is_none();

    if server.router.is_none() && is_static {
        begin_send_static(client);
        return;
    }

    let mut run = PhpRun::startup(server, client);
    let mut result: Result<Value, VmError> = Ok(Value::Null);

    if let Some(router) = &server.router {
        // The router runs without changing directory.
        let r = run.run_file(router);
        let decline = matches!(r, Ok(Value::False));
        if !decline {
            result = r.map(|_| Value::Null);
            let le = run.last_error();
            let code = goro_ext_standard::misc::peek_response_code() as i32;
            client.log_response(code, None, le.as_ref());
            let resp = run.shutdown(result, client);
            finish_script_response(client, resp);
            return;
        }
        if is_static {
            // The declined router's request ends; output it produced went
            // out with its headers before the static file.
            let out = run.shutdown(Ok(Value::Null), client);
            if !out.body.is_empty() {
                let mut buf = out.head;
                buf.extend_from_slice(&out.body);
                client.out = buf;
                client.out_pos = 0;
                client.send_all();
            }
            begin_send_static(client);
            return;
        }
    }

    let pt = client.request.path_translated.clone().unwrap_or_default();
    if pt.contains(&0) {
        send_error_page(client, 400);
        return;
    }
    // php_execute_script() changes to the script's directory.
    let dir = std::path::Path::new(std::ffi::OsStr::from_bytes(&pt))
        .parent()
        .map(|p| p.to_path_buf());
    if result.is_ok() {
        result = with_cwd(dir, || run.run_file(&pt));
    }
    let le = run.last_error();
    let code = goro_ext_standard::misc::peek_response_code() as i32;
    client.log_response(code, None, le.as_ref());
    let resp = run.shutdown(result, client);
    finish_script_response(client, resp);
}

fn finish_script_response(client: &mut Client, resp: ScriptResponse) {
    let mut buf = resp.head;
    if client.request.method() != Method::Head {
        buf.extend_from_slice(&resp.body);
    }
    client.out = buf;
    client.out_pos = 0;
    client.sending = true;
}

/// php_cli_server_parse_addr()
fn parse_addr(addr: &str) -> Option<(String, u16)> {
    fn port(s: &str) -> Option<u16> {
        // strtol() semantics with a fully numeric remainder
        let t = s.trim_start();
        let (neg, digits) = match t.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, t.strip_prefix('+').unwrap_or(t)),
        };
        if !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let v: i64 = if digits.is_empty() {
            0
        } else {
            digits.parse().ok()?
        };
        let v = if neg { -v } else { v };
        if !(0..=65535).contains(&v) {
            return None;
        }
        Some(v as u16)
    }
    if let Some(rest) = addr.strip_prefix('[') {
        let end = rest.find(']')?;
        let after = &rest[end + 1..];
        let p = after.strip_prefix(':')?;
        return Some((rest[..end].to_string(), port(p)?));
    }
    let colon = addr.find(':')?;
    Some((addr[..colon].to_string(), port(&addr[colon + 1..])?))
}

/// php_network_listen_socket()
fn listen_socket(host: &str, port: u16) -> Result<(TcpListener, u16), String> {
    let addrs: Vec<SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(_) => {
            return Err(format!(
                "php_network_getaddresses: getaddrinfo for {} failed: Name or service not known",
                host
            ));
        }
    };
    let mut last_err = String::from("?");
    for a in addrs {
        match TcpListener::bind(a) {
            Ok(l) => {
                let p = l.local_addr().map(|x| x.port()).unwrap_or(port);
                return Ok((l, p));
            }
            Err(e) => {
                let errno = e.raw_os_error().unwrap_or(0);
                last_err = strerror(errno);
                if errno == libc::EINVAL || errno == libc::EADDRINUSE {
                    return Err(last_err);
                }
            }
        }
    }
    Err(last_err)
}

/// do_cli_server()
pub fn do_cli_server(argv: &[Vec<u8>], ini: IniSettings) -> i32 {
    let mut bind_address: Option<Vec<u8>> = None;
    let mut document_root: Option<Vec<u8>> = None;
    let mut g = GetOpt::new(argv, false);
    let mut optarg = None;
    loop {
        let c = g.next(&mut optarg);
        if c == -1 {
            break;
        }
        let a = optarg.take();
        if c == 'S' as i32 {
            bind_address = a;
        } else if c == 't' as i32 {
            document_root = a;
        } else if c == 'q' as i32 {
            LOG_LEVEL.with(|l| {
                if l.get() > 1 {
                    l.set(l.get() - 1)
                }
            });
        }
    }
    let document_root: Vec<u8> = match document_root {
        Some(d) => {
            let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&d));
            match std::fs::metadata(p) {
                Err(_) => {
                    eprintln!("Directory {} does not exist.", String::from_utf8_lossy(&d));
                    return 1;
                }
                Ok(m) if !m.is_dir() => {
                    eprintln!("{} is not a directory.", String::from_utf8_lossy(&d));
                    return 1;
                }
                Ok(_) => std::fs::canonicalize(p)
                    .map(|c| c.as_os_str().as_bytes().to_vec())
                    .unwrap_or(d),
            }
        }
        None => std::env::current_dir()
            .map(|c| c.as_os_str().as_bytes().to_vec())
            .unwrap_or_else(|_| b".".to_vec()),
    };
    let router = if argv.len() > g.optind {
        Some(argv[g.optind].clone())
    } else {
        None
    };
    let addr = String::from_utf8_lossy(&bind_address.unwrap_or_default()).into_owned();
    let Some((host, port)) = parse_addr(&addr) else {
        eprintln!("Invalid address: {}", addr);
        return 1;
    };
    let (listener, port) = match listen_socket(&host, port) {
        Ok(l) => l,
        Err(e) => {
            if host.contains(':') {
                log_write(
                    LOG_ERROR,
                    &format!("Failed to listen on [{}]:{} (reason: {})", host, port, e),
                );
            } else {
                log_write(
                    LOG_ERROR,
                    &format!("Failed to listen on {}:{} (reason: {})", host, port, e),
                );
            }
            return 1;
        }
    };
    if listener.set_nonblocking(true).is_err() {
        log_write(LOG_ERROR, "Failed to make server socket non-blocking");
        return 1;
    }
    let workers = startup_workers();
    let server = Server {
        host: host.clone(),
        port,
        document_root,
        router,
        ini,
    };
    let ipv6 = host.contains(':');
    log_write(
        LOG_PROCESS,
        &format!(
            "PHP {} Development Server (http://{}{}{}:{}) started",
            crate::PHP_VERSION,
            if ipv6 { "[" } else { "" },
            host,
            if ipv6 { "]" } else { "" },
            port
        ),
    );
    unsafe {
        libc::signal(
            libc::SIGINT,
            sigint_handler as *const () as libc::sighandler_t,
        );
    }
    let r = event_loop(&server, &listener);
    if let Some(pids) = workers {
        for pid in pids {
            let mut st = 0;
            unsafe {
                libc::waitpid(pid, &mut st, 0);
            }
        }
    }
    r
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

extern "C" fn sigint_handler(_sig: libc::c_int) {
    RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// php_cli_server_startup_workers(): the master's worker pids.
fn startup_workers() -> Option<Vec<libc::pid_t>> {
    let workers = std::env::var("PHP_CLI_SERVER_WORKERS").ok()?;
    let n: i64 = workers.trim().parse().unwrap_or(0);
    WORKERS_MAX.with(|w| w.set(n));
    if n > 1 {
        let master = std::process::id() as libc::pid_t;
        let mut pids = Vec::new();
        for i in 0..n {
            let pid = unsafe { libc::fork() };
            if pid < 0 {
                WORKERS_MAX.with(|w| w.set(i + 1));
                return Some(pids);
            } else if pid == 0 {
                unsafe {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    if libc::getppid() != master {
                        libc::_exit(1);
                    }
                }
                return None;
            } else {
                pids.push(pid);
            }
        }
        Some(pids)
    } else {
        eprintln!("number of workers must be larger than 1");
        None
    }
}

fn event_loop(server: &Server, listener: &TcpListener) -> i32 {
    let mut clients: Vec<Client> = Vec::new();
    let lfd = listener.as_raw_fd();
    while RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
        let mut fds: Vec<libc::pollfd> = Vec::with_capacity(clients.len() + 1);
        fds.push(libc::pollfd {
            fd: lfd,
            events: libc::POLLIN,
            revents: 0,
        });
        for c in &clients {
            fds.push(libc::pollfd {
                fd: c.stream.as_raw_fd(),
                events: if c.sending {
                    libc::POLLOUT
                } else {
                    libc::POLLIN
                },
                revents: 0,
            });
        }
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 1000) };
        if n < 0 {
            let err = last_errno();
            if err == libc::EINTR {
                continue;
            }
            log_write(LOG_ERROR, &strerror(err));
            return 1;
        }
        if n == 0 {
            continue;
        }
        // Client events first (in fd order like the select() loop), then accept.
        let mut closed: Vec<usize> = Vec::new();
        for (i, c) in clients.iter_mut().enumerate() {
            let re = fds[i + 1].revents;
            if re == 0 {
                continue;
            }
            if c.sending {
                c.send_all();
                close_connection(c);
                closed.push(i);
                continue;
            }
            match c.read_request(server) {
                Err(e) => {
                    if e == "Unexpected EOF" && c.parser.is_start_state() {
                        log_write(
                            LOG_MESSAGE,
                            &format!(
                                "{} Closed without sending a request; it was probably just an unused speculative preconnection",
                                c.addr_str
                            ),
                        );
                    } else {
                        log_write(
                            LOG_ERROR,
                            &format!("{} Invalid request ({})", c.addr_str, e),
                        );
                    }
                    close_connection(c);
                    closed.push(i);
                }
                Ok(true) => {
                    if c.request.method() == Method::NotImplemented {
                        send_error_page(c, 501);
                    } else {
                        dispatch(server, c);
                    }
                    if c.sending {
                        c.send_all();
                    }
                    close_connection(c);
                    closed.push(i);
                }
                Ok(false) => {}
            }
        }
        for i in closed.into_iter().rev() {
            let c = clients.remove(i);
            let _ = c.stream.shutdown(std::net::Shutdown::Both);
        }
        if fds[0].revents & libc::POLLIN != 0 {
            match listener.accept() {
                Ok((s, addr)) => {
                    if s.set_nonblocking(true).is_err() {
                        continue;
                    }
                    let c = Client::new(s, addr);
                    log_write(LOG_MESSAGE, &format!("{} Accepted", c.addr_str));
                    clients.push(c);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    log_write(
                        LOG_ERROR,
                        &format!(
                            "Failed to accept a client (reason: {})",
                            strerror(e.raw_os_error().unwrap_or(0))
                        ),
                    );
                }
            }
        }
    }
    0
}

/// php_cli_server_close_connection()
fn close_connection(c: &Client) {
    log_write(LOG_MESSAGE, &format!("{} Closing", c.addr_str));
}
