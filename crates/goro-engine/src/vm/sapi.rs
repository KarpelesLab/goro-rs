//! Response state of the SAPI: the status code and headers that header(),
//! http_response_code() and setcookie() manage, and for the built-in web
//! server (`goro -S`) the connection the response is written to (PHP
//! manual: header(), http_response_code(), Built-in web server).
//!
//! The CLI keeps the status code but no header list (observed on PHP 8.5:
//! headers_list() is empty in the CLI); the web server keeps both and
//! sends them before the first byte of output.

/// The SAPI response state (`vm.out.sapi`).
#[derive(Default)]
pub struct Sapi {
    /// The "cli-server" SAPI (the built-in web server) runs this request.
    pub server: bool,
    /// The response status; 0 while unset (the CLI before
    /// http_response_code() or a status header).
    pub status: i64,
    /// A status line given with header("HTTP/..."), sent as is.
    pub status_line: Option<Vec<u8>>,
    /// The response headers, in order (web server only).
    pub headers: Vec<Vec<u8>>,
    /// Where the output started (file, line), for "headers already sent".
    pub output_start: Option<(Vec<u8>, u32)>,
    /// The headers were written to the client.
    pub sent: bool,
    /// The connection of a web server request.
    pub conn: Option<Conn>,
    /// The request body (php://input).
    pub input: Vec<u8>,
    /// Files received with the request (is_uploaded_file()).
    pub uploads: Vec<Vec<u8>>,
    /// The request's header fields (getallheaders()).
    pub request_headers: Vec<(Vec<u8>, Vec<u8>)>,
}

/// How a response reaches the client; the web server's HTTP layer
/// (`cli_server::http1`) implements it.
pub trait Transport {
    /// The status line and the header lines, in order (no line ends).
    fn write_head(&mut self, status_line: &[u8], headers: &[Vec<u8>]);
    /// Body bytes (the body ends where the connection does).
    fn write_body(&mut self, b: &[u8]);
}

/// A web server request's client connection.
pub struct Conn {
    pub transport: Box<dyn Transport>,
    /// The request's protocol ("HTTP/1.1").
    pub protocol: Vec<u8>,
    /// The request's Host header, which php -S repeats in the response
    /// (observed on PHP 8.5; tests sapi/cli/tests/php_cli_server_004).
    pub host: Option<Vec<u8>>,
    /// A HEAD request: no body is sent (RFC 9110 9.3.2).
    pub head_only: bool,
    /// The Content-type sent when the script sets none.
    pub default_type: Vec<u8>,
    /// Bytes of body sent.
    pub body_bytes: u64,
}

/// Reason phrases php -S sends; other codes get "Unknown Status Code"
/// (observed on PHP 8.5: 418, 422, 425, 207 and others are unknown; tests
/// sapi/cli/tests/bug65066_422, bug65066_511, bug67429_*). The phrases are
/// those of RFC 9110 and RFC 6585, with the RFC 2616 wording for 413, 414
/// and 416.
pub fn reason(code: i64) -> &'static str {
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
        511 => "Network Authentication Required",
        _ => "Unknown Status Code",
    }
}

/// The name of a `Name: value` header line, trimmed.
pub fn header_name(h: &[u8]) -> &[u8] {
    match h.iter().position(|&c| c == b':') {
        Some(p) => h[..p].trim_ascii(),
        None => h.trim_ascii(),
    }
}

impl Sapi {
    pub fn has_header(&self, name: &[u8]) -> bool {
        self.headers
            .iter()
            .any(|h| h.contains(&b':') && header_name(h).eq_ignore_ascii_case(name))
    }

    /// Writes the status line and headers, once.
    pub fn send_head(&mut self) {
        if self.sent {
            return;
        }
        self.sent = true;
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        // The default Content-type joins the list (headers_list() shows it
        // once sent), except for 304 responses (observed on PHP 8.5; test
        // sapi/cli/tests/bug64878).
        let has_type = self
            .headers
            .iter()
            .any(|h| h.contains(&b':') && header_name(h).eq_ignore_ascii_case(b"content-type"));
        if !has_type && self.status != 304 {
            let mut h = b"Content-type: ".to_vec();
            h.extend_from_slice(&conn.default_type);
            self.headers.push(h);
        }
        let line = match &self.status_line {
            Some(l) => l.clone(),
            None => {
                let mut l = conn.protocol.clone();
                l.extend_from_slice(format!(" {} {}", self.status, reason(self.status)).as_bytes());
                l
            }
        };
        let mut head: Vec<Vec<u8>> = Vec::new();
        if let Some(h) = &conn.host {
            let mut l = b"Host: ".to_vec();
            l.extend_from_slice(h);
            head.push(l);
        }
        // A Date header from the script replaces the server's (test
        // sapi/cli/tests/gh12363).
        let user_date = self
            .headers
            .iter()
            .any(|h| h.contains(&b':') && header_name(h).eq_ignore_ascii_case(b"date"));
        if !user_date {
            head.push(format!("Date: {}", http_date(now())).into_bytes());
        }
        head.push(b"Connection: close".to_vec());
        head.extend(self.headers.iter().filter(|h| !h.is_empty()).cloned());
        conn.transport.write_head(&line, &head);
    }

    /// Sends response body bytes (after the head).
    pub fn write_body(&mut self, b: &[u8]) {
        if b.is_empty() {
            return;
        }
        self.send_head();
        if let Some(conn) = self.conn.as_mut() {
            conn.body_bytes += b.len() as u64;
            if !conn.head_only {
                conn.transport.write_body(b);
            }
        }
    }
}

/// Seconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Civil date of a Unix time (UTC): (year, month 1-12, day, hour, minute,
/// second, weekday 0 = Sunday).
pub fn civil(t: i64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    // Days to civil date (proleptic Gregorian calendar).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let wday = (days + 4).rem_euclid(7) as u32;
    (
        y,
        m,
        d,
        (secs / 3600) as u32,
        (secs / 60 % 60) as u32,
        (secs % 60) as u32,
        wday,
    )
}

pub const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// An IMF-fixdate (RFC 9110 5.6.7): "Sun, 06 Nov 1994 08:49:37 GMT".
pub fn http_date(t: i64) -> String {
    let (y, mo, d, h, mi, s, wd) = civil(t);
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        DAYS[wd as usize],
        d,
        MONTHS[mo as usize - 1],
        y,
        h,
        mi,
        s
    )
}
