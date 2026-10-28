//! The `http://` and `https://` stream wrappers (PHP manual: HTTP and
//! HTTPS wrappers, HTTP context options, SSL context options,
//! $http_response_header, get_headers(),
//! http_get_last_response_headers()), speaking HTTP/1.x (RFC 9110,
//! RFC 9112) over the socket layer of builtins/sockets.rs.
//!
//! Request and response details not in the manual follow the tests in
//! ext/standard/tests/http/ and behaviour observed on PHP 8.5: the order
//! of the generated request headers, how user headers are detected and
//! stripped on redirects, header folding and validation messages,
//! relative redirect resolution, and the notification callbacks.

use std::time::Duration;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::file::{Backing, Stream};
use super::sockets::{Sock, context_option, default_context, qfloat, qint, qstr, with_context};
use super::streams::{NativeStream, NativeWrapper, register_wrapper};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "http_wrapper",
    functions: &[
        (
            "function http_get_last_response_headers(): ?array",
            http_get_last_response_headers,
        ),
        (
            "function http_clear_last_response_headers(): void",
            http_clear_last_response_headers,
        ),
        (
            "function get_headers(string $url, bool $associative = false, $context = null): array|false",
            get_headers,
        ),
    ],
    constants: &[
        ("STREAM_NOTIFY_RESOLVE", ConstVal::Int(1)),
        ("STREAM_NOTIFY_CONNECT", ConstVal::Int(2)),
        ("STREAM_NOTIFY_AUTH_REQUIRED", ConstVal::Int(3)),
        ("STREAM_NOTIFY_MIME_TYPE_IS", ConstVal::Int(4)),
        ("STREAM_NOTIFY_FILE_SIZE_IS", ConstVal::Int(5)),
        ("STREAM_NOTIFY_REDIRECTED", ConstVal::Int(6)),
        ("STREAM_NOTIFY_PROGRESS", ConstVal::Int(7)),
        ("STREAM_NOTIFY_COMPLETED", ConstVal::Int(8)),
        ("STREAM_NOTIFY_FAILURE", ConstVal::Int(9)),
        ("STREAM_NOTIFY_AUTH_RESULT", ConstVal::Int(10)),
        ("STREAM_NOTIFY_SEVERITY_INFO", ConstVal::Int(0)),
        ("STREAM_NOTIFY_SEVERITY_WARN", ConstVal::Int(1)),
        ("STREAM_NOTIFY_SEVERITY_ERR", ConstVal::Int(2)),
    ],
    ini: &[("from", "")],
    init: Some(init),
    listed: false,
    ..Module::EMPTY
};

static WRAPPER: NativeWrapper = NativeWrapper { open: wrapper_open };

fn init(vm: &mut Vm) {
    register_wrapper(vm, "http", &WRAPPER);
    register_wrapper(vm, "https", &WRAPPER);
}

/// The headers of the last request (http_get_last_response_headers()).
#[derive(Default)]
struct LastHeaders(Option<PhpArray>);

const NOTIFY_CONNECT: i64 = 2;
const NOTIFY_MIME_TYPE_IS: i64 = 4;
const NOTIFY_FILE_SIZE_IS: i64 = 5;
const NOTIFY_REDIRECTED: i64 = 6;
const NOTIFY_PROGRESS: i64 = 7;
const NOTIFY_COMPLETED: i64 = 8;
const NOTIFY_FAILURE: i64 = 9;

/// The longest Location value followed (test ghsa-52jp-hrpf-2jff-002).
const LOCATION_LIMIT: usize = 8182;
/// The longest status line kept (test gh9316.phpt).
const STATUS_LIMIT: usize = 126;
/// The largest timeout (seconds) accepted (test gh16810.phpt; observed
/// on PHP 8.5: microseconds held in 64 bits).
const TIMEOUT_LIMIT: f64 = 18446744073709.0;

fn http_get_last_response_headers(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(match &vm.ext::<LastHeaders>().0 {
        Some(h) => Value::Array(h.clone()),
        None => Value::Null,
    })
}

fn http_clear_last_response_headers(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.ext::<LastHeaders>().0 = None;
    Ok(Value::Null)
}

/// The context argument of the function opening the stream (its position
/// differs), or the default context.
fn context_arg(a: &Args) -> Value {
    let name = a.fname().to_ascii_lowercase();
    let idx = match name.as_str() {
        "fopen" => Some(3),
        "file_get_contents" | "file" | "readfile" | "copy" | "get_headers" => Some(2),
        _ => None,
    };
    if let Some(i) = idx {
        let v = a.val(i);
        if with_context(&v, |_| ()).is_some() {
            return v;
        }
    }
    default_context()
}

/// The stream_context_set_params() "notification" callback.
struct Notifier(Option<Value>);

impl Notifier {
    fn of(ctx: &Value) -> Notifier {
        Notifier(
            with_context(ctx, |c| {
                c.params
                    .borrow()
                    .get_str(&PhpStr::from("notification"))
                    .map(|v| v.deref())
            })
            .flatten()
            .filter(|v| !matches!(v, Value::Null)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn send(
        &self,
        vm: &mut Vm,
        code: i64,
        severity: i64,
        msg: Option<&[u8]>,
        msg_code: i64,
        done: i64,
        max: i64,
    ) -> R<()> {
        let Some(cb) = &self.0 else {
            return Ok(());
        };
        let args = vec![
            Value::Int(code),
            Value::Int(severity),
            msg.map_or(Value::Null, |m| Value::Str(PhpStr::new(m))),
            Value::Int(msg_code),
            Value::Int(done),
            Value::Int(max),
        ];
        vm.call_value(cb, args).map(|_| ())
    }
}

/// A connection with read-ahead, for the status line and headers.
struct Conn {
    sock: Sock,
    buf: Vec<u8>,
    pos: usize,
}

impl Conn {
    fn fill(&mut self, vm: &mut Vm) -> bool {
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let mut chunk = [0u8; 8192];
        match self.sock.read(vm, &mut chunk) {
            Ok(n) if n > 0 => {
                self.buf.extend_from_slice(&chunk[..n]);
                true
            }
            _ => false,
        }
    }

    /// The next line with its "\n" (a last unterminated line too);
    /// none at the end.
    fn line(&mut self, vm: &mut Vm) -> Option<Vec<u8>> {
        loop {
            if let Some(p) = self.buf[self.pos..].iter().position(|&c| c == b'\n') {
                let l = self.buf[self.pos..self.pos + p + 1].to_vec();
                self.pos += p + 1;
                return Some(l);
            }
            if !self.fill(vm) {
                if self.pos < self.buf.len() {
                    let l = self.buf[self.pos..].to_vec();
                    self.pos = self.buf.len();
                    return Some(l);
                }
                return None;
            }
        }
    }

    fn send(&mut self, vm: &mut Vm, mut data: &[u8]) -> bool {
        while !data.is_empty() {
            match self.sock.write(vm, data) {
                Ok(0) | Err(_) => return false,
                Ok(n) => data = &data[n..],
            }
        }
        true
    }
}

/// Transfer-Encoding: chunked decoding (RFC 9112 section 7.1), lenient
/// about bare LF line ends (test bug47021.phpt).
#[derive(Default)]
struct Dechunk {
    state: ChunkState,
    size: u64,
    line: Vec<u8>,
}

#[derive(Default, PartialEq)]
enum ChunkState {
    #[default]
    Size,
    Data,
    DataEnd,
    Trailer,
    Done,
}

impl Dechunk {
    fn feed(&mut self, mut input: &[u8], out: &mut Vec<u8>) {
        while !input.is_empty() {
            match self.state {
                ChunkState::Size | ChunkState::DataEnd | ChunkState::Trailer => {
                    let Some(p) = input.iter().position(|&c| c == b'\n') else {
                        self.line.extend_from_slice(input);
                        return;
                    };
                    self.line.extend_from_slice(&input[..p]);
                    input = &input[p + 1..];
                    let line = std::mem::take(&mut self.line);
                    let line = line.strip_suffix(b"\r").unwrap_or(&line);
                    match self.state {
                        ChunkState::Size => {
                            let hex: Vec<u8> = line
                                .iter()
                                .copied()
                                .skip_while(|c| *c == b' ' || *c == b'\t')
                                .take_while(|c| c.is_ascii_hexdigit())
                                .collect();
                            if hex.is_empty() {
                                // Not chunked after all: pass it on.
                                out.extend_from_slice(line);
                                out.push(b'\n');
                                out.extend_from_slice(input);
                                self.state = ChunkState::Done;
                                return;
                            }
                            let size = hex.iter().fold(0u64, |acc, &c| {
                                acc.saturating_mul(16)
                                    .saturating_add((c as char).to_digit(16).unwrap_or(0) as u64)
                            });
                            self.size = size;
                            self.state = if size == 0 {
                                ChunkState::Trailer
                            } else {
                                ChunkState::Data
                            };
                        }
                        ChunkState::DataEnd => self.state = ChunkState::Size,
                        _ => {
                            if line.is_empty() {
                                self.state = ChunkState::Done;
                            }
                        }
                    }
                }
                ChunkState::Data => {
                    let n = (self.size.min(input.len() as u64)) as usize;
                    out.extend_from_slice(&input[..n]);
                    input = &input[n..];
                    self.size -= n as u64;
                    if self.size == 0 {
                        self.state = ChunkState::DataEnd;
                    }
                }
                ChunkState::Done => return,
            }
        }
    }
}

/// An open http:// response body.
struct HttpStream {
    conn: Conn,
    chunked: Option<Dechunk>,
    /// Decoded bytes not read yet.
    out: Vec<u8>,
    headers: PhpArray,
    ctx: Value,
    notify: Notifier,
    transferred: i64,
    max: i64,
    done: bool,
    completed: bool,
}

impl HttpStream {
    fn raw(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        let c = &mut self.conn;
        if c.pos < c.buf.len() {
            let n = buf.len().min(c.buf.len() - c.pos);
            buf[..n].copy_from_slice(&c.buf[c.pos..c.pos + n]);
            c.pos += n;
            return Ok(n);
        }
        c.sock.read(vm, buf)
    }

    fn notify(&self, vm: &mut Vm, code: i64) {
        if let Err(e) = self
            .notify
            .send(vm, code, 0, None, 0, self.transferred, self.max)
            && vm.err.deferred.is_none()
        {
            vm.err.deferred = Some(e);
        }
    }
}

impl NativeStream for HttpStream {
    fn read(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = if self.chunked.is_some() {
            while self.out.is_empty() && !self.done {
                let mut raw = [0u8; 8192];
                let r = self.raw(vm, &mut raw);
                let d = self.chunked.as_mut().unwrap();
                match r {
                    Ok(0) => self.done = true,
                    Ok(k) => {
                        d.feed(&raw[..k], &mut self.out);
                        if d.state == ChunkState::Done {
                            self.done = true;
                        }
                    }
                    Err(e) => {
                        if self.out.is_empty() {
                            return Err(e);
                        }
                        break;
                    }
                }
            }
            let n = buf.len().min(self.out.len());
            buf[..n].copy_from_slice(&self.out[..n]);
            self.out.drain(..n);
            n
        } else {
            let n = self.raw(vm, buf)?;
            if n == 0 {
                self.done = true;
            }
            n
        };
        if n > 0 {
            self.transferred += n as i64;
            self.notify(vm, NOTIFY_PROGRESS);
        } else if !self.completed {
            self.completed = true;
            self.notify(vm, NOTIFY_COMPLETED);
        }
        Ok(n)
    }

    fn eof(&self) -> bool {
        if !self.out.is_empty() || self.conn.pos < self.conn.buf.len() {
            return false;
        }
        self.done || self.chunked.is_none() && NativeStream::eof(&self.conn.sock)
    }

    fn close(&mut self, vm: &mut Vm) -> R<()> {
        self.conn.sock.close(vm)
    }

    fn write(&mut self, vm: &mut Vm, data: &[u8]) -> std::io::Result<usize> {
        self.conn.sock.write(vm, data)
    }

    fn raw_fd(&self) -> Option<i32> {
        self.conn.sock.raw_fd()
    }

    fn buffered(&self) -> bool {
        !self.out.is_empty() || self.conn.pos < self.conn.buf.len() || self.conn.sock.buffered()
    }

    fn set_blocking(&mut self, blocking: bool) -> bool {
        self.conn.sock.set_blocking(blocking)
    }

    fn set_timeout(&mut self, secs: f64) -> bool {
        self.conn.sock.set_timeout(secs)
    }

    fn meta(&self, m: &mut PhpArray) {
        self.conn.sock.meta(m);
        let unread = self.out.len() + self.conn.buf.len() - self.conn.pos;
        m.insert(Key::from("unread_bytes"), Value::Int(unread as i64));
        m.insert(Key::from("eof"), Value::Bool(NativeStream::eof(self)));
    }

    fn context(&self) -> Option<Value> {
        Some(self.ctx.clone())
    }

    fn wrapper_data(&self) -> Option<Value> {
        Some(Value::Array(self.headers.clone()))
    }
}

/// An option of the context's "http" group.
fn opt(ctx: &Value, name: &str) -> Option<Value> {
    context_option(ctx, "http", name)
}

fn truthy(v: &Option<Value>) -> bool {
    v.as_ref().is_some_and(goro_runtime::convert::to_bool_quiet)
}

/// The user's "header" option as one block: array entries joined with
/// CRLF, surrounding whitespace trimmed, cut at the first empty line
/// (observed on PHP 8.5).
fn user_headers(v: Option<Value>) -> Vec<u8> {
    let mut h: Vec<u8> = match v {
        Some(Value::Array(arr)) => {
            let mut out = Vec::new();
            for (_, x) in arr.iter() {
                if let Value::Str(s) = x.deref() {
                    out.extend_from_slice(s.as_bytes());
                    out.extend_from_slice(b"\r\n");
                }
            }
            out
        }
        Some(Value::Null) | None => Vec::new(),
        Some(v) => qstr(&v).as_bytes().to_vec(),
    };
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0 | 0x0b);
    let end = h.iter().rposition(|c| !ws(c)).map_or(0, |p| p + 1);
    h.truncate(end);
    let start = h.iter().position(|c| !ws(c)).unwrap_or(h.len());
    h.drain(..start);
    if let Some(p) = h.windows(4).position(|w| w == b"\r\n\r\n") {
        h.truncate(p);
    }
    h
}

/// Whether the user headers set `name` (with its colon): at the start
/// or after a CRLF (test ghsa-hgf5-96fm-v528-001).
fn has_header(h: &[u8], name: &[u8]) -> bool {
    let n = name.len();
    (0..h.len()).any(|i| {
        (i == 0 || i >= 2 && &h[i - 2..i] == b"\r\n")
            && h.len() - i >= n
            && h[i..i + n].eq_ignore_ascii_case(name)
    })
}

/// Removes the `name` header lines (with their line end) from the user
/// headers, as a redirect turning the request into a GET does (test
/// bug61548.phpt).
fn strip_header(h: &mut Vec<u8>, name: &[u8]) {
    let n = name.len();
    let mut i = 0;
    while i < h.len() {
        let at_start = i == 0 || h[i - 1] == b'\n' || h[i - 1] == b'\r';
        if at_start && h.len() - i >= n && h[i..i + n].eq_ignore_ascii_case(name) {
            let mut e = i;
            while e < h.len() && h[e] != b'\r' && h[e] != b'\n' {
                e += 1;
            }
            if h[e..].starts_with(b"\r\n") {
                e += 2;
            } else if e < h.len() {
                e += 1;
            }
            h.drain(i..e);
            continue;
        }
        i += 1;
    }
}

/// URL user info as the wrapper decodes it: in place, keeping the
/// original length past a NUL (observed on PHP 8.5).
fn decode_userinfo(s: &[u8]) -> Vec<u8> {
    let d = goro_ext_standard::url::urldecode(s);
    if d.len() >= s.len() {
        return d;
    }
    let mut out = d.clone();
    out.push(0);
    out.extend_from_slice(&s[d.len() + 1..]);
    out
}

/// The response code of a status line: the number at offset 9, as
/// atoi() reads it (test bug75981.phpt).
fn status_code(line: &[u8]) -> i64 {
    if line.len() <= 9 {
        return 0;
    }
    let mut v: i64 = 0;
    for &c in line[9..].iter().take_while(|c| c.is_ascii_digit()).take(9) {
        v = v * 10 + (c - b'0') as i64;
    }
    v
}

fn atoi(s: &[u8]) -> i64 {
    let s = s.trim_ascii_start();
    let (neg, s) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut v: i64 = 0;
    for &c in s.iter().take_while(|c| c.is_ascii_digit()) {
        v = v.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    if neg { -v } else { v }
}

/// A line without its line end.
fn trim_eol(l: &[u8]) -> &[u8] {
    let l = l.strip_suffix(b"\n").unwrap_or(l);
    l.strip_suffix(b"\r").unwrap_or(l)
}

/// A failure: the message after "Failed to open stream: ".
enum Fail {
    Msg(String),
}

/// What one request left.
struct Parsed {
    code: i64,
    status_raw: Vec<u8>,
    location: Option<Vec<u8>>,
    chunked: bool,
    size: i64,
}

/// Where a URL points.
struct Target {
    https: bool,
    host: Vec<u8>,
    port: Option<i64>,
    user: Option<Vec<u8>>,
    pass: Option<Vec<u8>>,
    /// Path (at least "/") and query.
    resource: Vec<u8>,
    path: Vec<u8>,
}

fn parse_target(url: &[u8]) -> Option<Target> {
    let u = goro_ext_standard::url::parse_url(url)?;
    let scheme = u.scheme?.to_ascii_lowercase();
    let https = match &scheme[..] {
        b"http" => false,
        b"https" => true,
        _ => return None,
    };
    let host = u.host.filter(|h| !h.is_empty())?;
    let path = u
        .path
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| b"/".to_vec());
    let mut resource = path.clone();
    if let Some(q) = u.query {
        resource.push(b'?');
        resource.extend_from_slice(&q);
    }
    Some(Target {
        https,
        host,
        port: u.port,
        user: u.user,
        pass: u.pass,
        resource,
        path,
    })
}

/// Resolves a Location against the current URL (observed on PHP 8.5: a
/// relative location goes after the directory part and a "/").
fn resolve_location(t: &Target, loc: &[u8]) -> Vec<u8> {
    let scheme_end = loc
        .iter()
        .position(|&c| !(c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.'));
    if let Some(p) = scheme_end
        && p > 0
        && loc[p..].starts_with(b"://")
    {
        return loc.to_vec();
    }
    let mut out: Vec<u8> = if t.https {
        b"https://".to_vec()
    } else {
        b"http://".to_vec()
    };
    out.extend_from_slice(&t.host);
    if let Some(p) = t.port {
        out.extend_from_slice(format!(":{p}").as_bytes());
    }
    if loc.first() == Some(&b'/') {
        out.extend_from_slice(loc);
        return out;
    }
    let dir = match t.path.iter().rposition(|&c| c == b'/') {
        Some(0) | None => &t.path[..0],
        Some(p) => &t.path[..=p],
    };
    out.extend_from_slice(dir);
    out.push(b'/');
    out.extend_from_slice(loc);
    out
}

/// The transport address of a host and port ("tcp://[::1]:80").
fn transport_addr(host: &[u8], port: i64) -> Vec<u8> {
    let mut a = b"tcp://".to_vec();
    if host.contains(&b':') && !host.starts_with(b"[") {
        a.push(b'[');
        a.extend_from_slice(host);
        a.push(b']');
    } else {
        a.extend_from_slice(host);
    }
    a.extend_from_slice(format!(":{port}").as_bytes());
    a
}

/// The host part of a URL as written ("[::1" for "http://[::1/x").
fn raw_host(url: &[u8]) -> Vec<u8> {
    let rest = match url.windows(3).position(|w| w == b"://") {
        Some(p) => &url[p + 3..],
        None => url,
    };
    let end = rest
        .iter()
        .position(|&c| c == b'/' || c == b'?' || c == b'#')
        .unwrap_or(rest.len());
    let auth = &rest[..end];
    let auth = match auth.iter().rposition(|&c| c == b'@') {
        Some(at) => &auth[at + 1..],
        None => auth,
    };
    auth.to_vec()
}

fn connect(
    vm: &mut Vm,
    a: &Args,
    url: &[u8],
    addr: &[u8],
    host: &[u8],
    to: Option<Duration>,
) -> R<Result<Sock, Fail>> {
    if host.starts_with(b"[") && !host.contains(&b']') {
        return Ok(Err(Fail::Msg(format!(
            "Failed to parse IPv6 address \"{}\"",
            String::from_utf8_lossy(&raw_host(url))
        ))));
    }
    vm.out.flush();
    match super::sockets::connect_client(addr, to) {
        Ok(s) => Ok(Ok(s)),
        Err(m) => {
            if m.starts_with("php_network_getaddresses") {
                vm.fwarn(a, &m)?;
            }
            Ok(Err(Fail::Msg(m)))
        }
    }
}

fn wrapper_open(vm: &mut Vm, a: &Args, url: &[u8], mode: &[u8], quiet: bool) -> R<Option<Stream>> {
    let fname = a.fname();
    let scheme = &url[..url.windows(3).position(|w| w == b"://").unwrap_or(0)];
    let scheme = String::from_utf8_lossy(scheme).to_ascii_lowercase();
    let fail = |vm: &mut Vm, msg: &str| -> R<Option<Stream>> {
        if !quiet {
            vm.warn(&format!(
                "{}({}): Failed to open stream: {}",
                fname,
                String::from_utf8_lossy(url),
                msg
            ))?;
        }
        Ok(None)
    };
    let allowed = vm.ini_bool(b"allow_url_fopen", true);
    if !allowed {
        if !quiet {
            vm.warn(&format!(
                "{fname}(): {scheme}:// wrapper is disabled in the server configuration by allow_url_fopen=0"
            ))?;
        }
        return fail(vm, "no suitable wrapper could be found");
    }
    vm.ext::<LastHeaders>().0 = None;
    if mode
        .iter()
        .any(|c| matches!(c, b'w' | b'a' | b'x' | b'c' | b'+'))
    {
        return fail(vm, "HTTP wrapper does not support writeable connections");
    }
    let ctx = context_arg(a);
    let mut headers = PhpArray::new();
    let mut started = false;
    let r = request(vm, a, &ctx, url, &mut headers, &mut started)?;
    if started {
        vm.ext::<LastHeaders>().0 = Some(headers.clone());
        vm.assign_var_var(
            &Value::Str(PhpStr::from("http_response_header")),
            Value::Array(headers.clone()),
        )?;
    }
    match r {
        Ok(mut hs) => {
            hs.headers = headers;
            let mut s = Stream::new(
                Backing::Native(Box::new(hs)),
                mode,
                url,
                "http",
                "tcp_socket/ssl",
            );
            // Writes go to the connection (observed on PHP 8.5).
            s.readable = true;
            s.writable = true;
            Ok(Some(s))
        }
        Err(Fail::Msg(m)) => fail(vm, &m),
    }
}

/// Runs the request (and its redirects).
fn request(
    vm: &mut Vm,
    a: &Args,
    ctx: &Value,
    url: &[u8],
    headers: &mut PhpArray,
    started: &mut bool,
) -> R<Result<HttpStream, Fail>> {
    let notify = Notifier::of(ctx);
    let timeout = match opt(ctx, "timeout") {
        Some(v) => {
            let t = qfloat(&v);
            if t > TIMEOUT_LIMIT {
                return Ok(Err(Fail::Msg(format!(
                    "timeout must be lower than {}",
                    TIMEOUT_LIMIT as u64
                ))));
            }
            t
        }
        None => vm
            .get_ini(b"default_socket_timeout")
            .map(|v| goro_runtime::convert::str_to_float(v.as_bytes()))
            .unwrap_or(60.0),
    };
    let to = super::sockets::timeout_of(timeout);
    let follow =
        opt(ctx, "follow_location").is_none_or(|v| goro_runtime::convert::to_bool_quiet(&v));
    let max_redirects = opt(ctx, "max_redirects").map_or(20, |v| qint(&v));
    let ignore_errors = truthy(&opt(ctx, "ignore_errors"));
    let mut user = user_headers(opt(ctx, "header"));
    let mut method: Vec<u8> = opt(ctx, "method")
        .map(|v| qstr(&v).as_bytes().to_vec())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| b"GET".to_vec());
    let mut content: Vec<u8> = opt(ctx, "content")
        .map(|v| qstr(&v).as_bytes().to_vec())
        .unwrap_or_default();
    let version: Vec<u8> = match opt(ctx, "protocol_version") {
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(v @ (Value::Int(_) | Value::Float(_) | Value::Bool(_))) => {
            format!("{:.1}", qfloat(&v)).into_bytes()
        }
        _ => b"1.1".to_vec(),
    };
    let orig_method = method.clone();
    let orig_content = content.clone();
    let orig_user = user.clone();
    let proxy = opt(ctx, "proxy")
        .map(|v| qstr(&v).as_bytes().to_vec())
        .filter(|p| !p.is_empty());
    let fulluri = truthy(&opt(ctx, "request_fulluri"));
    let user_agent = match opt(ctx, "user_agent") {
        Some(v) => qstr(&v).as_bytes().to_vec(),
        None => vm
            .get_ini(b"user_agent")
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default(),
    };
    let mut cur = url.to_vec();
    let mut hop: i64 = 0;
    loop {
        let Some(t) = parse_target(&cur) else {
            return Ok(Err(Fail::Msg("operation failed".into())));
        };
        let port = t.port.unwrap_or(if t.https { 443 } else { 80 });
        let use_proxy = proxy.is_some();
        let mut target = t.resource.clone();
        if use_proxy && fulluri && !t.https {
            let mut full = cur.clone();
            if let Some(h) = full.iter().position(|&c| c == b'#') {
                full.truncate(h);
            }
            if full.iter().any(|&c| c == b'\r' || c == b'\n') {
                return Ok(Err(Fail::Msg(
                    "HTTP wrapper full URI path does not allow CR or LF characters".into(),
                )));
            }
            target = full;
        }
        let addr = match &proxy {
            Some(p) => p.clone(),
            None => transport_addr(&t.host, port),
        };
        let mut sock = match connect(vm, a, &cur, &addr, &t.host, to)? {
            Ok(s) => s,
            Err(f) => return Ok(Err(f)),
        };
        sock.ctx = Some(ctx.clone());
        sock.host = t.host.clone();
        sock.timeout = to;
        let mut conn = Conn {
            sock,
            buf: Vec::new(),
            pos: 0,
        };
        if t.https {
            if use_proxy {
                let hp = format!("{}:{}", String::from_utf8_lossy(&t.host), port);
                let req = format!("CONNECT {hp} HTTP/1.1\r\nHost: {hp}\r\n\r\n");
                if !conn.send(vm, req.as_bytes()) {
                    return Ok(Err(Fail::Msg("HTTP request failed!".into())));
                }
                let st = conn.line(vm).unwrap_or_default();
                while let Some(l) = conn.line(vm) {
                    if trim_eol(&l).is_empty() {
                        break;
                    }
                }
                if !(200..300).contains(&status_code(&st)) {
                    return Ok(Err(Fail::Msg("HTTP request failed!".into())));
                }
            }
            let r = super::openssl::tls::enable(vm, a, &mut conn.sock, None, false)?;
            if r != super::openssl::tls::Enabled::Yes {
                return Ok(Err(Fail::Msg("operation failed".into())));
            }
        }
        *started = true;
        notify.send(vm, NOTIFY_CONNECT, 0, None, 0, 0, 0)?;
        // The request (header order observed on PHP 8.5 and in the tests
        // bug48929.phpt, bug53198.phpt, bug79265.phpt).
        let mut req = Vec::new();
        req.extend_from_slice(&method);
        req.push(b' ');
        req.extend_from_slice(&target);
        req.extend_from_slice(b" HTTP/");
        req.extend_from_slice(&version);
        req.extend_from_slice(b"\r\n");
        if let Some(u) = &t.user
            && !has_header(&user, b"authorization:")
        {
            let mut cred = decode_userinfo(u);
            cred.push(b':');
            if let Some(p) = &t.pass {
                cred.extend_from_slice(&decode_userinfo(p));
            }
            req.extend_from_slice(b"Authorization: Basic ");
            req.extend_from_slice(&goro_ext_standard::url::base64_encode(&cred));
            req.extend_from_slice(b"\r\n");
        }
        if let Some(from) = vm.get_ini(b"from")
            && !from.is_empty()
            && !has_header(&user, b"from:")
        {
            req.extend_from_slice(b"From: ");
            req.extend_from_slice(from.as_bytes());
            req.extend_from_slice(b"\r\n");
        }
        if !has_header(&user, b"host:") {
            req.extend_from_slice(b"Host: ");
            if t.host.contains(&b':') && !t.host.starts_with(b"[") {
                req.push(b'[');
                req.extend_from_slice(&t.host);
                req.push(b']');
            } else {
                req.extend_from_slice(&t.host);
            }
            if let Some(p) = t.port {
                req.extend_from_slice(format!(":{p}").as_bytes());
            }
            req.extend_from_slice(b"\r\n");
        }
        if !has_header(&user, b"connection:") {
            req.extend_from_slice(b"Connection: close\r\n");
        }
        if !user_agent.is_empty() && !has_header(&user, b"user-agent:") {
            req.extend_from_slice(b"User-Agent: ");
            req.extend_from_slice(&user_agent);
            req.extend_from_slice(b"\r\n");
        }
        if !content.is_empty() && !has_header(&user, b"content-length:") {
            req.extend_from_slice(format!("Content-Length: {}\r\n", content.len()).as_bytes());
        }
        if !user.is_empty() {
            req.extend_from_slice(&user);
            req.extend_from_slice(b"\r\n");
        }
        if !content.is_empty() && !has_header(&user, b"content-type:") {
            vm.fnotice(
                a,
                "Content-type not specified assuming application/x-www-form-urlencoded",
            )?;
            req.extend_from_slice(b"Content-Type: application/x-www-form-urlencoded\r\n");
        }
        req.extend_from_slice(b"\r\n");
        req.extend_from_slice(&content);
        if !conn.send(vm, &req) {
            return Ok(Err(Fail::Msg("HTTP request failed!".into())));
        }
        let p = match read_response(vm, &mut conn, headers, &notify, ignore_errors)? {
            Ok(p) => p,
            Err(f) => return Ok(Err(f)),
        };
        let redirect = (300..400).contains(&p.code) || p.code == 201;
        if redirect
            && follow
            && let Some(loc) = &p.location
        {
            if hop + 1 >= max_redirects {
                if ignore_errors {
                    return Ok(Ok(body_stream(conn, &p, ctx, notify)));
                }
                return Ok(Err(Fail::Msg("Redirection limit reached, aborting".into())));
            }
            notify.send(vm, NOTIFY_REDIRECTED, 0, Some(loc), 0, 0, 0)?;
            let next = resolve_location(&t, loc);
            drop(conn);
            // A redirect other than 307/308 repeats the request as a GET
            // without a body (test gh11274.phpt); a later 307/308 goes
            // back to the request asked for (observed on PHP 8.5).
            let keep = orig_method.eq_ignore_ascii_case(b"GET")
                || orig_method.eq_ignore_ascii_case(b"HEAD");
            if p.code != 307 && p.code != 308 && !keep {
                method = b"GET".to_vec();
                content.clear();
                user = orig_user.clone();
                strip_header(&mut user, b"content-type:");
                strip_header(&mut user, b"content-length:");
            } else {
                method = orig_method.clone();
                content = orig_content.clone();
                user = orig_user.clone();
            }
            cur = next;
            hop += 1;
            continue;
        }
        if !(200..400).contains(&p.code) && !ignore_errors {
            let mut m = "HTTP request failed!".to_string();
            if p.code != -1 {
                m.push(' ');
                m.push_str(&String::from_utf8_lossy(&p.status_raw));
            }
            return Ok(Err(Fail::Msg(m)));
        }
        return Ok(Ok(body_stream(conn, &p, ctx, notify)));
    }
}

fn body_stream(conn: Conn, p: &Parsed, ctx: &Value, notify: Notifier) -> HttpStream {
    HttpStream {
        conn,
        chunked: p.chunked.then(Dechunk::default),
        out: Vec::new(),
        headers: PhpArray::new(),
        ctx: ctx.clone(),
        notify,
        transferred: 0,
        max: p.size,
        done: false,
        completed: false,
    }
}

/// Reads the status line and headers. A code of -1 means no status line
/// came at all.
fn read_response(
    vm: &mut Vm,
    conn: &mut Conn,
    headers: &mut PhpArray,
    notify: &Notifier,
    ignore_errors: bool,
) -> R<Result<Parsed, Fail>> {
    let Some(mut raw) = conn.line(vm) else {
        return Ok(Ok(Parsed {
            code: -1,
            status_raw: Vec::new(),
            location: None,
            chunked: false,
            size: 0,
        }));
    };
    let mut code = status_code(&raw);
    // Interim 1xx responses (but 101) are skipped (test bug73297.phpt);
    // at the end the last line read stands as the status line (test
    // bug75981.phpt, observed on PHP 8.5).
    while (100..200).contains(&code) && code != 101 {
        let mut last = raw.clone();
        while let Some(l) = conn.line(vm) {
            let blank = trim_eol(&l).is_empty();
            last = l;
            if blank {
                break;
            }
        }
        raw = conn.line(vm).unwrap_or(last);
        code = status_code(&raw);
    }
    let mut status = trim_eol(&raw).to_vec();
    let status_raw = if status.len() > STATUS_LIMIT {
        status.truncate(STATUS_LIMIT);
        status.clone()
    } else {
        raw.clone()
    };
    let _ = headers.push(Value::Str(PhpStr::from(status)));
    if !(200..400).contains(&code) && !ignore_errors {
        notify.send(vm, NOTIFY_FAILURE, 2, Some(&status_raw), code, 0, 0)?;
    }
    let mut p = Parsed {
        code,
        status_raw,
        location: None,
        chunked: false,
        size: 0,
    };
    // Header lines, a folded line joining the one before (RFC 9112
    // section 5.2); a header is handled once the next line shows it is
    // complete, so one cut off by the end is dropped (test
    // bug75535.phpt).
    let mut pending: Option<Vec<u8>> = None;
    let mut first = true;
    while let Some(l) = conn.line(vm) {
        let line = trim_eol(&l);
        if line.is_empty() {
            if let Some(h) = pending.take()
                && let Err(f) = header(vm, &h, headers, notify, &mut p)?
            {
                return Ok(Err(f));
            }
            break;
        }
        if line[0] == b' ' || line[0] == b'\t' {
            if first {
                return Ok(Err(Fail::Msg(
                    "HTTP invalid response format (folding header at the start)!".into(),
                )));
            }
            let more = line.trim_ascii();
            if let Some(h) = &mut pending
                && !more.is_empty()
            {
                h.push(b' ');
                h.extend_from_slice(more);
            }
            continue;
        }
        first = false;
        if let Some(h) = pending.take()
            && let Err(f) = header(vm, &h, headers, notify, &mut p)?
        {
            return Ok(Err(f));
        }
        if line[0] == b'\r' {
            return Ok(Err(Fail::Msg(
                "HTTP invalid header name (cannot start with CR character)!".into(),
            )));
        }
        pending = Some(line.trim_ascii_end().to_vec());
    }
    Ok(Ok(p))
}

/// Validates and records one (unfolded) header line.
fn header(
    vm: &mut Vm,
    h: &[u8],
    headers: &mut PhpArray,
    notify: &Notifier,
    p: &mut Parsed,
) -> R<Result<(), Fail>> {
    let Some(colon) = h.iter().position(|&c| c == b':') else {
        return Ok(Err(Fail::Msg(
            "HTTP invalid response format (no colon in header line)!".into(),
        )));
    };
    let name = &h[..colon];
    if name.iter().any(|&c| c == b' ' || c == b'\t') {
        return Ok(Err(Fail::Msg(
            "HTTP invalid response format (space in header name)!".into(),
        )));
    }
    let value = h[colon + 1..].trim_ascii_start();
    if name.eq_ignore_ascii_case(b"location") {
        if value.len() > LOCATION_LIMIT {
            return Ok(Err(Fail::Msg(format!(
                "HTTP Location header size is over the limit of {LOCATION_LIMIT} bytes"
            ))));
        }
        p.location = Some(value.to_vec());
    }
    let _ = headers.push(Value::Str(PhpStr::new(h)));
    if name.eq_ignore_ascii_case(b"content-type") {
        notify.send(vm, NOTIFY_MIME_TYPE_IS, 0, Some(value), 0, 0, 0)?;
    } else if name.eq_ignore_ascii_case(b"content-length") {
        p.size = atoi(value);
        notify.send(vm, NOTIFY_FILE_SIZE_IS, 0, Some(h), 0, 0, p.size)?;
    } else if name.eq_ignore_ascii_case(b"transfer-encoding")
        && value.eq_ignore_ascii_case(b"chunked")
    {
        p.chunked = true;
    }
    Ok(Ok(()))
}

/// get_headers() (PHP manual): the response headers of a request, by
/// line or by name.
fn get_headers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let url = a.str(0);
    let assoc = a.bool(1);
    let u = url.as_bytes();
    let is_http = u.len() > 7 && u[..7].eq_ignore_ascii_case(b"http://")
        || u.len() > 8 && u[..8].eq_ignore_ascii_case(b"https://");
    let allowed = vm.ini_bool(b"allow_url_fopen", true);
    if is_http && !allowed {
        let scheme = if u[4] == b':' { "http" } else { "https" };
        vm.fwarn(
            a,
            &format!(
                "{scheme}:// wrapper is disabled in the server configuration by allow_url_fopen=0"
            ),
        )?;
    }
    if !is_http || !allowed {
        vm.fwarn(a, "This function may only be used against URLs")?;
        return Ok(Value::Bool(false));
    }
    let Some(mut s) = wrapper_open(vm, a, u, b"r", false)? else {
        return Ok(Value::Bool(false));
    };
    let lines = match &s.backing {
        Backing::Native(n) => match n.wrapper_data() {
            Some(Value::Array(h)) => h,
            _ => PhpArray::new(),
        },
        _ => PhpArray::new(),
    };
    super::streams::close_stream(vm, &mut s)?;
    if !assoc {
        return Ok(Value::Array(lines));
    }
    let mut out = PhpArray::new();
    for (_, v) in lines.iter() {
        let line = qstr(v);
        let b = line.as_bytes();
        match b.iter().position(|&c| c == b':') {
            Some(c) => {
                let key = Key::from_php_str(&PhpStr::new(&b[..c]));
                let val = Value::Str(PhpStr::new(b[c + 1..].trim_ascii_start()));
                match out.get(key.as_ref()).map(|x| x.deref()) {
                    Some(Value::Array(mut arr)) => {
                        let _ = arr.push(val);
                        out.insert(key, Value::Array(arr));
                    }
                    Some(prev) => {
                        out.insert(key, Value::Array(PhpArray::from_values(vec![prev, val])));
                    }
                    None => {
                        out.insert(key, val);
                    }
                }
            }
            None => {
                let _ = out.push(Value::Str(line.clone()));
            }
        }
    }
    Ok(Value::Array(out))
}
