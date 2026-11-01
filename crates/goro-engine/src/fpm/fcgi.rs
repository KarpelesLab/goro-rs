//! The FastCGI protocol, application side (FastCGI Specification 1.0:
//! §3 protocol basics, §4 management records, §5 application records).
//!
//! A connection carries records of an 8-byte header (version, type,
//! request id, content length, padding length) and their content. A
//! Responder request is FCGI_BEGIN_REQUEST, FCGI_PARAMS records ended by an
//! empty one, then FCGI_STDIN records ended by an empty one; the answer is
//! FCGI_STDOUT/FCGI_STDERR streams, each ended by an empty record, then
//! FCGI_END_REQUEST.

use std::os::fd::RawFd;

pub const VERSION_1: u8 = 1;

pub const BEGIN_REQUEST: u8 = 1;
pub const ABORT_REQUEST: u8 = 2;
pub const END_REQUEST: u8 = 3;
pub const PARAMS: u8 = 4;
pub const STDIN: u8 = 5;
pub const STDOUT: u8 = 6;
pub const STDERR: u8 = 7;
pub const DATA: u8 = 8;
pub const GET_VALUES: u8 = 9;
pub const GET_VALUES_RESULT: u8 = 10;
pub const UNKNOWN_TYPE: u8 = 11;

/// FCGI_BEGIN_REQUEST flags (§5.1).
pub const KEEP_CONN: u8 = 1;

/// Roles (§5.1).
pub const RESPONDER: u16 = 1;

/// protocolStatus values of FCGI_END_REQUEST (§5.5).
pub const REQUEST_COMPLETE: u8 = 0;
pub const CANT_MPX_CONN: u8 = 1;
pub const UNKNOWN_ROLE: u8 = 3;

/// The largest content of one record.
const MAX_CONTENT: usize = 65535;

/// One request, as read from the connection.
#[derive(Default, Clone)]
pub struct Request {
    pub id: u16,
    /// FCGI_KEEP_CONN: the connection stays open after the response.
    pub keep_conn: bool,
    /// The name-value pairs of FCGI_PARAMS, in order.
    pub params: Vec<(Vec<u8>, Vec<u8>)>,
    /// The FCGI_STDIN stream.
    pub stdin: Vec<u8>,
}

impl Request {
    pub fn param(&self, name: &[u8]) -> Option<&[u8]> {
        self.params
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_slice())
    }
}

/// What reading a connection gave.
pub enum Incoming {
    Request(Request),
    /// The connection ended (or broke) before a complete request.
    Closed,
    /// `on_signal` asked to stop waiting.
    Interrupted,
}

/// A descriptor (a signalfd) watched while waiting for data, and what to
/// do when it is readable: true stops the wait, false goes on with what
/// was read so far.
pub struct Watch<'a> {
    pub fd: RawFd,
    pub on_signal: &'a mut dyn FnMut() -> bool,
}

/// Reads exactly `buf.len()` bytes, or fewer at end of file.
fn read_full(fd: RawFd, buf: &mut [u8], watch: &mut Watch) -> Result<usize, bool> {
    let mut got = 0;
    while got < buf.len() {
        {
            let w = watch.fd;
            let mut fds = [
                libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: w,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: fds is a valid array of two pollfd.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if n < 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(false);
            }
            if fds[1].revents != 0 && (watch.on_signal)() {
                return Err(true);
            }
            if fds[0].revents == 0 {
                continue;
            }
        }
        // SAFETY: reading into the unfilled part of buf.
        let n = unsafe {
            libc::read(
                fd,
                buf[got..].as_mut_ptr() as *mut libc::c_void,
                buf.len() - got,
            )
        };
        if n < 0 {
            let e = std::io::Error::last_os_error().raw_os_error();
            if e == Some(libc::EINTR) || e == Some(libc::EAGAIN) {
                continue;
            }
            return Err(false);
        }
        if n == 0 {
            break;
        }
        got += n as usize;
    }
    Ok(got)
}

/// Writes all of `b`; false when the peer is gone.
pub fn write_all(fd: RawFd, mut b: &[u8]) -> bool {
    while !b.is_empty() {
        // SAFETY: writing from a valid slice.
        let n = unsafe { libc::write(fd, b.as_ptr() as *const libc::c_void, b.len()) };
        if n < 0 {
            let e = std::io::Error::last_os_error().raw_os_error();
            if e == Some(libc::EINTR) {
                continue;
            }
            if e == Some(libc::EAGAIN) {
                let mut p = libc::pollfd {
                    fd,
                    events: libc::POLLOUT,
                    revents: 0,
                };
                // SAFETY: one valid pollfd.
                unsafe { libc::poll(&mut p, 1, 1000) };
                continue;
            }
            return false;
        }
        b = &b[n as usize..];
    }
    true
}

/// The 8-byte record header (§3.3).
fn header(ty: u8, id: u16, len: usize, pad: usize) -> [u8; 8] {
    [
        VERSION_1,
        ty,
        (id >> 8) as u8,
        id as u8,
        (len >> 8) as u8,
        len as u8,
        pad as u8,
        0,
    ]
}

/// Appends the records of a stream's content (without the closing empty
/// record). No padding (§3.3 makes it optional).
pub fn stream_records(out: &mut Vec<u8>, ty: u8, id: u16, data: &[u8]) {
    for chunk in data.chunks(MAX_CONTENT) {
        out.extend_from_slice(&header(ty, id, chunk.len(), 0));
        out.extend_from_slice(chunk);
    }
}

/// An empty record, the end of a stream.
pub fn end_stream(out: &mut Vec<u8>, ty: u8, id: u16) {
    out.extend_from_slice(&header(ty, id, 0, 0));
}

/// FCGI_END_REQUEST (§5.5).
pub fn end_request(out: &mut Vec<u8>, id: u16, app_status: u32, protocol_status: u8) {
    out.extend_from_slice(&header(END_REQUEST, id, 8, 0));
    out.extend_from_slice(&app_status.to_be_bytes());
    out.push(protocol_status);
    out.extend_from_slice(&[0, 0, 0]);
}

/// Name-value pairs (§3.4): lengths of one byte below 128, else four
/// bytes with the high bit set.
pub fn parse_pairs(mut b: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    fn len(b: &mut &[u8]) -> Option<usize> {
        let first = *b.first()?;
        if first & 0x80 == 0 {
            *b = &b[1..];
            Some(first as usize)
        } else {
            if b.len() < 4 {
                return None;
            }
            let n = (((first & 0x7f) as usize) << 24)
                | ((b[1] as usize) << 16)
                | ((b[2] as usize) << 8)
                | b[3] as usize;
            *b = &b[4..];
            Some(n)
        }
    }
    let mut out = Vec::new();
    while !b.is_empty() {
        let (Some(nl), Some(vl)) = (len(&mut b), len(&mut b)) else {
            break;
        };
        if b.len() < nl + vl {
            break;
        }
        out.push((b[..nl].to_vec(), b[nl..nl + vl].to_vec()));
        b = &b[nl + vl..];
    }
    out
}

pub fn encode_pair(out: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    for l in [name.len(), value.len()] {
        if l < 128 {
            out.push(l as u8);
        } else {
            out.extend_from_slice(&((l as u32) | 0x8000_0000).to_be_bytes());
        }
    }
    out.extend_from_slice(name);
    out.extend_from_slice(value);
}

/// The values this application reports for FCGI_GET_VALUES (§4.1): it
/// takes one connection per process and does not multiplex.
fn value_of(name: &[u8], max_conns: usize) -> Option<Vec<u8>> {
    match name {
        b"FCGI_MAX_CONNS" | b"FCGI_MAX_REQS" => Some(max_conns.to_string().into_bytes()),
        b"FCGI_MPXS_CONNS" => Some(b"0".to_vec()),
        _ => None,
    }
}

/// Reads the next request from a connection, answering management
/// records on the way. `max_conns` is what FCGI_MAX_CONNS reports.
pub fn read_request(fd: RawFd, watch: &mut Watch, max_conns: usize) -> Incoming {
    let mut req: Option<Request> = None;
    let mut params = Vec::new();
    let mut params_done = false;
    loop {
        let mut h = [0u8; 8];
        match read_full(fd, &mut h, watch) {
            Ok(8) => {}
            Ok(_) => return Incoming::Closed,
            Err(true) => return Incoming::Interrupted,
            Err(false) => return Incoming::Closed,
        }
        let ty = h[1];
        let id = u16::from_be_bytes([h[2], h[3]]);
        let len = u16::from_be_bytes([h[4], h[5]]) as usize;
        let pad = h[6] as usize;
        let mut content = vec![0u8; len + pad];
        match read_full(fd, &mut content, watch) {
            Ok(n) if n == len + pad => {}
            Err(true) => return Incoming::Interrupted,
            _ => return Incoming::Closed,
        }
        content.truncate(len);
        if h[0] != VERSION_1 {
            return Incoming::Closed;
        }
        match ty {
            GET_VALUES if id == 0 => {
                let mut body = Vec::new();
                for (n, _) in parse_pairs(&content) {
                    if let Some(v) = value_of(&n, max_conns) {
                        encode_pair(&mut body, &n, &v);
                    }
                }
                let mut out = Vec::new();
                out.extend_from_slice(&header(GET_VALUES_RESULT, 0, body.len(), 0));
                out.extend_from_slice(&body);
                if !write_all(fd, &out) {
                    return Incoming::Closed;
                }
            }
            BEGIN_REQUEST => {
                if content.len() < 8 {
                    return Incoming::Closed;
                }
                let role = u16::from_be_bytes([content[0], content[1]]);
                let keep = content[2] & KEEP_CONN != 0;
                if req.is_some() {
                    // One request at a time on a connection (§5.5:
                    // FCGI_CANT_MPX_CONN).
                    let mut out = Vec::new();
                    end_request(&mut out, id, 0, CANT_MPX_CONN);
                    let _ = write_all(fd, &out);
                    continue;
                }
                if role != RESPONDER {
                    let mut out = Vec::new();
                    end_request(&mut out, id, 0, UNKNOWN_ROLE);
                    if !write_all(fd, &out) || !keep {
                        return Incoming::Closed;
                    }
                    continue;
                }
                req = Some(Request {
                    id,
                    keep_conn: keep,
                    ..Default::default()
                });
                params.clear();
                params_done = false;
            }
            PARAMS => {
                if let Some(r) = req.as_mut().filter(|r| r.id == id) {
                    if content.is_empty() {
                        params_done = true;
                        r.params = parse_pairs(&params);
                    } else {
                        params.extend_from_slice(&content);
                    }
                }
            }
            STDIN => {
                if let Some(r) = req.as_mut().filter(|r| r.id == id) {
                    if content.is_empty() {
                        if !params_done {
                            r.params = parse_pairs(&params);
                        }
                        return Incoming::Request(req.take().unwrap());
                    }
                    r.stdin.extend_from_slice(&content);
                }
            }
            ABORT_REQUEST => {
                if req.as_ref().is_some_and(|r| r.id == id) {
                    let mut out = Vec::new();
                    end_request(&mut out, id, 0, REQUEST_COMPLETE);
                    let keep = req.as_ref().is_some_and(|r| r.keep_conn);
                    req = None;
                    if !write_all(fd, &out) || !keep {
                        return Incoming::Closed;
                    }
                }
            }
            DATA => {}
            t if id == 0 => {
                // An unknown management record (§4.2).
                let mut out = Vec::new();
                out.extend_from_slice(&header(UNKNOWN_TYPE, 0, 8, 0));
                out.push(t);
                out.extend_from_slice(&[0; 7]);
                if !write_all(fd, &out) {
                    return Incoming::Closed;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_round_trip() {
        let mut b = Vec::new();
        encode_pair(&mut b, b"SCRIPT_NAME", b"/x.php");
        let long = vec![b'a'; 300];
        encode_pair(&mut b, b"HTTP_X", &long);
        let p = parse_pairs(&b);
        assert_eq!(p[0], (b"SCRIPT_NAME".to_vec(), b"/x.php".to_vec()));
        assert_eq!(p[1].1.len(), 300);
    }

    #[test]
    fn stream_padding() {
        let mut out = Vec::new();
        stream_records(&mut out, STDOUT, 1, b"hello");
        assert_eq!(out.len(), 13);
        assert_eq!(out[6], 0);
    }
}
