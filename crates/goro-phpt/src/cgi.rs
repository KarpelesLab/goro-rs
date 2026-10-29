//! Tests run through the CGI SAPI: the sections that describe a request
//! (CGI, GET, POST, POST_RAW, PUT, GZIP_POST, DEFLATE_POST, COOKIE) and
//! the response headers a test expects (EXPECTHEADERS).
//!
//! Conventions, with their sources:
//! - The request is the CGI/1.1 environment (RFC 3875 4.1):
//!   REQUEST_METHOD, QUERY_STRING (the GET section), CONTENT_TYPE,
//!   CONTENT_LENGTH, HTTP_COOKIE (the COOKIE section), SCRIPT_FILENAME and
//!   PATH_TRANSLATED (the test script), REDIRECT_STATUS; the body goes to
//!   stdin.
//! - POST is a form body (application/x-www-form-urlencoded). POST_RAW and
//!   PUT start with their `Content-Type:` line; the rest is the body
//!   (tests/basic/rfc1867_*.phpt, whose multipart bodies need it).
//!   GZIP_POST and DEFLATE_POST are form bodies sent gzip / zlib encoded
//!   with a Content-Encoding (ext/soap/tests/server019.phpt and
//!   server020.phpt).
//! - The response starts with header lines and an empty line; the body is
//!   compared with the EXPECT section, and each `Name: value` line of
//!   EXPECTHEADERS must be among the response's headers (names compared
//!   without case).

use std::ffi::OsString;

use crate::section::{Phpt, trim_ws};

/// The CGI request of a test.
pub struct Request {
    pub env: Vec<(OsString, OsString)>,
    pub body: Option<Vec<u8>>,
}

/// Builds the request for a test; `script` is the path of the test script.
pub fn request(phpt: &Phpt, script: &str) -> Request {
    let mut env: Vec<(OsString, OsString)> = Vec::new();
    let mut set = |k: &str, v: &[u8]| {
        use std::os::unix::ffi::OsStringExt;
        env.push((k.into(), OsString::from_vec(v.to_vec())));
    };
    set("REDIRECT_STATUS", b"1");
    set("GATEWAY_INTERFACE", b"CGI/1.1");
    set(
        "QUERY_STRING",
        phpt.get("GET").map(trim_ws).unwrap_or_default(),
    );
    set("SCRIPT_FILENAME", script.as_bytes());
    set("PATH_TRANSLATED", script.as_bytes());
    if let Some(c) = phpt.get("COOKIE") {
        set("HTTP_COOKIE", trim_ws(c));
    }
    let mut method: &[u8] = b"GET";
    let mut body = None;
    if let Some(raw) = phpt.get("POST_RAW").or_else(|| phpt.get("PUT")) {
        method = if phpt.has("POST_RAW") {
            b"POST"
        } else {
            b"PUT"
        };
        let (ct, b) = split_raw(trim_ws(raw));
        if let Some(ct) = ct {
            set("CONTENT_TYPE", &ct);
        }
        set("CONTENT_LENGTH", b.len().to_string().as_bytes());
        body = Some(b);
    } else if let Some((sec, enc)) = [
        ("POST", None),
        ("GZIP_POST", Some("gzip")),
        ("DEFLATE_POST", Some("deflate")),
    ]
    .into_iter()
    .find(|(s, _)| phpt.has(s))
    {
        method = b"POST";
        let data = trim_ws(phpt.get(sec).unwrap_or_default()).to_vec();
        let data = match enc {
            Some("gzip") => gzip(&data),
            Some(_) => zlib(&data),
            None => data,
        };
        if let Some(e) = enc {
            set("HTTP_CONTENT_ENCODING", e.as_bytes());
        }
        set("CONTENT_TYPE", b"application/x-www-form-urlencoded");
        set("CONTENT_LENGTH", data.len().to_string().as_bytes());
        body = Some(data);
    }
    set("REQUEST_METHOD", method);
    Request { env, body }
}

/// A POST_RAW body: its first `Content-Type:` line gives the type, the
/// other lines are the body.
fn split_raw(raw: &[u8]) -> (Option<Vec<u8>>, Vec<u8>) {
    let mut ct = None;
    let mut body: Vec<u8> = Vec::new();
    let mut started = false;
    for line in raw.split(|&c| c == b'\n') {
        if ct.is_none() && line.len() >= 13 && line[..13].eq_ignore_ascii_case(b"content-type:") {
            let v: Vec<u8> = line[13..].iter().copied().filter(|&c| c != b'\r').collect();
            ct = Some(v.trim_ascii().to_vec());
            continue;
        }
        if started {
            body.push(b'\n');
        }
        started = true;
        body.extend_from_slice(line);
    }
    (ct, body)
}

/// The expected headers: `Name: value` lines.
pub fn expected_headers(body: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(body).lines() {
        if let Some((n, v)) = line.split_once(':') {
            let n = n.trim();
            if !n.is_empty() {
                out.push((n.to_string(), v.trim().to_string()));
            }
        }
    }
    out
}

/// Splits a response into its header lines and body.
pub fn split_response(out: &[u8]) -> (Vec<(String, String)>, Vec<u8>) {
    let mut i = 0;
    let end = loop {
        let Some(p) = out[i..].iter().position(|&c| c == b'\n') else {
            return (Vec::new(), out.to_vec());
        };
        let line_end = i + p;
        let line = &out[i..line_end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break (i, line_end + 1);
        }
        i = line_end + 1;
    };
    let headers = expected_headers(&out[..end.0]);
    (headers, out[end.1..].to_vec())
}

/// What is wrong with the response headers, if anything.
pub fn check_headers(expected: &[(String, String)], actual: &[(String, String)]) -> Option<String> {
    let mut missing = Vec::new();
    for (n, v) in expected {
        let found = actual
            .iter()
            .any(|(an, av)| an.eq_ignore_ascii_case(n) && av == v);
        if !found {
            missing.push(format!("{n}: {v}"));
        }
    }
    if missing.is_empty() {
        None
    } else {
        Some(missing.join("; "))
    }
}

/// Renders headers as lines, for failure details.
pub fn header_text(h: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (n, v) in h {
        out.extend_from_slice(format!("{n}: {v}\n").as_bytes());
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &c in data {
        a = (a + c as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Raw deflate with stored blocks only (RFC 1951 3.2.4): valid and simple.
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 5);
    let mut chunks = data.chunks(65535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
        return out;
    }
    while let Some(c) = chunks.next() {
        out.push(if chunks.peek().is_none() { 1 } else { 0 });
        let n = c.len() as u16;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(c);
    }
    out
}

/// The gzip format (RFC 1952), as gzencode() makes.
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
    out.extend(deflate_stored(data));
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// The zlib format (RFC 1950), as gzcompress() makes.
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend(deflate_stored(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_bodies() {
        let (ct, b) = split_raw(b"Content-Type: multipart/form-data; boundary=x\n--x\nA\n--x--");
        assert_eq!(ct.as_deref(), Some(&b"multipart/form-data; boundary=x"[..]));
        assert_eq!(b, b"--x\nA\n--x--");
    }

    #[test]
    fn responses() {
        let (h, b) = split_response(b"Status: 404 Not Found\r\nX-A: 1\r\n\r\nbody\n");
        assert_eq!(
            h,
            vec![
                ("Status".to_string(), "404 Not Found".to_string()),
                ("X-A".to_string(), "1".to_string())
            ]
        );
        assert_eq!(b, b"body\n");
        let exp = expected_headers(b"x-a: 1\n");
        assert_eq!(check_headers(&exp, &h), None);
        assert!(check_headers(&expected_headers(b"X-A: 2"), &h).is_some());
    }

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(&gzip(b"")[10..15], &[1, 0, 0, 0xff, 0xff]);
    }
}
