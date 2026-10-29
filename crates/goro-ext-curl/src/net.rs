//! Connections: TCP (IPv6 and IPv4 addresses tried in order, as libcurl's
//! "Trying" messages show), Unix domain sockets (CURLOPT_UNIX_SOCKET_PATH)
//! and TLS on goro-ext-openssl's sessions, with deadlines for every
//! blocking operation.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use goro_ext_openssl::tls::{Io, Session};

/// The socket under a connection.
pub enum Sock {
    Tcp(TcpStream),
    Unix(UnixStream),
}

impl Sock {
    fn set_read_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        match self {
            Sock::Tcp(s) => s.set_read_timeout(d),
            Sock::Unix(s) => s.set_read_timeout(d),
        }
    }

    fn set_write_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        match self {
            Sock::Tcp(s) => s.set_write_timeout(d),
            Sock::Unix(s) => s.set_write_timeout(d),
        }
    }

    fn peek(&self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Sock::Tcp(s) => s.peek(b),
            Sock::Unix(s) => {
                use std::os::fd::AsRawFd;
                // SAFETY: recv(2) into a valid buffer of the given length on
                // a descriptor the stream owns.
                let n = unsafe {
                    libc::recv(
                        s.as_raw_fd(),
                        b.as_mut_ptr().cast(),
                        b.len(),
                        libc::MSG_PEEK,
                    )
                };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
        }
    }
}

impl io::Read for Sock {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Sock::Tcp(s) => io::Read::read(s, b),
            Sock::Unix(s) => io::Read::read(s, b),
        }
    }
}

impl io::Write for Sock {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match self {
            Sock::Tcp(s) => io::Write::write(s, b),
            Sock::Unix(s) => io::Write::write(s, b),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct SockIo<'a>(&'a mut Sock);

impl Io for SockIo<'_> {
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match io::Read::read(self.0, buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                r => return r,
            }
        }
    }
    fn send(&mut self, data: &[u8]) -> io::Result<usize> {
        loop {
            match io::Write::write(self.0, data) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                r => return r,
            }
        }
    }
}

/// An open connection.
pub struct Conn {
    pub sock: Sock,
    pub tls: Option<Box<Session>>,
    buf: Vec<u8>,
    pos: usize,
    pub id: i64,
    pub remote: Option<SocketAddr>,
    pub local: Option<SocketAddr>,
    /// The deadline of the current operation.
    pub deadline: Option<Instant>,
}

/// Why a read or write failed.
#[derive(Debug)]
pub enum NetError {
    Timeout,
    Io(io::Error),
}

impl Conn {
    pub fn new(sock: Sock, id: i64) -> Conn {
        let (remote, local) = match &sock {
            Sock::Tcp(s) => (s.peer_addr().ok(), s.local_addr().ok()),
            Sock::Unix(_) => (None, None),
        };
        Conn {
            sock,
            tls: None,
            buf: Vec::new(),
            pos: 0,
            id,
            remote,
            local,
            deadline: None,
        }
    }

    fn arm(&mut self) -> Result<(), NetError> {
        let t = match self.deadline {
            None => None,
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    return Err(NetError::Timeout);
                }
                Some((d - now).max(Duration::from_millis(1)))
            }
        };
        let _ = self.sock.set_read_timeout(t);
        let _ = self.sock.set_write_timeout(t);
        Ok(())
    }

    fn map(e: io::Error) -> NetError {
        match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => NetError::Timeout,
            _ => NetError::Io(e),
        }
    }

    /// Runs the TLS handshake.
    pub fn handshake(&mut self, mut session: Session) -> Result<(), String> {
        if let Err(NetError::Timeout) = self.arm() {
            return Err("timeout".into());
        }
        let ahead = self.buf.split_off(self.pos);
        self.buf.clear();
        self.pos = 0;
        if !ahead.is_empty() {
            session.push_input(&ahead);
        }
        loop {
            match session.handshake(&mut SockIo(&mut self.sock)) {
                Ok(true) => break,
                Ok(false) => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        self.tls = Some(Box::new(session));
        Ok(())
    }

    /// Reads some bytes (0 at the end of the stream).
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, NetError> {
        if self.pos < self.buf.len() {
            let n = out.len().min(self.buf.len() - self.pos);
            out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            if self.pos == self.buf.len() {
                self.buf.clear();
                self.pos = 0;
            }
            return Ok(n);
        }
        self.read_raw(out)
    }

    fn fill(&mut self) -> Result<bool, NetError> {
        let mut chunk = [0u8; 16384];
        let n = self.read_raw(&mut chunk)?;
        if n == 0 {
            return Ok(false);
        }
        if self.pos > 0 && self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        }
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(true)
    }

    fn read_raw(&mut self, out: &mut [u8]) -> Result<usize, NetError> {
        self.arm()?;
        loop {
            let r = match &mut self.tls {
                Some(t) => t.read(&mut SockIo(&mut self.sock), out),
                None => io::Read::read(&mut self.sock, out),
            };
            match r {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => return Ok(0),
                Err(e) => return Err(Self::map(e)),
                Ok(n) => return Ok(n),
            }
        }
    }

    /// The next line with its line end (an unterminated last line too);
    /// None at the end of the stream.
    pub fn read_line(&mut self) -> Result<Option<Vec<u8>>, NetError> {
        loop {
            if let Some(p) = self.buf[self.pos..].iter().position(|&c| c == b'\n') {
                let l = self.buf[self.pos..self.pos + p + 1].to_vec();
                self.pos += p + 1;
                return Ok(Some(l));
            }
            if self.buf.len() - self.pos > 1 << 20 {
                let l = self.buf[self.pos..].to_vec();
                self.pos = self.buf.len();
                return Ok(Some(l));
            }
            if !self.fill()? {
                if self.pos < self.buf.len() {
                    let l = self.buf[self.pos..].to_vec();
                    self.pos = self.buf.len();
                    return Ok(Some(l));
                }
                return Ok(None);
            }
        }
    }

    /// Writes everything.
    pub fn write_all(&mut self, mut data: &[u8]) -> Result<(), NetError> {
        self.arm()?;
        while !data.is_empty() {
            let r = match &mut self.tls {
                Some(t) => t.write(&mut SockIo(&mut self.sock), data),
                None => io::Write::write(&mut self.sock, data),
            };
            match r {
                Ok(0) => {
                    return Err(NetError::Io(io::Error::from(io::ErrorKind::WriteZero)));
                }
                Ok(n) => data = &data[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Self::map(e)),
            }
        }
        if let Some(t) = &mut self.tls {
            t.flush(&mut SockIo(&mut self.sock)).map_err(Self::map)?;
        }
        Ok(())
    }

    /// Whether data arrives within `d` (for Expect: 100-continue).
    pub fn readable_within(&mut self, d: Duration) -> bool {
        if self.pos < self.buf.len() {
            return true;
        }
        if self.tls.as_ref().is_some_and(|t| t.buffered()) {
            return true;
        }
        let d = match self.deadline {
            Some(dl) => d.min(dl.saturating_duration_since(Instant::now())),
            None => d,
        }
        .max(Duration::from_millis(1));
        let _ = self.sock.set_read_timeout(Some(d));
        let mut b = [0u8; 1];
        self.sock.peek(&mut b).is_ok()
    }

    /// Whether an idle connection is still usable: nothing to read (the
    /// peer has neither closed it nor sent anything unexpected).
    pub fn is_alive(&mut self) -> bool {
        if self.pos < self.buf.len() || self.tls.as_ref().is_some_and(|t| t.buffered()) {
            return false;
        }
        let fd = match &self.sock {
            Sock::Tcp(s) => {
                use std::os::fd::AsRawFd;
                s.as_raw_fd()
            }
            Sock::Unix(s) => {
                use std::os::fd::AsRawFd;
                s.as_raw_fd()
            }
        };
        let mut b = [0u8; 1];
        // SAFETY: a one-byte buffer on a descriptor the connection owns.
        let n = unsafe {
            libc::recv(
                fd,
                b.as_mut_ptr().cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        n < 0 && matches!(io::Error::last_os_error().kind(), io::ErrorKind::WouldBlock)
    }

    /// Closes the TLS session politely.
    pub fn close(&mut self) {
        if let Some(t) = &mut self.tls {
            let _ = self
                .sock
                .set_write_timeout(Some(Duration::from_millis(200)));
            t.close(&mut SockIo(&mut self.sock));
        }
    }
}

/// The text of an address for messages ("::1", "127.0.0.1").
pub fn ip_text(a: &IpAddr) -> String {
    a.to_string()
}

/// Connects to the first address that accepts, before `deadline`.
/// `trying` is told about each attempt; failures give the last error.
pub fn connect(
    addrs: &[SocketAddr],
    deadline: Instant,
    trying: &mut dyn FnMut(&SocketAddr, Option<&io::Error>),
) -> Result<TcpStream, (bool, Option<io::Error>)> {
    let mut last = None;
    let mut timed_out = false;
    for a in addrs {
        trying(a, None);
        let now = Instant::now();
        if now >= deadline {
            timed_out = true;
            break;
        }
        match TcpStream::connect_timeout(a, deadline - now) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => {
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) {
                    timed_out = true;
                }
                trying(a, Some(&e));
                last = Some(e);
            }
        }
    }
    Err((timed_out, last))
}
