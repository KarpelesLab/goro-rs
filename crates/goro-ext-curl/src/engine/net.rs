//! Sockets, name resolution, the connection pool (lib/cf-socket.c,
//! lib/hostip.c, lib/conncache.c in spirit).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::os::fd::{AsRawFd, FromRawFd};
use std::time::{Duration, Instant};

use super::tls::TlsSession;

/// strerror() text of an errno value
pub(crate) fn os_strerror(errno: i32) -> String {
    let s = std::io::Error::from_raw_os_error(errno).to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// Curl_printable_address
pub(crate) fn ip_string(ip: &IpAddr) -> String {
    ip.to_string()
}

/// A resolved host name (struct Curl_dns_entry)
#[derive(Clone)]
pub(crate) struct DnsEntry {
    pub addrs: Vec<IpAddr>,
    pub stamp: Instant,
    /// CURLOPT_RESOLVE entries never expire (unless prefixed with '+')
    pub permanent: bool,
}

/// The DNS cache (shared by a multi handle or a share handle)
#[derive(Default)]
pub(crate) struct DnsCache {
    pub entries: HashMap<(String, u16), DnsEntry>,
}

impl DnsCache {
    pub fn get(&mut self, host: &str, port: u16, timeout_secs: i64) -> Option<DnsEntry> {
        let key = (host.to_ascii_lowercase(), port);
        let e = self.entries.get(&key)?;
        if !e.permanent && timeout_secs >= 0 && e.stamp.elapsed().as_secs() as i64 >= timeout_secs {
            self.entries.remove(&key);
            return None;
        }
        Some(e.clone())
    }
    pub fn put(&mut self, host: &str, port: u16, addrs: Vec<IpAddr>, permanent: bool) {
        self.entries.insert(
            (host.to_ascii_lowercase(), port),
            DnsEntry {
                addrs,
                stamp: Instant::now(),
                permanent,
            },
        );
    }
}

/// getaddrinfo() of a host name (AF_UNSPEC); IP literals are returned as is
pub(crate) fn resolve(host: &str, port: u16, ipresolve: i64) -> Option<Vec<IpAddr>> {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    let mut out: Vec<IpAddr> = Vec::new();
    if let Ok(ip) = h.parse::<IpAddr>() {
        out.push(ip);
    } else {
        let addrs = (h, port).to_socket_addrs().ok()?;
        for a in addrs {
            if !out.contains(&a.ip()) {
                out.push(a.ip());
            }
        }
    }
    match ipresolve {
        1 => out.retain(|a| a.is_ipv4()),
        2 => out.retain(|a| a.is_ipv6()),
        _ => {}
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Result of one connect attempt
pub(crate) enum ConnectErr {
    /// errno, local address of the failed socket
    Failed(i32, Option<SocketAddr>),
    Timeout,
}

fn sockaddr_from(addr: &SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let len = match addr {
        SocketAddr::V4(a) => {
            let sin = libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: a.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(a.ip().octets()),
                },
                sin_zero: [0; 8],
            };
            unsafe {
                std::ptr::write(&mut ss as *mut _ as *mut libc::sockaddr_in, sin);
            }
            std::mem::size_of::<libc::sockaddr_in>()
        }
        SocketAddr::V6(a) => {
            let sin6 = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: a.port().to_be(),
                sin6_flowinfo: a.flowinfo(),
                sin6_addr: libc::in6_addr {
                    s6_addr: a.ip().octets(),
                },
                sin6_scope_id: a.scope_id(),
            };
            unsafe {
                std::ptr::write(&mut ss as *mut _ as *mut libc::sockaddr_in6, sin6);
            }
            std::mem::size_of::<libc::sockaddr_in6>()
        }
    };
    (ss, len as libc::socklen_t)
}

fn sockaddr_to(ss: &libc::sockaddr_storage) -> Option<SocketAddr> {
    unsafe {
        match ss.ss_family as i32 {
            libc::AF_INET => {
                let sin = &*(ss as *const _ as *const libc::sockaddr_in);
                let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                Some(SocketAddr::new(IpAddr::V4(ip), u16::from_be(sin.sin_port)))
            }
            libc::AF_INET6 => {
                let sin6 = &*(ss as *const _ as *const libc::sockaddr_in6);
                let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
                Some(SocketAddr::new(
                    IpAddr::V6(ip),
                    u16::from_be(sin6.sin6_port),
                ))
            }
            _ => None,
        }
    }
}

fn local_of(fd: i32) -> Option<SocketAddr> {
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let r = unsafe { libc::getsockname(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut len) };
    if r != 0 {
        return None;
    }
    sockaddr_to(&ss)
}

/// Non-blocking connect with a timeout, like cf-socket.c does
pub(crate) fn tcp_connect(
    addr: &SocketAddr,
    timeout: Option<Duration>,
) -> Result<TcpStream, ConnectErr> {
    let family = if addr.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    let fd = unsafe {
        libc::socket(
            family,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        return Err(ConnectErr::Failed(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
            None,
        ));
    }
    let (ss, len) = sockaddr_from(addr);
    let r = unsafe { libc::connect(fd, &ss as *const _ as *const libc::sockaddr, len) };
    if r != 0 {
        let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if e != libc::EINPROGRESS {
            let local = local_of(fd);
            unsafe { libc::close(fd) };
            return Err(ConnectErr::Failed(e, local));
        }
        let ms = match timeout {
            Some(t) => t.as_millis().min(i32::MAX as u128) as i32,
            None => -1,
        };
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let start = Instant::now();
        loop {
            let left = if ms < 0 {
                -1
            } else {
                (ms as i64 - start.elapsed().as_millis() as i64).max(0) as i32
            };
            let n = unsafe { libc::poll(&mut pfd, 1, left) };
            if n < 0 {
                let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if e == libc::EINTR {
                    continue;
                }
                unsafe { libc::close(fd) };
                return Err(ConnectErr::Failed(e, None));
            }
            if n == 0 {
                unsafe { libc::close(fd) };
                return Err(ConnectErr::Timeout);
            }
            break;
        }
        let mut err: libc::c_int = 0;
        let mut elen = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                &mut err as *mut _ as *mut libc::c_void,
                &mut elen,
            )
        };
        if err != 0 {
            let local = local_of(fd);
            unsafe { libc::close(fd) };
            return Err(ConnectErr::Failed(err, local));
        }
    }
    let s = unsafe { TcpStream::from_raw_fd(fd) };
    let _ = s.set_nonblocking(false);
    Ok(s)
}

/// Wait until the socket is readable (or `timeout` passed). Returns false on
/// timeout.
pub(crate) fn wait_readable(fd: i32, timeout: Option<Duration>) -> bool {
    let ms = match timeout {
        Some(t) => t.as_millis().min(i32::MAX as u128) as i32,
        None => -1,
    };
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let n = unsafe { libc::poll(&mut pfd, 1, ms) };
        if n < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return n != 0;
    }
}

/// The byte stream of a connection: plain TCP or TLS over TCP
pub(crate) struct Conn {
    pub id: i64,
    pub sock: TcpStream,
    pub tls: Option<Box<TlsSession>>,
    /// received (and decrypted) bytes not consumed yet
    pub rbuf: Vec<u8>,
    pub rpos: usize,
    pub eof: bool,
    /// the URL scheme this connection was made for ("http", "https")
    pub scheme: String,
    /// origin host name (as in the URL, lowercase) and port
    pub host: String,
    pub port: u16,
    /// http proxy: (host, port, tunnel)
    pub proxy: Option<(String, u16, bool)>,
    pub proxy_user: Option<Vec<u8>>,
    pub primary: SocketAddr,
    pub local: SocketAddr,
    pub close: bool,
    pub httpversion_seen: u8,
    pub created: Instant,
    pub idle_since: Instant,
    pub ssl_verifyresult: i64,
    pub certinfo: Vec<Vec<Vec<u8>>>,
    /// TLS config fingerprint for reuse checks
    pub tls_key: Vec<u8>,
    pub ntlm: super::httpauth::NtlmData,
    pub proxy_ntlm: super::httpauth::NtlmData,
    /// conn->bits.reuse for the current request
    pub reused: bool,
}

impl Conn {
    pub fn fd(&self) -> i32 {
        self.sock.as_raw_fd()
    }

    /// Bytes buffered for reading
    pub fn buffered(&self) -> usize {
        self.rbuf.len() - self.rpos
    }

    /// Send everything. Returns the io error on failure.
    pub fn send_all(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        match &mut self.tls {
            Some(t) => {
                t.send(data).map_err(|(_, m)| std::io::Error::other(m))?;
                let out = t.take_outgoing();
                self.sock.write_all(&out)
            }
            None => self.sock.write_all(data),
        }
    }

    /// Fill the read buffer; Ok(0) = EOF. Waits at most `timeout`
    /// (Err(TimedOut) when nothing arrived).
    pub fn fill(&mut self, timeout: Option<Duration>) -> Result<usize, std::io::Error> {
        if self.rpos > 0 && self.rpos == self.rbuf.len() {
            self.rbuf.clear();
            self.rpos = 0;
        }
        loop {
            if let Some(t) = &mut self.tls {
                let plain = t.recv().map_err(|(_, m)| std::io::Error::other(m))?;
                if !plain.is_empty() {
                    self.rbuf.extend_from_slice(&plain);
                    return Ok(plain.len());
                }
                if t.peer_closed() {
                    self.eof = true;
                    return Ok(0);
                }
            }
            if self.eof {
                return Ok(0);
            }
            if !wait_readable(self.fd(), timeout) {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
            }
            let mut buf = [0u8; 16384];
            let n = match self.sock.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            };
            if n == 0 {
                self.eof = true;
                return Ok(0);
            }
            match &mut self.tls {
                Some(t) => {
                    t.feed(&buf[..n])
                        .map_err(|(_, m)| std::io::Error::other(m))?;
                    let out = t.take_outgoing();
                    if !out.is_empty() {
                        let _ = self.sock.write_all(&out);
                    }
                }
                None => {
                    self.rbuf.extend_from_slice(&buf[..n]);
                    return Ok(n);
                }
            }
        }
    }

    /// Take up to `max` buffered bytes
    pub fn take(&mut self, max: usize) -> Vec<u8> {
        let n = self.buffered().min(max);
        let v = self.rbuf[self.rpos..self.rpos + n].to_vec();
        self.rpos += n;
        v
    }

    /// A connection in the pool is dead when it is readable (EOF or junk)
    pub fn seems_dead(&mut self) -> bool {
        if self.buffered() > 0 || self.eof {
            return true;
        }
        if !wait_readable(self.fd(), Some(Duration::from_millis(0))) {
            return false;
        }
        let mut b = [0u8; 1];
        let _ = self.sock.set_nonblocking(true);
        let r = self.sock.peek(&mut b);
        let _ = self.sock.set_nonblocking(false);
        match r {
            Ok(0) => true,
            Ok(_) => self.tls.is_none(),
            Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(t) = &mut self.tls {
            t.close();
            let out = t.take_outgoing();
            let _ = self.sock.set_nonblocking(true);
            let _ = self.sock.write_all(&out);
        }
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
    }
}

/// A connection cache plus the id counters of a multi handle
#[derive(Default)]
pub(crate) struct Pool {
    pub idle: Vec<Conn>,
    pub next_conn_id: i64,
    pub next_xfer_id: i64,
    pub dns: std::rc::Rc<std::cell::RefCell<DnsCache>>,
}
