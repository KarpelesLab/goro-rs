//! A lenient HTTP/1.x layer for the built-in web server: request parsing
//! and the response transport.
//!
//! php -S accepts what clients and the tests send: lines ending in a bare
//! LF, header lines without a colon (their text joins the next header's
//! name; test sapi/cli/tests/emptyheader.phpt), any method token; and its
//! responses are close-delimited, with the headers the PHP side orders.
//! Kept apart from the PHP side (which only sees [`Request`] and the
//! `vm::sapi::Transport` trait) so it can move to the httpsd crate once its
//! php-compat mode exists (https://github.com/KarpelesLab/httpsd/issues/6).

use std::io::Write;

/// A parsed request.
pub struct Request {
    pub method: Vec<u8>,
    /// The request target as sent.
    pub target: Vec<u8>,
    /// "HTTP/1.1".
    pub protocol: Vec<u8>,
    /// Header fields in order of first appearance; repeated fields are
    /// joined with ", " (RFC 9110 5.3; test sapi/cli/tests/gh16137).
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &[u8]) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| &v[..])
    }
}

pub enum Parse {
    /// More bytes are needed.
    Incomplete,
    Done(Request),
    /// Not a request; the reason is for the server log.
    Invalid(&'static str),
}

/// Largest request head accepted.
const MAX_HEAD: usize = 1 << 20;

/// The end of the line starting at `i` (index of the LF) and the start of
/// the next one.
fn line_end(buf: &[u8], i: usize) -> Option<(usize, usize)> {
    let p = buf[i..].iter().position(|&c| c == b'\n')? + i;
    let end = if p > i && buf[p - 1] == b'\r' {
        p - 1
    } else {
        p
    };
    Some((end, p + 1))
}

/// Parses a request from the bytes received so far.
pub fn parse(buf: &[u8]) -> Parse {
    // Leading empty lines are ignored (RFC 9112 2.2).
    let mut i = 0;
    while i < buf.len() && (buf[i] == b'\r' || buf[i] == b'\n') {
        i += 1;
    }
    let Some((end, next)) = line_end(buf, i) else {
        return if buf.len() > MAX_HEAD {
            Parse::Invalid("Request too large")
        } else {
            Parse::Incomplete
        };
    };
    let line = &buf[i..end];
    let mut parts = line.split(|&c| c == b' ').filter(|p| !p.is_empty());
    let (Some(method), Some(target), Some(protocol), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Parse::Invalid("Malformed HTTP request");
    };
    if !protocol.starts_with(b"HTTP/") || !method.iter().all(|c| c.is_ascii_graphic()) {
        return Parse::Invalid("Malformed HTTP request");
    }
    let mut headers: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut add = |name: Vec<u8>, value: Vec<u8>| match headers
        .iter_mut()
        .find(|(n, _)| n.eq_ignore_ascii_case(&name))
    {
        Some((_, v)) => {
            v.extend_from_slice(b", ");
            v.extend_from_slice(&value);
        }
        None => headers.push((name, value)),
    };
    let mut pending: Vec<u8> = Vec::new();
    let mut i = next;
    let body_start = loop {
        let Some((end, next)) = line_end(buf, i) else {
            return if buf.len() > MAX_HEAD {
                Parse::Invalid("Request too large")
            } else {
                Parse::Incomplete
            };
        };
        let line = &buf[i..end];
        i = next;
        if line.is_empty() {
            break i;
        }
        match line.iter().position(|&c| c == b':') {
            Some(p) => {
                let mut name = std::mem::take(&mut pending);
                name.extend_from_slice(line[..p].trim_ascii());
                add(name, line[p + 1..].trim_ascii().to_vec());
            }
            None => pending.extend_from_slice(line.trim_ascii()),
        }
    };
    if !pending.is_empty() {
        add(pending, Vec::new());
    }
    let find = |n: &[u8]| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.clone())
    };
    let rest = &buf[body_start..];
    let body = if find(b"transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().ends_with(b"chunked"))
    {
        match dechunk(rest) {
            Some(Ok(b)) => b,
            Some(Err(())) => return Parse::Invalid("Malformed chunked body"),
            None => return Parse::Incomplete,
        }
    } else {
        let len = match find(b"content-length") {
            Some(v) => {
                if v.is_empty() || !v.iter().all(|c| c.is_ascii_digit()) {
                    return Parse::Invalid("Malformed Content-Length");
                }
                match std::str::from_utf8(&v)
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    Some(n) => n,
                    None => return Parse::Invalid("Malformed Content-Length"),
                }
            }
            None => 0,
        };
        if rest.len() < len {
            return Parse::Incomplete;
        }
        rest[..len].to_vec()
    };
    Parse::Done(Request {
        method: method.to_vec(),
        target: target.to_vec(),
        protocol: protocol.to_vec(),
        headers,
        body,
    })
}

/// Decodes a chunked body (RFC 9112 7.1): `None` while incomplete.
fn dechunk(mut b: &[u8]) -> Option<Result<Vec<u8>, ()>> {
    let mut out = Vec::new();
    loop {
        let p = b.iter().position(|&c| c == b'\n')?;
        let line = b[..p].trim_ascii();
        let size = line.split(|&c| c == b';').next().unwrap_or(b"");
        let Some(n) = std::str::from_utf8(size)
            .ok()
            .and_then(|s| usize::from_str_radix(s.trim(), 16).ok())
        else {
            return Some(Err(()));
        };
        b = &b[p + 1..];
        if n == 0 {
            // Trailer fields up to an empty line.
            loop {
                let p = b.iter().position(|&c| c == b'\n')?;
                let empty = b[..p].trim_ascii().is_empty();
                b = &b[p + 1..];
                if empty {
                    return Some(Ok(out));
                }
            }
        }
        if b.len() < n {
            return None;
        }
        out.extend_from_slice(&b[..n]);
        b = &b[n..];
        let p = b.iter().position(|&c| c == b'\n')?;
        b = &b[p + 1..];
    }
}

/// The response side: status line, headers, then the body until the
/// connection closes.
pub struct H1Transport(pub std::net::TcpStream);

impl crate::vm::sapi::Transport for H1Transport {
    fn write_head(&mut self, status_line: &[u8], headers: &[Vec<u8>]) {
        let mut out = status_line.to_vec();
        out.extend_from_slice(b"\r\n");
        for h in headers {
            out.extend_from_slice(h);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        let _ = self.0.write_all(&out);
    }

    fn write_body(&mut self, b: &[u8]) {
        let _ = self.0.write_all(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(b: &[u8]) -> Request {
        match parse(b) {
            Parse::Done(r) => r,
            _ => panic!("not parsed"),
        }
    }

    #[test]
    fn lf_lines_and_body() {
        let r = done(b"POST /x?a=1 HTTP/1.1\nHost: h\nContent-Length: 3\n\nabcdef");
        assert_eq!(r.method, b"POST");
        assert_eq!(r.target, b"/x?a=1");
        assert_eq!(r.header(b"host"), Some(&b"h"[..]));
        assert_eq!(r.body, b"abc");
        assert!(matches!(
            parse(b"POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\nab"),
            Parse::Incomplete
        ));
    }

    #[test]
    fn colonless_lines_join_the_next_name() {
        let r = done(b"GET / HTTP/1.1\r\nUser-Agent\r\nAccept: */*\r\nReferer:\r\nHi\r\n\r\n");
        let h: Vec<_> = r.headers.iter().map(|(n, v)| (&n[..], &v[..])).collect();
        assert_eq!(
            h,
            vec![
                (&b"User-AgentAccept"[..], &b"*/*"[..]),
                (b"Referer", b""),
                (b"Hi", b"")
            ]
        );
    }

    #[test]
    fn repeated_fields_join() {
        let r = done(b"GET / HTTP/1.0\r\nX-F: a\r\nx-f: b\r\n\r\n");
        assert_eq!(r.header(b"X-F"), Some(&b"a, b"[..]));
    }

    #[test]
    fn chunked() {
        let r = done(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n1\r\nd\r\n0\r\n\r\n",
        );
        assert_eq!(r.body, b"abcd");
    }

    #[test]
    fn invalid() {
        assert!(matches!(parse(b"GARBAGE\r\n\r\n"), Parse::Invalid(_)));
    }
}
