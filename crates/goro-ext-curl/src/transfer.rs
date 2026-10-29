//! The transfer engine behind curl_easy_perform(3): HTTP/1.x (RFC 9110,
//! RFC 9112) over TCP, TLS or Unix sockets, through HTTP and SOCKS
//! proxies, with redirects, Basic authentication, cookies, uploads,
//! content decoding, and file:// URLs.
//!
//! Behaviour follows the libcurl documentation of each option and what
//! PHP 8.5 with libcurl 8.18 was observed to send and report: the order
//! of the generated request headers, which headers user headers replace or
//! remove, the verbose texts, the result codes and their messages.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::consts as c;
use crate::cookie::CookieJar;
use crate::info::Info;
use crate::multipart;
use crate::net::{self, Conn, NetError, Sock};
use crate::options::{Body, Options, SUPPORTED, proto_bit};
use crate::url::{self, Url, UrlError};

/// What a read callback produced.
pub enum ReadResult {
    /// Bytes to send (empty at the end).
    Data(Vec<u8>),
    /// CURL_READFUNC_PAUSE.
    Pause,
    /// CURL_READFUNC_ABORT.
    Abort,
    /// A value that is neither a string nor a known code.
    Funny,
}

/// The application side of a transfer: where data goes and comes from.
pub trait Host {
    /// Body data; returns how much was taken (less is an error).
    fn write(&mut self, data: &[u8]) -> usize;
    /// One header line (with its CRLF); returns how much was taken.
    fn header(&mut self, data: &[u8]) -> usize;
    /// Upload data, at most `max` bytes.
    fn read(&mut self, max: usize) -> ReadResult;
    /// Progress (CURLOPT_NOPROGRESS off): false aborts.
    fn progress(&mut self, dltotal: i64, dlnow: i64, ultotal: i64, ulnow: i64) -> bool;
    /// A debug event (CURLOPT_VERBOSE): CURLINFO_TEXT, CURLINFO_HEADER_IN,
    /// ... and its data.
    fn debug(&mut self, kind: i64, data: &[u8]);
    /// The pre-request callback: None without one, Some(false) to abort.
    fn prereq(
        &mut self,
        remote_ip: &str,
        local_ip: &str,
        remote_port: u16,
        local_port: u16,
    ) -> Option<bool>;
    /// Whether the application stopped the transfer (a fatal error or an
    /// exit in a callback).
    fn stopped(&self) -> bool;
    /// A random number (multipart boundaries).
    fn random(&mut self) -> u64;
}

/// A failed transfer: the CURLE_* code and the error buffer text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: i64,
    pub message: String,
}

fn fail<T>(code: i64, message: impl Into<String>) -> Result<T, Failure> {
    Err(Failure {
        code,
        message: message.into(),
    })
}

/// A connection cache (an easy handle's own, a multi handle's, or a share
/// handle's): the connection numbering of CURLINFO_CONN_ID and the idle
/// connections kept for reuse.
#[derive(Default)]
pub struct Pool {
    pub next_id: i64,
    idle: Vec<(String, Conn, bool)>,
}

/// Idle connections kept per cache (libcurl's default CURLOPT_MAXCONNECTS
/// is 5).
const MAX_IDLE: usize = 5;

impl Pool {
    fn take(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// An idle connection for `key` that the peer has not closed, and
    /// whether requests on it use absolute targets (an HTTP proxy).
    fn take_idle(&mut self, key: &str) -> Option<(Conn, bool)> {
        while let Some(i) = self.idle.iter().position(|(k, _, _)| k == key) {
            let (_, mut c, abs) = self.idle.remove(i);
            if c.is_alive() {
                return Some((c, abs));
            }
            c.close();
        }
        None
    }

    fn put_idle(&mut self, key: String, conn: Conn, absolute: bool) {
        if self.idle.len() >= MAX_IDLE {
            let (_, mut old, _) = self.idle.remove(0);
            old.close();
        }
        self.idle.push((key, conn, absolute));
    }

    /// Closes the idle connections.
    pub fn clear(&mut self) {
        for (_, mut c, _) in self.idle.drain(..) {
            c.close();
        }
    }
}

const TEXT: i64 = 0;
const HEADER_IN: i64 = 1;
const HEADER_OUT: i64 = 2;
const DATA_IN: i64 = 3;
const DATA_OUT: i64 = 4;

/// Result codes PHP has no constant for (libcurl-errors(3)).
const E_PEER_FAILED_VERIFICATION: i64 = 60;
const E_SEND_FAIL_REWIND: i64 = 65;
const E_SSL_ISSUER_ERROR: i64 = 83;
const E_RANGE_ERROR: i64 = 33;

/// Runs a transfer.
pub fn perform(
    opts: &Options,
    host: &mut dyn Host,
    jar: &mut CookieJar,
    pool: &mut Pool,
) -> (Info, Result<(), Failure>) {
    let mut t = Transfer {
        opts,
        host,
        jar,
        pool,
        info: Info::default(),
        start: Instant::now(),
        deadline: if opts.timeout_ms > 0 {
            Some(Instant::now() + Duration::from_millis(opts.timeout_ms as u64))
        } else {
            None
        },
        received: 0,
        expected: None,
        redirect_base_us: 0,
    };
    t.info.upload_content_length = -1;
    let r = t.run();
    t.info.total_us = t.us().max(1);
    if let Err(f) = &r
        && t.opts.verbose
        && !f.message.is_empty()
    {
        let m = format!("{}\n", f.message);
        t.host.debug(TEXT, m.as_bytes());
    }
    (t.info, r)
}

struct Transfer<'a> {
    opts: &'a Options,
    host: &'a mut dyn Host,
    jar: &'a mut CookieJar,
    pool: &'a mut Pool,
    info: Info,
    start: Instant,
    deadline: Option<Instant>,
    /// Body bytes received (timeout messages).
    received: i64,
    /// The announced body size (timeout messages).
    expected: Option<i64>,
    redirect_base_us: i64,
}

/// What the request sends as its body.
#[derive(Clone)]
enum Plan {
    None,
    Bytes {
        data: Vec<u8>,
        ctype: Vec<u8>,
    },
    /// From the read callback, `size` bytes (None: unknown).
    Reader {
        size: Option<i64>,
    },
}

/// A response head.
struct Head {
    version: i64,
    code: i64,
    lines: Vec<Vec<u8>>,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Head {
    fn get(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .rev()
            .find(|(n, _)| n.eq_ignore_ascii_case(name.as_bytes()))
            .map(|(_, v)| v.as_slice())
    }
    fn all(&self, name: &str) -> impl Iterator<Item = &[u8]> {
        self.headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name.as_bytes()))
            .map(|(_, v)| v.as_slice())
    }
}

/// A parsed proxy setting.
struct Proxy {
    kind: i64,
    host: String,
    port: u16,
    user: Option<Vec<u8>>,
    pass: Option<Vec<u8>>,
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

fn base64(data: &[u8]) -> Vec<u8> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for ch in data.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63]);
        out.push(T[(n >> 12) as usize & 63]);
        out.push(if ch.len() > 1 {
            T[(n >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if ch.len() > 2 {
            T[n as usize & 63]
        } else {
            b'='
        });
    }
    out
}

/// An HTTP date (IMF-fixdate, RFC 9110 section 5.6.7).
pub fn http_date(t: i64) -> String {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let mon = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(m - 1) as usize];
    format!(
        "{wd}, {d:02} {mon} {y} {:02}:{:02}:{:02} GMT",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The user headers (CURLOPT_HTTPHEADER): what to send, and the lower-case
/// names they replace or remove among the generated ones.
/// The header lines to send, and per lower-case name what the user set
/// (None: removed).
type UserHeaders = (Vec<Vec<u8>>, Vec<(String, Option<Vec<u8>>)>);

fn user_headers(list: &[Vec<u8>]) -> UserHeaders {
    let mut send = Vec::new();
    let mut names = Vec::new();
    for h in list {
        let mut h = h.as_slice();
        while let [rest @ .., b'\r' | b'\n'] = h {
            h = rest;
        }
        if h.is_empty() {
            continue;
        }
        if let Some(p) = h.iter().position(|&c| c == b':') {
            let name = String::from_utf8_lossy(trim(&h[..p])).to_ascii_lowercase();
            let value = trim(&h[p + 1..]);
            if value.is_empty() {
                // "Name:" removes a generated header and sends nothing.
                names.push((name, None));
            } else {
                names.push((name, Some(value.to_vec())));
                send.push(h.to_vec());
            }
        } else if let Some(n) = h.strip_suffix(b";") {
            // "Name;" sends a header without a value.
            let mut l = trim(n).to_vec();
            l.push(b':');
            names.push((
                String::from_utf8_lossy(trim(n)).to_ascii_lowercase(),
                Some(Vec::new()),
            ));
            send.push(l);
        }
    }
    (send, names)
}

impl Transfer<'_> {
    fn us(&self) -> i64 {
        (self.start.elapsed().as_micros() as i64).max(1)
    }

    fn text(&mut self, s: &str) {
        if self.opts.verbose {
            let m = format!("{s}\n");
            self.host.debug(TEXT, m.as_bytes());
        }
    }

    fn check_stop(&self) -> Result<(), Failure> {
        if self.host.stopped() {
            return fail(c::CURLE_ABORTED_BY_CALLBACK, "Callback aborted");
        }
        Ok(())
    }

    fn progress(
        &mut self,
        dltotal: i64,
        dlnow: i64,
        ultotal: i64,
        ulnow: i64,
    ) -> Result<(), Failure> {
        if self.opts.noprogress {
            return Ok(());
        }
        let go = self.host.progress(dltotal, dlnow, ultotal, ulnow);
        self.check_stop()?;
        if !go {
            return fail(c::CURLE_ABORTED_BY_CALLBACK, "Callback aborted");
        }
        Ok(())
    }

    fn timeout_failure(&self) -> Failure {
        let ms = self.start.elapsed().as_millis();
        let message = match self.expected {
            Some(t) if t >= 0 => format!(
                "Operation timed out after {ms} milliseconds with {} out of {t} bytes received",
                self.received
            ),
            _ => format!(
                "Operation timed out after {ms} milliseconds with {} bytes received",
                self.received
            ),
        };
        Failure {
            code: c::CURLE_OPERATION_TIMEDOUT,
            message,
        }
    }

    fn recv_failure(&self, e: NetError) -> Failure {
        match e {
            NetError::Timeout => self.timeout_failure(),
            NetError::Io(e) => Failure {
                code: c::CURLE_RECV_ERROR,
                message: format!("Recv failure: {}", io_text(&e)),
            },
        }
    }

    fn send_failure(&self, e: NetError) -> Failure {
        match e {
            NetError::Timeout => self.timeout_failure(),
            NetError::Io(e) => Failure {
                code: c::CURLE_SEND_ERROR,
                message: format!("Send failure: {}", io_text(&e)),
            },
        }
    }

    fn run(&mut self) -> Result<(), Failure> {
        let raw = match &self.opts.url {
            Some(u) if !u.is_empty() => u.clone(),
            _ => return fail(c::CURLE_URL_MALFORMAT, "No URL set"),
        };
        let default = self.opts.default_protocol.as_deref().unwrap_or("http");
        let url = match Url::parse(&raw, default) {
            Ok(u) => u,
            Err(UrlError::Empty) => return fail(c::CURLE_URL_MALFORMAT, "No URL set"),
            Err(UrlError::Malformed(m)) => {
                return fail(c::CURLE_URL_MALFORMAT, format!("URL rejected: {m}"));
            }
        };
        self.check_scheme(&url.scheme, self.opts.protocols)?;
        self.info.effective_url = Some(url.to_bytes());
        self.info.scheme = Some(url.scheme.clone());
        self.info.protocol = proto_bit(&url.scheme);
        self.info.queue_us = self.us();
        self.progress(0, 0, 0, 0)?;
        if url.scheme == "file" {
            return self.file(&url);
        }
        self.http(url)
    }

    fn check_scheme(&self, scheme: &str, allowed: i64) -> Result<(), Failure> {
        let bit = proto_bit(scheme);
        if bit == 0 || bit & SUPPORTED == 0 {
            return fail(
                c::CURLE_UNSUPPORTED_PROTOCOL,
                format!("Protocol \"{scheme}\" not supported"),
            );
        }
        if allowed & bit == 0 {
            return fail(
                c::CURLE_UNSUPPORTED_PROTOCOL,
                format!("Protocol \"{scheme}\" disabled"),
            );
        }
        Ok(())
    }

    // ------------------------------------------------------------ file://

    fn file(&mut self, url: &Url) -> Result<(), Failure> {
        let path = url::unescape(&url.path);
        let shown = String::from_utf8_lossy(&path).into_owned();
        let os = std::path::Path::new(
            <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(&path),
        );
        self.info.conn_id = self.pool.take();
        self.info.namelookup_us = self.us();
        self.info.pretransfer_us = self.us();
        if self.opts.upload {
            use std::io::Write;
            let mut oo = std::fs::OpenOptions::new();
            oo.write(true).create(true);
            if self.opts.append {
                oo.append(true);
            } else {
                oo.truncate(true);
            }
            let mut f = match oo.open(os) {
                Ok(f) => f,
                Err(_) => {
                    return fail(
                        c::CURLE_WRITE_ERROR,
                        format!("Can't open {shown} for writing"),
                    );
                }
            };
            self.info.upload_content_length = self.opts.infilesize;
            let mut total = 0i64;
            loop {
                let max = 65536;
                let r = self.host.read(max);
                self.check_stop()?;
                let data = match r {
                    ReadResult::Data(d) => d,
                    ReadResult::Pause => continue,
                    ReadResult::Abort => {
                        return fail(
                            c::CURLE_ABORTED_BY_CALLBACK,
                            "operation aborted by callback",
                        );
                    }
                    ReadResult::Funny => {
                        return fail(c::CURLE_READ_ERROR, "read function returned funny value");
                    }
                };
                if data.is_empty() {
                    break;
                }
                if f.write_all(&data).is_err() {
                    return fail(c::CURLE_WRITE_ERROR, "Failed writing to file");
                }
                total += data.len() as i64;
                self.info.size_upload = total;
                self.progress(0, 0, self.opts.infilesize.max(0), total)?;
            }
            self.info.posttransfer_us = self.us();
            return Ok(());
        }
        let meta = match std::fs::metadata(os) {
            Ok(m) => m,
            Err(_) => {
                return fail(
                    c::CURLE_FILE_COULDNT_READ_FILE,
                    format!("Could not open file {shown}"),
                );
            }
        };
        let mtime = {
            use std::os::unix::fs::MetadataExt;
            meta.mtime()
        };
        let data = if meta.is_dir() {
            let mut out = Vec::new();
            if let Ok(rd) = std::fs::read_dir(os) {
                for e in rd.flatten() {
                    use std::os::unix::ffi::OsStrExt;
                    out.extend_from_slice(e.file_name().as_bytes());
                    out.push(b'\n');
                }
            }
            out
        } else {
            match std::fs::read(os) {
                Ok(d) => d,
                Err(_) => {
                    return fail(
                        c::CURLE_FILE_COULDNT_READ_FILE,
                        format!("Could not open file {shown}"),
                    );
                }
            }
        };
        let size = data.len() as i64;
        let (from, to) = self.file_range(size)?;
        self.info.filetime = mtime;
        self.info.download_content_length = (to - from) as i64;
        self.info.starttransfer_us = self.us();
        if !meta.is_dir() {
            let lines = [
                format!("Content-Length: {}\r\n", to - from),
                "Accept-ranges: bytes\r\n".to_string(),
                format!("Last-Modified: {}\r\n", http_date(mtime)),
                "\r\n".to_string(),
            ];
            for l in lines {
                self.deliver_header(l.as_bytes())?;
            }
        }
        if !self.opts.nobody {
            self.expected = Some((to - from) as i64);
            let body = data[from..to].to_vec();
            self.deliver_all(&body)?;
            self.info.size_download = body.len() as i64;
        }
        self.progress(
            self.info.download_content_length,
            self.info.size_download,
            0,
            0,
        )?;
        Ok(())
    }

    /// The byte range of a file:// download (CURLOPT_RANGE "a-b", "a-",
    /// "-n", or CURLOPT_RESUME_FROM).
    fn file_range(&self, size: i64) -> Result<(usize, usize), Failure> {
        let mut from = 0i64;
        let mut to = size;
        if let Some(r) = &self.opts.range {
            let s = String::from_utf8_lossy(r).into_owned();
            let first = s.split(',').next().unwrap_or("").trim().to_string();
            if let Some(n) = first.strip_prefix('-') {
                let n: i64 = n.trim().parse().unwrap_or(0);
                from = (size - n).max(0);
            } else if let Some((a, b)) = first.split_once('-') {
                from = a.trim().parse().unwrap_or(0);
                if !b.trim().is_empty() {
                    to = (b.trim().parse::<i64>().unwrap_or(size - 1) + 1).min(size);
                }
            }
        } else if self.opts.resume_from > 0 {
            from = self.opts.resume_from;
        }
        if from > size {
            return fail(
                c::CURLE_BAD_DOWNLOAD_RESUME,
                "failed to resume file:// transfer",
            );
        }
        Ok((from as usize, to.max(from) as usize))
    }

    // ------------------------------------------------------------ output

    fn write_checked(&mut self, data: &[u8]) -> Result<(), Failure> {
        let n = self.host.write(data);
        self.check_stop()?;
        if n != data.len() {
            return fail(
                c::CURLE_WRITE_ERROR,
                format!(
                    "Failure writing output to destination, passed {} returned {}",
                    data.len(),
                    n
                ),
            );
        }
        Ok(())
    }

    fn deliver_header(&mut self, line: &[u8]) -> Result<(), Failure> {
        let n = self.host.header(line);
        self.check_stop()?;
        if n != line.len() {
            return fail(
                c::CURLE_WRITE_ERROR,
                format!(
                    "Failure writing output to destination, passed {} returned {}",
                    line.len(),
                    n
                ),
            );
        }
        if self.opts.header {
            self.write_checked(line)?;
        }
        Ok(())
    }

    fn chunk_size(&self) -> usize {
        self.opts.buffersize.clamp(1024, 10 << 20) as usize
    }

    fn deliver_all(&mut self, data: &[u8]) -> Result<(), Failure> {
        let n = self.chunk_size();
        for ch in data.chunks(n) {
            self.write_checked(ch)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ HTTP

    fn credentials(&self, url: &Url) -> Option<(Vec<u8>, Vec<u8>)> {
        let mut user = None;
        let mut pass = None;
        if let Some(up) = &self.opts.userpwd {
            match up.iter().position(|&c| c == b':') {
                Some(p) => {
                    user = Some(up[..p].to_vec());
                    pass = Some(up[p + 1..].to_vec());
                }
                None => user = Some(up.clone()),
            }
        }
        if let Some(u) = &self.opts.username {
            user = Some(u.clone());
        }
        if let Some(p) = &self.opts.password {
            pass = Some(p.clone());
        }
        if user.is_none() {
            user = url.user.clone();
            if pass.is_none() {
                pass = url.password.clone();
            }
        }
        user.map(|u| (u, pass.unwrap_or_default()))
    }

    fn proxy_for(&self, url: &Url) -> Result<Option<Proxy>, Failure> {
        let Some(p) = &self.opts.proxy else {
            return Ok(None);
        };
        if p.is_empty() {
            return Ok(None);
        }
        if let Some(np) = &self.opts.noproxy {
            let np = String::from_utf8_lossy(np).to_ascii_lowercase();
            let h = url.host.to_ascii_lowercase();
            for e in np.split(',').map(|s| s.trim().trim_start_matches('.')) {
                if e == "*" || (!e.is_empty() && (h == e || h.ends_with(&format!(".{e}")))) {
                    return Ok(None);
                }
            }
        }
        let shown = String::from_utf8_lossy(p).into_owned();
        let (scheme, rest) = match p.windows(3).position(|w| w == b"://") {
            Some(i) => (
                String::from_utf8_lossy(&p[..i]).to_ascii_lowercase(),
                &p[i + 3..],
            ),
            None => (String::new(), &p[..]),
        };
        let kind = match scheme.as_str() {
            "" => self.opts.proxytype,
            "http" => c::CURLPROXY_HTTP,
            "https" => c::CURLPROXY_HTTPS,
            "socks4" => c::CURLPROXY_SOCKS4,
            "socks4a" => c::CURLPROXY_SOCKS4A,
            "socks5" => c::CURLPROXY_SOCKS5,
            "socks5h" => c::CURLPROXY_SOCKS5_HOSTNAME,
            _ => {
                return fail(
                    c::CURLE_COULDNT_RESOLVE_PROXY,
                    format!("Unsupported proxy scheme for '{shown}'"),
                );
            }
        };
        let rest = match rest.iter().position(|&c| c == b'/') {
            Some(i) => &rest[..i],
            None => rest,
        };
        let (userinfo, hp) = match rest.iter().rposition(|&c| c == b'@') {
            Some(i) => (Some(&rest[..i]), &rest[i + 1..]),
            None => (None, rest),
        };
        let (host, port) = if let Some(r) = hp.strip_prefix(b"[") {
            let end = r.iter().position(|&c| c == b']').unwrap_or(r.len());
            let after = r.get(end + 1..).unwrap_or(b"");
            (
                String::from_utf8_lossy(&r[..end]).into_owned(),
                after.strip_prefix(b":"),
            )
        } else {
            match hp.iter().rposition(|&c| c == b':') {
                Some(i) => (
                    String::from_utf8_lossy(&hp[..i]).into_owned(),
                    Some(&hp[i + 1..]),
                ),
                None => (String::from_utf8_lossy(hp).into_owned(), None),
            }
        };
        let port = match port {
            Some(pp) if !pp.is_empty() => {
                let ok = pp.iter().all(|c| c.is_ascii_digit()) && pp.len() <= 5;
                let n: u32 = if ok {
                    String::from_utf8_lossy(pp).parse().unwrap_or(70000)
                } else {
                    70000
                };
                if n > 65535 {
                    return fail(
                        c::CURLE_COULDNT_RESOLVE_PROXY,
                        format!(
                            "Unsupported proxy syntax in '{shown}': Port number was not a decimal number between 0 and 65535"
                        ),
                    );
                }
                n as u16
            }
            _ => {
                if self.opts.proxyport > 0 {
                    self.opts.proxyport as u16
                } else if kind == c::CURLPROXY_HTTPS {
                    443
                } else {
                    1080
                }
            }
        };
        if host.is_empty() {
            return fail(
                c::CURLE_COULDNT_RESOLVE_PROXY,
                format!("Unsupported proxy syntax in '{shown}'"),
            );
        }
        let (mut user, mut pass) = match userinfo {
            Some(u) => match u.iter().position(|&c| c == b':') {
                Some(i) => (
                    Some(url::unescape(&u[..i])),
                    Some(url::unescape(&u[i + 1..])),
                ),
                None => (Some(url::unescape(u)), None),
            },
            None => (None, None),
        };
        if let Some(up) = &self.opts.proxyuserpwd {
            match up.iter().position(|&c| c == b':') {
                Some(i) => {
                    user = Some(up[..i].to_vec());
                    pass = Some(up[i + 1..].to_vec());
                }
                None => user = Some(up.clone()),
            }
        }
        if let Some(u) = &self.opts.proxyusername {
            user = Some(u.clone());
        }
        if let Some(p) = &self.opts.proxypassword {
            pass = Some(p.clone());
        }
        Ok(Some(Proxy {
            kind,
            host,
            port,
            user,
            pass,
        }))
    }

    /// Name resolution: CURLOPT_RESOLVE entries, IP literals, getaddrinfo.
    fn resolve(&mut self, host: &str, port: u16, proxy: bool) -> Result<Vec<SocketAddr>, Failure> {
        let key = format!("{}:{}", host.to_ascii_lowercase(), port);
        for e in &self.opts.resolve {
            let s = String::from_utf8_lossy(e).into_owned();
            let s = s.trim_start_matches('+');
            if s.starts_with('-') {
                continue;
            }
            let mut it = s.splitn(3, ':');
            let (Some(h), Some(p), Some(addrs)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            if format!("{}:{}", h.to_ascii_lowercase(), p) != key
                && !(h == "*" && p == port.to_string())
            {
                continue;
            }
            let v: Vec<SocketAddr> = addrs
                .split(',')
                .filter_map(|a| {
                    a.trim()
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .parse::<IpAddr>()
                        .ok()
                })
                .map(|ip| SocketAddr::new(ip, port))
                .collect();
            if !v.is_empty() {
                return Ok(v);
            }
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }
        let r = (host, port).to_socket_addrs();
        let mut v: Vec<SocketAddr> = match r {
            Ok(it) => it.collect(),
            Err(_) => Vec::new(),
        };
        let mut seen = Vec::new();
        v.retain(|a| {
            if seen.contains(a) {
                false
            } else {
                seen.push(*a);
                true
            }
        });
        match self.opts.ipresolve {
            1 => v.retain(|a| a.is_ipv4()),
            2 => v.retain(|a| a.is_ipv6()),
            _ => {}
        }
        if v.is_empty() {
            return if proxy {
                fail(
                    c::CURLE_COULDNT_RESOLVE_PROXY,
                    format!("Could not resolve proxy: {host}"),
                )
            } else {
                fail(
                    c::CURLE_COULDNT_RESOLVE_HOST,
                    format!("Could not resolve host: {host}"),
                )
            };
        }
        // IPv6 first, as libcurl's happy eyeballs tries it first.
        v.sort_by_key(|a| a.is_ipv4());
        if self.opts.verbose {
            let six: Vec<String> = v
                .iter()
                .filter(|a| a.is_ipv6())
                .map(|a| a.ip().to_string())
                .collect();
            let four: Vec<String> = v
                .iter()
                .filter(|a| a.is_ipv4())
                .map(|a| a.ip().to_string())
                .collect();
            self.text(&format!("Host {host}:{port} was resolved."));
            self.text(&format!(
                "IPv6: {}",
                if six.is_empty() {
                    "(none)".to_string()
                } else {
                    six.join(", ")
                }
            ));
            self.text(&format!(
                "IPv4: {}",
                if four.is_empty() {
                    "(none)".to_string()
                } else {
                    four.join(", ")
                }
            ));
        }
        Ok(v)
    }

    fn connect_deadline(&self) -> Instant {
        let ct = if self.opts.connect_timeout_ms > 0 {
            self.opts.connect_timeout_ms as u64
        } else {
            300_000
        };
        let d = self.start + Duration::from_millis(ct);
        match self.deadline {
            Some(o) if o < d => o,
            _ => d,
        }
    }

    /// Opens a TCP (or Unix socket) connection to `host:port`.
    fn open(&mut self, host: &str, port: u16, proxy: bool) -> Result<Conn, Failure> {
        // The connection is numbered before it is made (observed on PHP
        // 8.5: a refused connection reports CURLINFO_CONN_ID 0).
        let id = self.pool.take();
        self.info.conn_id = id;
        if let Some(p) = &self.opts.unix_socket_path {
            let path = std::path::Path::new(
                <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(p),
            );
            let shown = String::from_utf8_lossy(p).into_owned();
            self.text(&format!("  Trying {shown}:{port}..."));
            self.info.namelookup_us = self.us();
            return match std::os::unix::net::UnixStream::connect(path) {
                Ok(s) => {
                    self.info.connect_us = self.us();
                    self.info.num_connects += 1;
                    self.text(&format!("Connected to {host} ({shown}) port {port}"));
                    Ok(Conn::new(Sock::Unix(s), id))
                }
                Err(e) => fail(
                    c::CURLE_COULDNT_CONNECT,
                    format!(
                        "Failed to connect to {host} port {port} after {} ms: {}",
                        self.start.elapsed().as_millis(),
                        io_text(&e)
                    ),
                ),
            };
        }
        let addrs = self.resolve(host, port, proxy)?;
        self.info.namelookup_us = self.us();
        let deadline = self.connect_deadline();
        let verbose = self.opts.verbose;
        let mut events: Vec<String> = Vec::new();
        let r = net::connect(&addrs, deadline, &mut |a, err| {
            if !verbose {
                return;
            }
            let shown = match a {
                SocketAddr::V6(v) => format!("[{}]:{}", v.ip(), v.port()),
                SocketAddr::V4(v) => format!("{}:{}", v.ip(), v.port()),
            };
            match err {
                None => events.push(format!("  Trying {shown}...")),
                Some(e) => events.push(format!(
                    "connect to {} port {} failed: {}",
                    a.ip(),
                    a.port(),
                    io_text(e)
                )),
            }
        });
        for e in events {
            self.text(&e);
        }
        match r {
            Ok(s) => {
                self.info.connect_us = self.us();
                self.info.num_connects += 1;
                let conn = Conn::new(Sock::Tcp(s), id);
                if let (Some(rem), Some(loc)) = (conn.remote, conn.local) {
                    self.info.primary_ip = rem.ip().to_string();
                    self.info.primary_port = rem.port() as i64;
                    self.info.local_ip = loc.ip().to_string();
                    self.info.local_port = loc.port() as i64;
                    self.text(&format!(
                        "Established connection to {} ({} port {}) from {} port {} ",
                        host,
                        rem.ip(),
                        rem.port(),
                        loc.ip(),
                        loc.port()
                    ));
                }
                Ok(conn)
            }
            Err((timed_out, e)) => {
                if let Some(e) = &e {
                    self.info.os_errno = e.raw_os_error().unwrap_or(0) as i64;
                }
                let ms = self.start.elapsed().as_millis();
                if timed_out {
                    fail(
                        c::CURLE_OPERATION_TIMEDOUT,
                        format!("Connection timed out after {ms} milliseconds"),
                    )
                } else {
                    fail(
                        c::CURLE_COULDNT_CONNECT,
                        format!(
                            "Failed to connect to {host} port {port} after {ms} ms: Could not connect to server"
                        ),
                    )
                }
            }
        }
    }

    /// SOCKS4/4a/5 negotiation (RFC 1928, RFC 1929; the SOCKS 4 and 4A
    /// protocol descriptions).
    fn socks(&mut self, conn: &mut Conn, p: &Proxy, host: &str, port: u16) -> Result<(), Failure> {
        let perr = |m: &str| Failure {
            code: c::CURLE_PROXY,
            message: m.to_string(),
        };
        let read_n = |conn: &mut Conn, n: usize| -> Result<Vec<u8>, Failure> {
            let mut out = vec![0u8; n];
            let mut got = 0;
            while got < n {
                match conn.read(&mut out[got..]) {
                    Ok(0) | Err(_) => return Err(perr("Unable to receive SOCKS response")),
                    Ok(k) => got += k,
                }
            }
            Ok(out)
        };
        if p.kind == c::CURLPROXY_SOCKS4 || p.kind == c::CURLPROXY_SOCKS4A {
            let mut req = vec![4u8, 1, (port >> 8) as u8, port as u8];
            let local_resolve = p.kind == c::CURLPROXY_SOCKS4;
            let mut name = None;
            if local_resolve {
                let a = self
                    .resolve(host, port, false)?
                    .into_iter()
                    .find(|a| a.is_ipv4())
                    .ok_or_else(|| perr("SOCKS4 connection to IPv6 address not supported"))?;
                let IpAddr::V4(v4) = a.ip() else {
                    unreachable!()
                };
                req.extend_from_slice(&v4.octets());
            } else {
                req.extend_from_slice(&[0, 0, 0, 1]);
                name = Some(host.as_bytes().to_vec());
            }
            req.extend_from_slice(p.user.as_deref().unwrap_or(b""));
            req.push(0);
            if let Some(n) = name {
                req.extend_from_slice(&n);
                req.push(0);
            }
            conn.write_all(&req).map_err(|e| self.send_failure(e))?;
            let r = read_n(conn, 8)?;
            if r[1] != 90 {
                return Err(perr(&format!(
                    "SOCKS4 connection to {host} rejected: request rejected or failed"
                )));
            }
            return Ok(());
        }
        let auth = p.user.is_some();
        let greet: &[u8] = if auth { &[5, 2, 0, 2] } else { &[5, 1, 0] };
        conn.write_all(greet).map_err(|e| self.send_failure(e))?;
        let r = read_n(conn, 2)?;
        if r[0] != 5 {
            return Err(perr("Received invalid version in initial SOCKS5 response."));
        }
        match r[1] {
            0 => {}
            2 if auth => {
                let u = p.user.clone().unwrap_or_default();
                let pw = p.pass.clone().unwrap_or_default();
                let mut m = vec![1u8, u.len() as u8];
                m.extend_from_slice(&u);
                m.push(pw.len() as u8);
                m.extend_from_slice(&pw);
                conn.write_all(&m).map_err(|e| self.send_failure(e))?;
                let r = read_n(conn, 2)?;
                if r[1] != 0 {
                    return Err(perr("User was rejected by the SOCKS5 server"));
                }
            }
            _ => return Err(perr("No authentication method was acceptable.")),
        }
        let mut req = vec![5u8, 1, 0];
        if p.kind == c::CURLPROXY_SOCKS5_HOSTNAME && host.parse::<IpAddr>().is_err() {
            req.push(3);
            req.push(host.len() as u8);
            req.extend_from_slice(host.as_bytes());
        } else {
            let a = self.resolve(host, port, false)?[0];
            match a.ip() {
                IpAddr::V4(v) => {
                    req.push(1);
                    req.extend_from_slice(&v.octets());
                }
                IpAddr::V6(v) => {
                    req.push(4);
                    req.extend_from_slice(&v.octets());
                }
            }
        }
        req.push((port >> 8) as u8);
        req.push(port as u8);
        conn.write_all(&req).map_err(|e| self.send_failure(e))?;
        let r = read_n(conn, 4)?;
        if r[1] != 0 {
            return Err(perr(&format!(
                "Can't complete SOCKS5 connection to {host}. ({})",
                r[1]
            )));
        }
        let rest = match r[3] {
            1 => 4 + 2,
            4 => 16 + 2,
            3 => read_n(conn, 1)?[0] as usize + 2,
            _ => 2,
        };
        read_n(conn, rest)?;
        Ok(())
    }

    /// A CONNECT tunnel through an HTTP proxy (RFC 9110 section 9.3.6).
    fn tunnel(&mut self, conn: &mut Conn, p: &Proxy, url: &Url) -> Result<(), Failure> {
        let target = format!("{}:{}", url.host_display(), url.port_or_default());
        let mut req = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n").into_bytes();
        if let Some(u) = &p.user {
            let mut cred = u.clone();
            cred.push(b':');
            cred.extend_from_slice(p.pass.as_deref().unwrap_or(b""));
            req.extend_from_slice(b"Proxy-Authorization: Basic ");
            req.extend_from_slice(&base64(&cred));
            req.extend_from_slice(b"\r\n");
            self.info.proxyauth_used = c::CURLAUTH_BASIC;
        }
        if let Some(ua) = &self.opts.useragent {
            req.extend_from_slice(b"User-Agent: ");
            req.extend_from_slice(ua);
            req.extend_from_slice(b"\r\n");
        }
        for h in user_headers(&self.opts.proxy_headers).0 {
            req.extend_from_slice(&h);
            req.extend_from_slice(b"\r\n");
        }
        req.extend_from_slice(b"\r\n");
        if self.opts.verbose {
            self.host.debug(HEADER_OUT, &req);
        }
        conn.write_all(&req).map_err(|e| self.send_failure(e))?;
        let head = self.read_head(conn)?;
        self.info.http_connectcode = head.code;
        if self.opts.verbose {
            for l in &head.lines {
                self.host.debug(HEADER_IN, l);
            }
        }
        if !(200..300).contains(&head.code) {
            return fail(
                c::CURLE_COULDNT_CONNECT,
                format!("CONNECT tunnel failed, response {}", head.code),
            );
        }
        // Proxy response headers reach the application unless suppressed.
        for l in head.lines.clone() {
            self.deliver_header(&l)?;
        }
        Ok(())
    }

    fn load_file(&self, path: &[u8]) -> Option<Vec<u8>> {
        std::fs::read(std::path::Path::new(
            <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(path),
        ))
        .ok()
    }

    /// TLS on a connection, with CURLOPT_SSL_VERIFYPEER/VERIFYHOST,
    /// client certificates and CURLOPT_ISSUERCERT.
    fn start_tls(&mut self, conn: &mut Conn, host: &str) -> Result<(), Failure> {
        use goro_ext_openssl::tls::{ClientConfig, Identity, Session, Versions, proto};
        let versions = {
            let all = proto::TLS1_0 | proto::TLS1_1 | proto::TLS1_2 | proto::TLS1_3;
            let mask = match self.opts.sslversion & 0xffff {
                4 => proto::TLS1_0 | proto::TLS1_1 | proto::TLS1_2 | proto::TLS1_3,
                5 => proto::TLS1_1 | proto::TLS1_2 | proto::TLS1_3,
                6 => proto::TLS1_2 | proto::TLS1_3,
                7 => proto::TLS1_3,
                _ => proto::TLS1_2 | proto::TLS1_3,
            };
            Versions::from_mask(mask & all).expect("versions")
        };
        let cert = match (&self.opts.sslcert_blob, &self.opts.sslcert) {
            (Some(b), _) => Some((b.clone(), "(memory blob)".to_string())),
            (None, Some(p)) => match self.load_file(p) {
                Some(d) => Some((d, String::from_utf8_lossy(p).into_owned())),
                None => {
                    return fail(
                        c::CURLE_SSL_CERTPROBLEM,
                        format!(
                            "could not load PEM client certificate from {}",
                            String::from_utf8_lossy(p)
                        ),
                    );
                }
            },
            _ => None,
        };
        let key = match (&self.opts.sslkey_blob, &self.opts.sslkey) {
            (Some(b), _) => {
                if b.is_empty() {
                    return fail(
                        c::CURLE_BAD_FUNCTION_ARGUMENT,
                        "A libcurl function was given a bad argument",
                    );
                }
                Some(b.clone())
            }
            (None, Some(p)) => match self.load_file(p) {
                Some(d) => Some(d),
                None => {
                    return fail(
                        c::CURLE_SSL_CERTPROBLEM,
                        format!(
                            "unable to set private key file: '{}' type PEM",
                            String::from_utf8_lossy(p)
                        ),
                    );
                }
            },
            _ => None,
        };
        let identity = match cert {
            Some((data, shown)) => {
                match Identity::from_pem(&data, key.as_deref(), self.opts.keypasswd.as_deref()) {
                    Ok(i) => Some(i),
                    Err(_) => {
                        return fail(
                            c::CURLE_SSL_CERTPROBLEM,
                            format!("could not load PEM client certificate from {shown}"),
                        );
                    }
                }
            }
            None => None,
        };
        let sni = if host.parse::<IpAddr>().is_err() {
            Some(host.to_string())
        } else {
            None
        };
        let session = match Session::client(ClientConfig {
            sni,
            versions,
            alpn: vec![b"http/1.1".to_vec()],
            identity,
        }) {
            Ok(s) => s,
            Err(e) => {
                return fail(
                    c::CURLE_SSL_CONNECT_ERROR,
                    format!("TLS connect error: {e}"),
                );
            }
        };
        if let Err(e) = conn.handshake(session) {
            if e == "timeout" {
                return Err(self.timeout_failure());
            }
            return fail(
                c::CURLE_SSL_CONNECT_ERROR,
                format!("TLS connect error: {e}"),
            );
        }
        self.info.appconnect_us = self.us();
        self.text("SSL connection using TLS");
        self.verify_peer(conn, host)
    }

    fn verify_peer(&mut self, conn: &mut Conn, host: &str) -> Result<(), Failure> {
        use goro_ext_openssl::verify::verify_chain;
        use goro_ext_openssl::x509::{Cert, GeneralName, certs_from_bytes};
        let Some(tls) = &conn.tls else { return Ok(()) };
        let chain: Vec<Cert> = tls
            .peer_certificates()
            .iter()
            .filter_map(|d| Cert::from_der(d).ok())
            .collect();
        let Some(leaf) = chain.first().cloned() else {
            return fail(E_PEER_FAILED_VERIFICATION, "SSL: no peer certificate");
        };
        if let Some(b) = self.opts.issuercert_blob.clone().or_else(|| {
            self.opts
                .issuercert
                .as_ref()
                .and_then(|p| self.load_file(p))
        }) {
            let issuers = certs_from_bytes(&b);
            let ok = issuers
                .iter()
                .any(|i| leaf.issuer.matches(&i.subject) && leaf.verify_signature_by(i));
            if !ok {
                return fail(E_SSL_ISSUER_ERROR, "SSL: Certificate issuer check failed");
            }
        }
        if self.opts.ssl_verifypeer {
            let mut trusted = Vec::new();
            if let Some(b) = &self.opts.cainfo_blob {
                trusted.extend(certs_from_bytes(b));
            }
            if let Some(p) = &self.opts.cainfo {
                match self.load_file(p) {
                    Some(d) => trusted.extend(certs_from_bytes(&d)),
                    None => {
                        return fail(
                            c::CURLE_SSL_CACERT_BADFILE,
                            format!(
                                "error setting certificate file: {}",
                                String::from_utf8_lossy(p)
                            ),
                        );
                    }
                }
            }
            if let Some(dir) = &self.opts.capath {
                let d = std::path::Path::new(
                    <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(dir),
                );
                if let Ok(rd) = std::fs::read_dir(d) {
                    for e in rd.flatten() {
                        if let Ok(data) = std::fs::read(e.path()) {
                            trusted.extend(certs_from_bytes(&data));
                        }
                    }
                }
            }
            let now = crate::cookie::now();
            if let Err(e) = verify_chain(&leaf, &trusted, &chain[1..], None, now) {
                self.info.ssl_verify_result = e as i64;
                return fail(
                    E_PEER_FAILED_VERIFICATION,
                    format!("SSL certificate problem: {}", e.text()),
                );
            }
        }
        if self.opts.ssl_verifyhost >= 2 {
            let h = host.to_ascii_lowercase();
            let names = leaf.alt_names();
            let ip = h.parse::<IpAddr>().ok();
            let mut has_dns = false;
            let mut ok = false;
            for n in &names {
                match n {
                    GeneralName::Dns(d) => {
                        has_dns = true;
                        if wildcard_match(&String::from_utf8_lossy(d).to_ascii_lowercase(), &h) {
                            ok = true;
                        }
                    }
                    GeneralName::Ip(b) => {
                        let mine = match ip {
                            Some(IpAddr::V4(v)) => v.octets().to_vec(),
                            Some(IpAddr::V6(v)) => v.octets().to_vec(),
                            None => Vec::new(),
                        };
                        if !mine.is_empty() && *b == mine {
                            ok = true;
                        }
                    }
                    _ => {}
                }
            }
            if !ok && !has_dns {
                for a in leaf.subject.find("2.5.4.3") {
                    if wildcard_match(&String::from_utf8_lossy(&a.value).to_ascii_lowercase(), &h) {
                        ok = true;
                    }
                }
            }
            if !ok {
                return fail(
                    E_PEER_FAILED_VERIFICATION,
                    format!(
                        "SSL: no alternative certificate subject name matches target host name '{host}'"
                    ),
                );
            }
        }
        Ok(())
    }

    fn read_head(&mut self, conn: &mut Conn) -> Result<Head, Failure> {
        let first = match conn.read_line() {
            Ok(Some(l)) => l,
            Ok(None) => return fail(c::CURLE_GOT_NOTHING, "Empty reply from server"),
            Err(e) => return Err(self.recv_failure(e)),
        };
        if self.info.starttransfer_us == 0 {
            self.info.starttransfer_us = self.us();
        }
        let t = trim(&first);
        if !t.starts_with(b"HTTP/") {
            if !self.opts.http09_allowed {
                return fail(
                    c::CURLE_UNSUPPORTED_PROTOCOL,
                    "Received HTTP/0.9 when not allowed",
                );
            }
            return fail(c::CURLE_WEIRD_SERVER_REPLY, "Unsupported HTTP/0.9 response");
        }
        let mut parts = t.splitn(3, |&c| c == b' ');
        let ver = parts.next().unwrap_or(b"");
        let code_s = parts.next().unwrap_or(b"");
        let version = match ver {
            b"HTTP/1.0" => 1,
            b"HTTP/1.1" => 2,
            b"HTTP/2" | b"HTTP/2.0" => 3,
            b"HTTP/3" => 30,
            _ => 2,
        };
        let code: i64 = match std::str::from_utf8(code_s)
            .ok()
            .and_then(|s| s.parse().ok())
        {
            Some(n) if (100..1000).contains(&n) && code_s.len() == 3 => n,
            _ => {
                return fail(
                    c::CURLE_WEIRD_SERVER_REPLY,
                    "Unsupported response code in HTTP response",
                );
            }
        };
        let mut lines = vec![first.clone()];
        let mut headers: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        loop {
            let l = match conn.read_line() {
                Ok(Some(l)) => l,
                Ok(None) => break,
                Err(e) => return Err(self.recv_failure(e)),
            };
            let blank = l == b"\r\n" || l == b"\n";
            lines.push(l.clone());
            if blank {
                break;
            }
            if (l[0] == b' ' || l[0] == b'\t') && !headers.is_empty() {
                let last = headers.last_mut().expect("header");
                last.1.push(b' ');
                last.1.extend_from_slice(trim(&l));
                continue;
            }
            if let Some(p) = l.iter().position(|&c| c == b':') {
                headers.push((trim(&l[..p]).to_vec(), trim(&l[p + 1..]).to_vec()));
            }
        }
        Ok(Head {
            version,
            code,
            lines,
            headers,
        })
    }

    /// Passes a response head to the application and records it.
    fn take_head(&mut self, head: &Head, url: &Url) -> Result<(), Failure> {
        self.info.response_code = head.code;
        self.info.http_version = head.version;
        for l in &head.lines {
            self.info.header_size += l.len() as i64;
        }
        if self.opts.verbose {
            for l in &head.lines {
                self.host.debug(HEADER_IN, l);
            }
        }
        if self.jar.enabled {
            let path = url.path.clone();
            for v in head.all("set-cookie") {
                self.jar.add_set_cookie(v, &url.host, &path);
            }
        }
        if let Some(ct) = head.get("content-type") {
            self.info.content_type = Some(ct.to_vec());
        }
        if head.code == 401 {
            for v in head.all("www-authenticate") {
                let scheme = String::from_utf8_lossy(v)
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                self.info.httpauth_avail |= match scheme.as_str() {
                    "basic" => c::CURLAUTH_BASIC,
                    "digest" => c::CURLAUTH_DIGEST,
                    "negotiate" => c::CURLAUTH_NEGOTIATE,
                    "ntlm" => c::CURLAUTH_NTLM,
                    "bearer" => c::CURLAUTH_BEARER,
                    _ => 0,
                };
            }
        }
        if let Some(ra) = head.get("retry-after") {
            self.info.retry_after = String::from_utf8_lossy(ra).trim().parse().unwrap_or(0);
        }
        for l in &head.lines {
            self.deliver_header(l)?;
        }
        Ok(())
    }

    fn http(&mut self, mut url: Url) -> Result<(), Failure> {
        let o = self.opts;
        let mut method: Vec<u8>;
        let mut plan = self.initial_plan(&url);
        let mut custom = o.custom_request.clone();
        let mut referer = o.referer.clone();
        let origin = (
            url.scheme.clone(),
            url.host.to_ascii_lowercase(),
            url.port_or_default(),
        );
        let mut scheme: Option<Challenge> = if o.httpauth == c::CURLAUTH_BASIC {
            Some(Challenge::Basic)
        } else {
            None
        };
        let mut auth_retried = false;
        let mut redirects = 0i64;
        loop {
            method = match &custom {
                Some(m) => m.clone(),
                None => {
                    if o.nobody {
                        b"HEAD".to_vec()
                    } else if o.upload && !matches!(plan, Plan::None) {
                        b"PUT".to_vec()
                    } else if !matches!(plan, Plan::None) {
                        b"POST".to_vec()
                    } else {
                        b"GET".to_vec()
                    }
                }
            };
            self.info.effective_method = method.clone();
            self.info.effective_url = Some(url.to_bytes());
            self.info.scheme = Some(url.scheme.clone());
            self.info.protocol = proto_bit(&url.scheme);
            let same_origin = (
                url.scheme.clone(),
                url.host.to_ascii_lowercase(),
                url.port_or_default(),
            ) == origin;
            let creds = if same_origin || o.unrestricted_auth {
                self.credentials(&url)
            } else {
                None
            };
            let send_auth = scheme.is_some();
            let auth = match (&mut scheme, &creds) {
                (Some(Challenge::Basic), Some((u, p))) => {
                    let mut s = u.clone();
                    s.push(b':');
                    s.extend_from_slice(p);
                    let mut v = b"Basic ".to_vec();
                    v.extend_from_slice(&base64(&s));
                    Some(v)
                }
                (Some(Challenge::Digest(d)), Some((u, p))) => {
                    let cn = {
                        let host = &mut *self.host;
                        format!("{:016x}{:016x}", host.random(), host.random())
                    };
                    Some(d.header(u, p, &method, &url.target(), cn.as_bytes()))
                }
                _ => None,
            };
            let out = self.exchange(
                &url,
                &method,
                &plan,
                referer.as_deref(),
                auth.as_deref(),
                &mut |me, head| {
                    // Whether the body of this response is skipped: followed
                    // redirects and authentication retries.
                    me.after_head(
                        head,
                        creds.is_some() && !send_auth && !auth_retried,
                        redirects,
                    )
                },
            )?;
            match out {
                Next::Done => return Ok(()),
                Next::Auth(ch) => {
                    // Reported for a negotiated scheme only (observed on PHP
                    // 8.5: 0 for Basic sent upfront).
                    self.info.httpauth_used = match ch {
                        Challenge::Basic => c::CURLAUTH_BASIC,
                        Challenge::Digest(_) => c::CURLAUTH_DIGEST,
                    };
                    auth_retried = true;
                    scheme = Some(ch);
                    continue;
                }
                Next::Redirect(code, loc) => {
                    redirects += 1;
                    if o.maxredirs >= 0 && redirects > o.maxredirs {
                        return fail(
                            c::CURLE_TOO_MANY_REDIRECTS,
                            format!("Maximum ({}) redirects followed", o.maxredirs),
                        );
                    }
                    let prev = url.to_bytes();
                    let new = url.resolve(&loc);
                    let nu = match Url::parse(&new, "http") {
                        Ok(u) => u,
                        Err(UrlError::Empty) => return fail(c::CURLE_URL_MALFORMAT, "No URL set"),
                        Err(UrlError::Malformed(m)) => {
                            return fail(c::CURLE_URL_MALFORMAT, format!("URL rejected: {m}"));
                        }
                    };
                    if let Err(mut f) =
                        self.check_scheme(&nu.scheme, o.redir_protocols & o.protocols)
                    {
                        if f.message.ends_with("disabled") {
                            f.message.push_str(" (in redirect)");
                        }
                        return Err(f);
                    }
                    self.info.redirect_count = redirects;
                    self.redirect_base_us = self.us();
                    self.info.redirect_us = self.redirect_base_us;
                    self.text(&format!(
                        "Issue another request to this URL: '{}'",
                        String::from_utf8_lossy(&nu.to_bytes())
                    ));
                    // The request type, not a custom method name, decides
                    // (observed on PHP 8.5: a POST sent as "PUT" becomes a
                    // body-less "PUT" after a 302 or 303).
                    let is_post = !matches!(plan, Plan::None) && !o.upload;
                    let switch = match code {
                        301 => is_post && o.postredir & 1 == 0,
                        302 => is_post && o.postredir & 2 == 0,
                        303 => !o.nobody && !(is_post && o.postredir & 4 != 0),
                        _ => false,
                    };
                    if switch {
                        plan = Plan::None;
                        if o.followlocation == c::CURLFOLLOW_OBEYCODE {
                            custom = None;
                        }
                    }
                    if o.followlocation == c::CURLFOLLOW_FIRSTONLY {
                        custom = None;
                    }
                    if matches!(plan, Plan::Reader { .. }) {
                        return fail(E_SEND_FAIL_REWIND, "necessary data rewind wasn't possible");
                    }
                    if o.autoreferer {
                        referer = Some(prev);
                    }
                    url = nu;
                    continue;
                }
            }
        }
    }

    /// Decides what follows a response head.
    fn after_head(&mut self, head: &Head, may_auth: bool, _redirects: i64) -> Next {
        if head.code == 401 && may_auth {
            // The safest scheme offered among the allowed ones (libcurl
            // documentation of CURLOPT_HTTPAUTH).
            let mask = self.opts.httpauth;
            let mut basic = false;
            for v in head.all("www-authenticate") {
                if mask & c::CURLAUTH_DIGEST != 0
                    && let Some(d) = Digest::parse(v)
                {
                    return Next::Auth(Challenge::Digest(d));
                }
                if v.len() >= 5 && v[..5].eq_ignore_ascii_case(b"basic") {
                    basic = true;
                }
            }
            if basic && mask & c::CURLAUTH_BASIC != 0 {
                return Next::Auth(Challenge::Basic);
            }
        }
        if (300..400).contains(&head.code)
            && !matches!(head.code, 304..=306)
            && let Some(loc) = head.get("location")
            && !loc.is_empty()
            && self.opts.followlocation != 0
        {
            return Next::Redirect(head.code, loc.to_vec());
        }
        Next::Done
    }

    fn initial_plan(&mut self, _url: &Url) -> Plan {
        let o = self.opts;
        let user_len = o.headers.iter().find_map(|h| {
            let p = h.iter().position(|&c| c == b':')?;
            if trim(&h[..p]).eq_ignore_ascii_case(b"content-length") {
                String::from_utf8_lossy(trim(&h[p + 1..]))
                    .parse::<i64>()
                    .ok()
            } else {
                None
            }
        });
        if o.upload {
            return Plan::Reader {
                size: if o.infilesize >= 0 {
                    Some(o.infilesize)
                } else {
                    user_len
                },
            };
        }
        if !o.post {
            return Plan::None;
        }
        match &o.body {
            Body::Fields(d) => {
                return Plan::Bytes {
                    data: d.clone(),
                    ctype: b"application/x-www-form-urlencoded".to_vec(),
                };
            }
            Body::Form(parts) => {
                let host = &mut *self.host;
                let b = multipart::boundary(&mut || host.random());
                let data = multipart::encode(parts, &b, o.mime_formescape);
                let mut ctype = b"multipart/form-data; boundary=".to_vec();
                ctype.extend_from_slice(&b);
                return Plan::Bytes { data, ctype };
            }
            Body::None => {}
        }
        Plan::Reader { size: user_len }
    }

    /// Builds the request head.
    #[allow(clippy::too_many_arguments)]
    fn request_head(
        &mut self,
        url: &Url,
        method: &[u8],
        plan: &Plan,
        referer: Option<&[u8]>,
        auth: Option<&[u8]>,
        proxy: Option<&Proxy>,
        absolute: bool,
    ) -> (Vec<u8>, bool, bool) {
        let o = self.opts;
        let v10 = o.http_version == c::CURL_HTTP_VERSION_1_0;
        let (custom, names) = user_headers(&o.headers);
        let has = |n: &str| names.iter().any(|(k, _)| k == n);
        let mut h = Vec::new();
        h.extend_from_slice(method);
        h.push(b' ');
        if let Some(t) = &o.request_target {
            h.extend_from_slice(t);
        } else if absolute {
            h.extend_from_slice(&url.to_bytes());
        } else {
            h.extend_from_slice(&url.target());
        }
        h.extend_from_slice(if v10 {
            b" HTTP/1.0\r\n"
        } else {
            b" HTTP/1.1\r\n"
        });
        let line = |h: &mut Vec<u8>, name: &str, value: &[u8]| {
            h.extend_from_slice(name.as_bytes());
            h.extend_from_slice(b": ");
            h.extend_from_slice(value);
            h.extend_from_slice(b"\r\n");
        };
        match names.iter().rev().find(|(k, _)| k == "host") {
            Some((_, Some(v))) => line(&mut h, "Host", v),
            Some((_, None)) => {}
            None => line(&mut h, "Host", url.host_header().as_bytes()),
        }
        if let Some(a) = auth
            && !has("authorization")
        {
            line(&mut h, "Authorization", a);
        }
        if let Some(p) = proxy
            && absolute
            && let Some(u) = &p.user
            && !has("proxy-authorization")
        {
            let mut cred = u.clone();
            cred.push(b':');
            cred.extend_from_slice(p.pass.as_deref().unwrap_or(b""));
            let mut v = b"Basic ".to_vec();
            v.extend_from_slice(&base64(&cred));
            line(&mut h, "Proxy-Authorization", &v);
            self.info.proxyauth_used = c::CURLAUTH_BASIC;
        }
        if let Some(r) = &o.range
            && !has("range")
        {
            let mut v = b"bytes=".to_vec();
            v.extend_from_slice(r);
            line(&mut h, "Range", &v);
        } else if o.resume_from > 0 && !has("range") {
            line(
                &mut h,
                "Range",
                format!("bytes={}-", o.resume_from).as_bytes(),
            );
        }
        if let Some(ua) = &o.useragent
            && !has("user-agent")
        {
            line(&mut h, "User-Agent", ua);
        }
        if !has("accept") {
            line(&mut h, "Accept", b"*/*");
        }
        if let Some(ae) = &o.accept_encoding
            && !has("accept-encoding")
        {
            let v: &[u8] = if ae.is_empty() { b"deflate, gzip" } else { ae };
            line(&mut h, "Accept-Encoding", v);
        }
        if let Some(r) = referer
            && !has("referer")
        {
            line(&mut h, "Referer", r);
        }
        if !has("cookie") {
            let mut ck: Vec<u8> = Vec::new();
            if self.jar.enabled
                && let Some(v) = self
                    .jar
                    .header_for(&url.host, &url.path, url.scheme == "https")
            {
                ck = v;
            }
            if let Some(cv) = &o.cookie {
                if !ck.is_empty() {
                    ck.extend_from_slice(b"; ");
                }
                ck.extend_from_slice(cv);
            }
            if !ck.is_empty() {
                line(&mut h, "Cookie", &ck);
            }
        }
        if o.timecondition != 0 && o.timevalue != 0 {
            let name = if o.timecondition == 2 {
                "If-Unmodified-Since"
            } else if o.timecondition == 3 {
                "Last-Modified"
            } else {
                "If-Modified-Since"
            };
            line(&mut h, name, http_date(o.timevalue).as_bytes());
        }
        for l in &custom {
            if trim(&l[..l.iter().position(|&c| c == b':').unwrap_or(0)])
                .eq_ignore_ascii_case(b"host")
            {
                continue;
            }
            h.extend_from_slice(l);
            h.extend_from_slice(b"\r\n");
        }
        let mut chunked = false;
        let mut expect = false;
        let user_te_chunked = names.iter().any(|(k, v)| {
            k == "transfer-encoding"
                && v.as_ref().is_some_and(|v| {
                    String::from_utf8_lossy(v)
                        .to_ascii_lowercase()
                        .contains("chunked")
                })
        });
        match plan {
            Plan::None => {
                if method.eq_ignore_ascii_case(b"POST")
                    && o.custom_request.is_none()
                    && !has("content-length")
                {
                    line(&mut h, "Content-Length", b"0");
                }
            }
            Plan::Bytes { data, ctype } => {
                if !has("content-length") {
                    line(&mut h, "Content-Length", data.len().to_string().as_bytes());
                }
                if !has("content-type") {
                    line(&mut h, "Content-Type", ctype);
                }
            }
            Plan::Reader { size } => {
                match size {
                    Some(n) => {
                        if !has("content-length") {
                            line(&mut h, "Content-Length", n.to_string().as_bytes());
                        }
                    }
                    None => {
                        chunked = !v10;
                        if chunked && !has("transfer-encoding") {
                            line(&mut h, "Transfer-Encoding", b"chunked");
                        }
                    }
                }
                if user_te_chunked {
                    chunked = true;
                }
                if !v10 && !has("expect") && (size.is_none() || size.is_some_and(|n| n > 1 << 20)) {
                    line(&mut h, "Expect", b"100-continue");
                    expect = true;
                }
                if method.eq_ignore_ascii_case(b"POST") && !o.upload && !has("content-type") {
                    line(&mut h, "Content-Type", b"application/x-www-form-urlencoded");
                }
            }
        }
        h.extend_from_slice(b"\r\n");
        (h, chunked, expect)
    }

    /// One request and its response: connection, request, head, body.
    #[allow(clippy::too_many_arguments)]
    fn exchange(
        &mut self,
        url: &Url,
        method: &[u8],
        plan: &Plan,
        referer: Option<&[u8]>,
        auth: Option<&[u8]>,
        decide: &mut dyn FnMut(&mut Self, &Head) -> Next,
    ) -> Result<Next, Failure> {
        let o = self.opts;
        let mut port = url.port_or_default();
        if o.port > 0 && o.port <= 65535 {
            port = o.port as u16;
        }
        let mut url = url.clone();
        if o.port > 0 && o.port <= 65535 {
            url.port = Some(port);
        }
        let mut connect_host = url.host.clone();
        let mut connect_port = port;
        for e in &o.connect_to {
            let s = String::from_utf8_lossy(e).into_owned();
            let f: Vec<&str> = s.splitn(4, ':').collect();
            if f.len() == 4
                && (f[0].is_empty() || f[0].eq_ignore_ascii_case(&url.host))
                && (f[1].is_empty() || f[1] == port.to_string())
            {
                if !f[2].is_empty() {
                    connect_host = f[2].to_string();
                }
                if let Ok(p) = f[3].parse() {
                    connect_port = p;
                }
                break;
            }
        }
        let proxy = self.proxy_for(&url)?;
        let https = url.scheme == "https";
        let key = format!(
            "{}|{}|{}|{}",
            url.scheme,
            connect_host.to_ascii_lowercase(),
            connect_port,
            proxy
                .as_ref()
                .map(|p| format!("{}:{}:{}", p.kind, p.host, p.port))
                .unwrap_or_default()
        );
        // A kept connection to the same place is used again (libcurl's
        // connection cache), unless CURLOPT_FRESH_CONNECT.
        if !o.fresh_connect
            && let Some((conn, absolute)) = self.pool.take_idle(&key)
        {
            self.text(&format!(
                "Reusing existing {}: connection with host {}",
                url.scheme, url.host
            ));
            if proxy.is_some() {
                self.info.used_proxy = 1;
            }
            if let (Some(rem), Some(loc)) = (conn.remote, conn.local) {
                self.info.primary_ip = rem.ip().to_string();
                self.info.primary_port = rem.port() as i64;
                self.info.local_ip = loc.ip().to_string();
                self.info.local_port = loc.port() as i64;
            }
            self.info.namelookup_us = self.us();
            self.info.connect_us = self.us();
            let before = (self.info.header_size, self.info.request_size);
            let retry_ok = !matches!(plan, Plan::Reader { .. });
            let r = self.exchange_on(
                conn,
                &url,
                method,
                plan,
                referer,
                auth,
                proxy.as_ref(),
                absolute,
                &key,
                true,
                decide,
            );
            match r {
                Err(f)
                    if retry_ok
                        && self.info.header_size == before.0
                        && matches!(
                            f.code,
                            c::CURLE_GOT_NOTHING | c::CURLE_SEND_ERROR | c::CURLE_RECV_ERROR
                        ) =>
                {
                    // The server closed the kept connection: a new one.
                    self.info.request_size = before.1;
                    self.text("Connection died, retrying a fresh connect");
                }
                r => return r,
            }
        }
        let mut absolute = false;
        let mut conn = match &proxy {
            None => self.open(&connect_host, connect_port, false)?,
            Some(p) => {
                self.info.used_proxy = 1;
                let mut conn = self.open(&p.host.clone(), p.port, true)?;
                if p.kind == c::CURLPROXY_HTTPS {
                    let ph = p.host.clone();
                    self.start_tls(&mut conn, &ph)?;
                }
                match p.kind {
                    k if k == c::CURLPROXY_SOCKS4
                        || k == c::CURLPROXY_SOCKS4A
                        || k == c::CURLPROXY_SOCKS5
                        || k == c::CURLPROXY_SOCKS5_HOSTNAME =>
                    {
                        conn.deadline = Some(self.connect_deadline());
                        self.socks(&mut conn, p, &connect_host, connect_port)?;
                    }
                    _ => {
                        if https || o.httpproxytunnel {
                            conn.deadline = Some(self.connect_deadline());
                            self.tunnel(&mut conn, p, &url)?;
                        } else {
                            absolute = true;
                        }
                    }
                }
                conn
            }
        };
        self.info.conn_id = conn.id;
        conn.deadline = Some(self.connect_deadline());
        if https {
            let h = url.host.clone();
            self.start_tls(&mut conn, &h)?;
        }
        conn.deadline = self.deadline;
        self.exchange_on(
            conn,
            &url,
            method,
            plan,
            referer,
            auth,
            proxy.as_ref(),
            absolute,
            &key,
            false,
            decide,
        )
    }

    /// The request and response on a connection.
    #[allow(clippy::too_many_arguments)]
    fn exchange_on(
        &mut self,
        mut conn: Conn,
        url: &Url,
        method: &[u8],
        plan: &Plan,
        referer: Option<&[u8]>,
        auth: Option<&[u8]>,
        proxy: Option<&Proxy>,
        absolute: bool,
        key: &str,
        reused: bool,
        decide: &mut dyn FnMut(&mut Self, &Head) -> Next,
    ) -> Result<Next, Failure> {
        let o = self.opts;
        let url = url.clone();
        self.info.conn_id = conn.id;
        conn.deadline = self.deadline;
        if let (Some(rem), Some(loc)) = (conn.remote, conn.local) {
            let r = self.host.prereq(
                &rem.ip().to_string(),
                &loc.ip().to_string(),
                rem.port(),
                loc.port(),
            );
            self.check_stop()?;
            if r == Some(false) {
                return fail(
                    c::CURLE_ABORTED_BY_CALLBACK,
                    "operation aborted by pre-request callback",
                );
            }
        }
        self.progress(0, 0, 0, 0)?;
        if !reused {
            self.text("using HTTP/1.x");
        }
        let (head_bytes, chunked, expect) =
            self.request_head(&url, method, plan, referer, auth, proxy, absolute);
        self.info.pretransfer_us = self.us();
        if o.record_header_out {
            self.info.header_out = Some(head_bytes.clone());
        }
        if o.verbose {
            self.host.debug(HEADER_OUT, &head_bytes);
        }
        self.info.request_size += head_bytes.len() as i64;
        if let Plan::Bytes { data, .. } = plan {
            self.info.upload_content_length = data.len() as i64;
        } else if let Plan::Reader { size: Some(n) } = plan {
            self.info.upload_content_length = *n;
        } else {
            self.info.upload_content_length = 0;
        }
        let mut wire = head_bytes;
        let body_now = match plan {
            Plan::Bytes { data, .. } if !expect => {
                wire.extend_from_slice(data);
                true
            }
            _ => false,
        };
        conn.write_all(&wire).map_err(|e| self.send_failure(e))?;
        if body_now && let Plan::Bytes { data, .. } = plan {
            self.info.size_upload = data.len() as i64;
            self.info.request_size += data.len() as i64;
            if o.verbose && !data.is_empty() {
                self.host.debug(DATA_OUT, data);
            }
        }
        // Expect: 100-continue: wait for the interim response, or a final
        // one that makes the body unnecessary.
        let mut early: Option<Head> = None;
        if expect {
            let wait = Duration::from_millis(o.expect_100_timeout_ms.max(0) as u64);
            if conn.readable_within(wait) {
                let head = self.read_head(&mut conn)?;
                if head.code == 100 {
                    self.take_head(&head, &url)?;
                } else {
                    early = Some(head);
                }
            } else {
                self.text("Done waiting for 100-continue");
            }
        }
        if early.is_none() && !body_now {
            self.send_body(&mut conn, plan, chunked)?;
        }
        self.info.posttransfer_us = self.us();
        if self.info.size_upload > 0 {
            let m = format!(
                "upload completely sent off: {} bytes",
                self.info.size_upload
            );
            self.text(&m);
        } else {
            self.text("Request completely sent off");
        }
        self.progress(
            0,
            0,
            self.info.upload_content_length.max(0),
            self.info.size_upload,
        )?;
        let head = loop {
            let head = match early.take() {
                Some(h) => h,
                None => self.read_head(&mut conn)?,
            };
            if (100..200).contains(&head.code) && head.code != 101 {
                self.take_head(&head, &url)?;
                continue;
            }
            break head;
        };
        self.take_head(&head, &url)?;
        if o.filetime
            && let Some(lm) = head.get("last-modified")
            && let Some(t) = crate::cookie::parse_cookie_date(lm)
        {
            self.info.filetime = t;
        }
        let next = decide(self, &head);
        let is_redirect = (300..400).contains(&head.code);
        if is_redirect
            && let Some(loc) = head.get("location")
            && !loc.is_empty()
        {
            self.info.redirect_url = Some(url.resolve(loc));
            if matches!(next, Next::Redirect(..)) {
                self.info.redirect_url = None;
            }
        }
        if o.failonerror && head.code >= 400 && !matches!(next, Next::Auth(_)) {
            self.close(conn, true);
            return fail(
                c::CURLE_HTTP_RETURNED_ERROR,
                format!("The requested URL returned error: {}", head.code),
            );
        }
        if (o.resume_from > 0 || o.range.is_some())
            && o.range.is_none()
            && (200..300).contains(&head.code)
            && head.code != 206
            && matches!(next, Next::Done)
        {
            self.close(conn, true);
            return fail(
                E_RANGE_ERROR,
                "HTTP server does not seem to support byte ranges. Cannot resume.",
            );
        }
        let skip = !matches!(next, Next::Done);
        let close = self.read_body(&mut conn, &head, method, skip)?;
        if close || o.forbid_reuse {
            self.close(conn, true);
        } else {
            let h = format!("{}:{}", url.host_display(), url.port_or_default());
            self.text(&format!(
                "Connection #{} to host {} left intact",
                conn.id, h
            ));
            self.pool.put_idle(key.to_string(), conn, absolute);
        }
        Ok(next)
    }

    fn close(&mut self, mut conn: Conn, closing: bool) {
        if closing {
            self.text(&format!("shutting down connection #{}", conn.id));
        }
        conn.close();
    }

    fn send_body(&mut self, conn: &mut Conn, plan: &Plan, chunked: bool) -> Result<(), Failure> {
        match plan {
            Plan::None => Ok(()),
            Plan::Bytes { data, .. } => {
                conn.write_all(data).map_err(|e| self.send_failure(e))?;
                self.info.size_upload = data.len() as i64;
                self.info.request_size += data.len() as i64;
                if self.opts.verbose && !data.is_empty() {
                    self.host.debug(DATA_OUT, data);
                }
                Ok(())
            }
            Plan::Reader { size } => {
                let mut sent = 0i64;
                let mut pauses = 0;
                loop {
                    if let Some(n) = size
                        && sent >= *n
                    {
                        break;
                    }
                    let max = if chunked { 65524 } else { 65536 };
                    let r = self.host.read(max);
                    self.check_stop()?;
                    let mut data = match r {
                        ReadResult::Data(d) => d,
                        ReadResult::Pause => {
                            pauses += 1;
                            if pauses > 20000 {
                                return fail(c::CURLE_ABORTED_BY_CALLBACK, "Callback aborted");
                            }
                            std::thread::sleep(Duration::from_micros(500));
                            continue;
                        }
                        ReadResult::Abort => {
                            return fail(
                                c::CURLE_ABORTED_BY_CALLBACK,
                                "operation aborted by callback",
                            );
                        }
                        ReadResult::Funny => {
                            return fail(c::CURLE_READ_ERROR, "read function returned funny value");
                        }
                    };
                    if data.is_empty() {
                        if let Some(n) = size
                            && sent < *n
                        {
                            return fail(
                                c::CURLE_READ_ERROR,
                                format!(
                                    "client read function EOF fail, only {sent}/{n} of needed bytes read"
                                ),
                            );
                        }
                        break;
                    }
                    if let Some(n) = size {
                        let left = (*n - sent) as usize;
                        data.truncate(left);
                    }
                    let mut w = Vec::with_capacity(data.len() + 16);
                    if chunked {
                        w.extend_from_slice(format!("{:x}\r\n", data.len()).as_bytes());
                        w.extend_from_slice(&data);
                        w.extend_from_slice(b"\r\n");
                    } else {
                        w.extend_from_slice(&data);
                    }
                    conn.write_all(&w).map_err(|e| self.send_failure(e))?;
                    if self.opts.verbose {
                        self.host.debug(DATA_OUT, &data);
                    }
                    sent += data.len() as i64;
                    self.info.size_upload = sent;
                    self.info.request_size += w.len() as i64;
                    self.progress(0, 0, size.unwrap_or(0), sent)?;
                }
                if chunked {
                    conn.write_all(b"0\r\n\r\n")
                        .map_err(|e| self.send_failure(e))?;
                    self.info.request_size += 5;
                }
                Ok(())
            }
        }
    }

    /// Reads the response body; whether the connection must close.
    fn read_body(
        &mut self,
        conn: &mut Conn,
        head: &Head,
        method: &[u8],
        skip: bool,
    ) -> Result<bool, Failure> {
        let o = self.opts;
        let conn_close = head.all("connection").any(|v| {
            String::from_utf8_lossy(v)
                .to_ascii_lowercase()
                .contains("close")
        }) || head.version == 1
            && !head.all("connection").any(|v| {
                String::from_utf8_lossy(v)
                    .to_ascii_lowercase()
                    .contains("keep-alive")
            });
        if method.eq_ignore_ascii_case(b"HEAD")
            || head.code == 204
            || head.code == 304
            || (100..200).contains(&head.code)
        {
            if let Some(cl) = head.get("content-length") {
                self.info.download_content_length =
                    String::from_utf8_lossy(cl).trim().parse().unwrap_or(-1);
            }
            self.progress(self.info.download_content_length.max(0), 0, 0, 0)?;
            return Ok(conn_close);
        }
        let chunked = o.transfer_decoding
            && head.get("transfer-encoding").is_some_and(|v| {
                String::from_utf8_lossy(v)
                    .to_ascii_lowercase()
                    .rsplit(',')
                    .next()
                    .is_some_and(|t| t.trim() == "chunked")
            });
        let length: Option<i64> = if chunked || o.ignore_content_length {
            None
        } else {
            head.get("content-length")
                .and_then(|v| String::from_utf8_lossy(v).trim().parse().ok())
        };
        if let Some(n) = length {
            self.info.download_content_length = n;
            if o.maxfilesize > 0 && n > o.maxfilesize {
                return fail(c::CURLE_FILESIZE_EXCEEDED, "Maximum file size exceeded");
            }
        }
        self.expected = length;
        let decode = if !skip && o.accept_encoding.is_some() && o.content_decoding {
            head.get("content-encoding").and_then(|v| {
                let v = String::from_utf8_lossy(v).trim().to_ascii_lowercase();
                match v.as_str() {
                    "gzip" | "x-gzip" => Some(1),
                    "deflate" => Some(2),
                    _ => None,
                }
            })
        } else {
            None
        };
        let mut encoded: Vec<u8> = Vec::new();
        let total = length.unwrap_or(-1).max(0);
        let mut got = 0i64;
        let mut on_data = |me: &mut Self, data: &[u8]| -> Result<(), Failure> {
            got += data.len() as i64;
            me.received = got;
            me.info.size_download = got;
            if o.verbose {
                me.host.debug(DATA_IN, data);
            }
            if skip {
                return Ok(());
            }
            if decode.is_some() {
                encoded.extend_from_slice(data);
            } else {
                let n = me.chunk_size();
                for ch in data.chunks(n) {
                    me.write_checked(ch)?;
                }
            }
            me.progress(total, got, 0, 0)
        };
        let mut buf = vec![0u8; 16384];
        if chunked {
            loop {
                let l = match conn.read_line() {
                    Ok(Some(l)) => l,
                    Ok(None) => {
                        return fail(
                            c::CURLE_PARTIAL_FILE,
                            "transfer closed with outstanding read data remaining",
                        );
                    }
                    Err(e) => return Err(self.recv_failure(e)),
                };
                let t = trim(&l);
                let hex: Vec<u8> = t
                    .iter()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .copied()
                    .collect();
                if hex.is_empty() {
                    return fail(c::CURLE_RECV_ERROR, "invalid chunk size");
                }
                let n =
                    u64::from_str_radix(&String::from_utf8_lossy(&hex), 16).unwrap_or(0) as usize;
                if n == 0 {
                    // Trailers up to the blank line.
                    loop {
                        match conn.read_line() {
                            Ok(Some(l)) if l != b"\r\n" && l != b"\n" => {}
                            _ => break,
                        }
                    }
                    break;
                }
                let mut left = n;
                while left > 0 {
                    let want = left.min(buf.len());
                    let k = match conn.read(&mut buf[..want]) {
                        Ok(0) => {
                            return fail(
                                c::CURLE_PARTIAL_FILE,
                                "transfer closed with outstanding read data remaining",
                            );
                        }
                        Ok(k) => k,
                        Err(e) => return Err(self.recv_failure(e)),
                    };
                    let d = buf[..k].to_vec();
                    on_data(self, &d)?;
                    left -= k;
                }
                let _ = conn.read_line();
            }
        } else if let Some(n) = length {
            let mut left = n.max(0) as usize;
            while left > 0 {
                let want = left.min(buf.len());
                let k = match conn.read(&mut buf[..want]) {
                    Ok(0) => {
                        return fail(
                            c::CURLE_PARTIAL_FILE,
                            format!("transfer closed with {left} bytes remaining to read"),
                        );
                    }
                    Ok(k) => k,
                    Err(e) => return Err(self.recv_failure(e)),
                };
                let d = buf[..k].to_vec();
                on_data(self, &d)?;
                left -= k;
            }
        } else {
            loop {
                let k = match conn.read(&mut buf) {
                    Ok(0) => break,
                    Ok(k) => k,
                    Err(e) => return Err(self.recv_failure(e)),
                };
                let d = buf[..k].to_vec();
                on_data(self, &d)?;
            }
        }
        if let Some(kind) = decode {
            let r = if kind == 1 {
                goro_ext_zlib::gzdecode(&encoded, 0)
            } else {
                goro_ext_zlib::zlib_decode(&encoded, 0)
            };
            match r {
                Ok(d) => {
                    self.deliver_all(&d)?;
                }
                Err(_) => {
                    return fail(
                        c::CURLE_BAD_CONTENT_ENCODING,
                        "Error while processing content unencoding: invalid stored block lengths",
                    );
                }
            }
            self.progress(total, got, 0, 0)?;
        }
        Ok(conn_close || length.is_none() && !chunked)
    }
}

/// What follows a response.
enum Next {
    Done,
    Auth(Challenge),
    Redirect(i64, Vec<u8>),
}

/// The authentication scheme a request uses.
#[derive(Clone)]
enum Challenge {
    Basic,
    Digest(Digest),
}

/// A Digest challenge (RFC 7616).
#[derive(Clone)]
struct Digest {
    realm: Vec<u8>,
    nonce: Vec<u8>,
    opaque: Option<Vec<u8>>,
    qop_auth: bool,
    algorithm: String,
    /// Whether the challenge named its algorithm (it is then echoed).
    algorithm_given: bool,
    nc: u32,
}

impl Digest {
    /// Parses `Digest realm="x", nonce="y", ...`.
    fn parse(v: &[u8]) -> Option<Digest> {
        if v.len() < 7 || !v[..6].eq_ignore_ascii_case(b"digest") {
            return None;
        }
        let mut d = Digest {
            realm: Vec::new(),
            nonce: Vec::new(),
            opaque: None,
            qop_auth: false,
            algorithm: "MD5".into(),
            algorithm_given: false,
            nc: 0,
        };
        let mut rest = &v[6..];
        loop {
            while let [b' ' | b',' | b'\t', r @ ..] = rest {
                rest = r;
            }
            if rest.is_empty() {
                break;
            }
            let eq = rest.iter().position(|&c| c == b'=')?;
            let key = String::from_utf8_lossy(trim(&rest[..eq])).to_ascii_lowercase();
            rest = &rest[eq + 1..];
            let val: Vec<u8>;
            if let [b'"', r @ ..] = rest {
                let mut out = Vec::new();
                let mut i = 0;
                while i < r.len() && r[i] != b'"' {
                    if r[i] == b'\\' && i + 1 < r.len() {
                        i += 1;
                    }
                    out.push(r[i]);
                    i += 1;
                }
                val = out;
                rest = r.get(i + 1..).unwrap_or(b"");
            } else {
                let end = rest.iter().position(|&c| c == b',').unwrap_or(rest.len());
                val = trim(&rest[..end]).to_vec();
                rest = &rest[end..];
            }
            match key.as_str() {
                "realm" => d.realm = val,
                "nonce" => d.nonce = val,
                "opaque" => d.opaque = Some(val),
                "qop" => {
                    d.qop_auth = String::from_utf8_lossy(&val)
                        .split(',')
                        .any(|q| q.trim().eq_ignore_ascii_case("auth"))
                }
                "algorithm" => {
                    d.algorithm = String::from_utf8_lossy(&val).into_owned();
                    d.algorithm_given = true;
                }
                _ => {}
            }
        }
        if d.nonce.is_empty() {
            return None;
        }
        Some(d)
    }

    fn hash(&self, data: &[u8]) -> Vec<u8> {
        let alg = self.algorithm.to_ascii_lowercase();
        let name: &[u8] = if alg.starts_with("sha-256") {
            b"sha256"
        } else if alg.starts_with("sha-512-256") {
            b"sha512/256"
        } else {
            b"md5"
        };
        let raw = goro_ext_hash::hash(name, data, &Default::default(), &mut Vec::new())
            .unwrap_or_default();
        goro_ext_hash::to_hex(&raw)
    }

    /// The Authorization value for a request (RFC 7616 section 3.4).
    fn header(
        &mut self,
        user: &[u8],
        pass: &[u8],
        method: &[u8],
        uri: &[u8],
        cnonce: &[u8],
    ) -> Vec<u8> {
        self.nc += 1;
        let nc = format!("{:08x}", self.nc);
        let cat = |parts: &[&[u8]]| parts.join(&b':');
        let mut ha1 = self.hash(&cat(&[user, &self.realm, pass]));
        if self.algorithm.to_ascii_lowercase().ends_with("-sess") {
            ha1 = self.hash(&cat(&[&ha1, &self.nonce, cnonce]));
        }
        let ha2 = self.hash(&cat(&[method, uri]));
        let response = if self.qop_auth {
            self.hash(&cat(&[
                &ha1,
                &self.nonce,
                nc.as_bytes(),
                cnonce,
                b"auth",
                &ha2,
            ]))
        } else {
            self.hash(&cat(&[&ha1, &self.nonce, &ha2]))
        };
        let mut h = b"Digest username=\"".to_vec();
        h.extend_from_slice(user);
        h.extend_from_slice(b"\", realm=\"");
        h.extend_from_slice(&self.realm);
        h.extend_from_slice(b"\", nonce=\"");
        h.extend_from_slice(&self.nonce);
        h.extend_from_slice(b"\", uri=\"");
        h.extend_from_slice(uri);
        if self.qop_auth {
            h.extend_from_slice(b"\", cnonce=\"");
            h.extend_from_slice(cnonce);
            h.extend_from_slice(b"\", nc=");
            h.extend_from_slice(nc.as_bytes());
            h.extend_from_slice(b", qop=auth, response=\"");
        } else {
            h.extend_from_slice(b"\", response=\"");
        }
        h.extend_from_slice(&response);
        h.push(b'"');
        if let Some(o) = &self.opaque {
            h.extend_from_slice(b", opaque=\"");
            h.extend_from_slice(o);
            h.push(b'"');
        }
        if self.algorithm_given {
            h.extend_from_slice(b", algorithm=");
            h.extend_from_slice(self.algorithm.as_bytes());
        }
        h
    }
}

fn wildcard_match(pattern: &str, host: &str) -> bool {
    if pattern == host {
        return true;
    }
    if let Some(rest) = pattern.strip_prefix("*.")
        && let Some(p) = host.find('.')
    {
        return &host[p + 1..] == rest && p > 0;
    }
    false
}

fn io_text(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => "Connection refused".into(),
        std::io::ErrorKind::ConnectionReset => "Connection reset by peer".into(),
        std::io::ErrorKind::BrokenPipe => "Broken pipe".into(),
        std::io::ErrorKind::NotFound => "No such file or directory".into(),
        _ => {
            let s = e.to_string();
            match s.find(" (os error") {
                Some(p) => s[..p].to_string(),
                None => s,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(1793238034), "Thu, 29 Oct 2026 01:40:34 GMT");
    }

    #[test]
    fn b64() {
        assert_eq!(base64(b"u:p"), b"dTpw");
        assert_eq!(base64(b"a:b"), b"YTpi");
    }

    #[test]
    fn user_header_rules() {
        let (send, names) =
            user_headers(&[b"X-A: 1".to_vec(), b"X-Empty;".to_vec(), b"X-Rm:".to_vec()]);
        assert_eq!(send, vec![b"X-A: 1".to_vec(), b"X-Empty:".to_vec()]);
        assert_eq!(names.len(), 3);
        assert_eq!(names[2], ("x-rm".to_string(), None));
    }
}
