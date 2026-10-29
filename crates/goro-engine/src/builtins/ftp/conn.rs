//! The FTP client (RFC 959, with RFC 2228/4217 AUTH TLS, RFC 2389 and
//! RFC 3659 SIZE/MDTM/MLSD): the control connection, replies, data
//! connections (PORT and PASV) and the ASCII line-ending conversions.
//! Shared by ext/ftp and the ftp:// stream wrapper.

use std::cell::RefCell;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

use goro_runtime::{PhpArray, PhpStr, ResRef, Value};

use crate::builtins::file::Stream;
use crate::builtins::sockets::{Kind, Sock, StreamContext};
use crate::builtins::streams::NativeStream;
use crate::func::Args;
use crate::vm::{R, Vm};

/// The representation type (TYPE A / TYPE I).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Ascii,
    Image,
}

/// A transfer in progress (the ftp_nb_* functions).
pub struct Nb {
    pub data: Sock,
    pub dir: NbDir,
    pub ascii: bool,
    /// ASCII: a CR held back at the end of the last chunk.
    pub cr: bool,
}

pub enum NbDir {
    /// A download into a stream.
    Get(Dest),
    /// An upload from a stream.
    Put(Dest),
}

/// The local side of a transfer: a stream resource (ftp_fget) or a stream
/// the call opened (ftp_get).
pub enum Dest {
    Res(ResRef),
    Own(Box<Stream>),
}

impl Dest {
    pub fn with<T>(&mut self, f: impl FnOnce(&mut Stream) -> T) -> Option<T> {
        match self {
            Dest::Own(s) => Some(f(s)),
            Dest::Res(r) => {
                let cell = {
                    let data = r.res().data.borrow();
                    data.as_ref()?
                        .downcast_ref::<RefCell<Stream>>()
                        .map(|c| c as *const RefCell<Stream>)?
                };
                // SAFETY: `r` keeps the resource alive for the call.
                let mut s = unsafe { (*cell).try_borrow_mut() }.ok()?;
                Some(f(&mut s))
            }
        }
    }
}

/// An FTP connection.
pub struct Conn {
    pub ctl: Sock,
    inbuf: Vec<u8>,
    pub timeout: Duration,
    pub autoseek: bool,
    pub usepasvaddress: bool,
    pub pasv: bool,
    /// ftp_ssl_connect(): AUTH TLS at login, and protected data.
    pub use_ssl: bool,
    pub ssl_data: bool,
    /// The type last set (none before the first TYPE).
    pub ty: Option<Ty>,
    /// The last reply: code (0 when unreadable), lines, and its text.
    pub code: i32,
    pub lines: Vec<Vec<u8>>,
    pub msg: Vec<u8>,
    pub nb: Option<Nb>,
    pub closed: bool,
}

impl goro_runtime::NativeData for Conn {}

/// A context turning certificate checks off (PHP's FTP over TLS doesn't
/// verify the server; test ext/ftp/tests/002.phpt uses a self-signed
/// certificate).
pub(super) fn no_verify_context() -> Value {
    let mut ssl = PhpArray::new();
    ssl.insert(goro_runtime::Key::from("verify_peer"), Value::Bool(false));
    ssl.insert(
        goro_runtime::Key::from("verify_peer_name"),
        Value::Bool(false),
    );
    ssl.insert(
        goro_runtime::Key::from("allow_self_signed"),
        Value::Bool(true),
    );
    let mut opts = PhpArray::new();
    opts.insert(goro_runtime::Key::from("ssl"), Value::Array(ssl));
    Value::Resource(ResRef::new(
        PhpStr::intern(b"stream-context"),
        Box::new(StreamContext {
            options: RefCell::new(opts),
            params: RefCell::new(PhpArray::new()),
        }),
    ))
}

/// The outcome of opening a connection.
pub enum Open {
    Ok(Box<Conn>),
    /// Name resolution failed: the message to warn.
    Lookup(String),
    /// The connection or greeting failed (no warning).
    Failed,
}

impl Conn {
    /// Connects and reads the greeting (220).
    pub fn open(vm: &mut Vm, host: &[u8], port: u16, timeout: Duration) -> Open {
        let addrs = match crate::builtins::sockets::resolve(host, port, false) {
            Ok(a) => a,
            Err(m) => return Open::Lookup(m),
        };
        for sa in addrs {
            let target = match sa {
                std::net::SocketAddr::V4(v) => format!("tcp://{}:{}", v.ip(), v.port()),
                std::net::SocketAddr::V6(v) => format!("tcp://[{}]:{}", v.ip(), v.port()),
            };
            let Ok(mut s) =
                crate::builtins::sockets::connect_client(target.as_bytes(), Some(timeout))
            else {
                continue;
            };
            s.timeout = Some(timeout);
            s.host = host.to_vec();
            let mut c = Box::new(Conn {
                ctl: s,
                inbuf: Vec::new(),
                timeout,
                autoseek: true,
                usepasvaddress: true,
                pasv: false,
                use_ssl: false,
                ssl_data: false,
                ty: None,
                code: 0,
                lines: Vec::new(),
                msg: Vec::new(),
                nb: None,
                closed: false,
            });
            if !c.getresp(vm) || c.code != 220 {
                return Open::Failed;
            }
            return Open::Ok(c);
        }
        Open::Failed
    }

    fn fill(&mut self, vm: &mut Vm) -> std::io::Result<usize> {
        let mut buf = [0u8; 8192];
        let n = self.ctl.read(vm, &mut buf)?;
        self.inbuf.extend_from_slice(&buf[..n]);
        Ok(n)
    }

    /// One line of the control connection, without its CRLF.
    fn line(&mut self, vm: &mut Vm) -> std::io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(p) = self.inbuf.iter().position(|&b| b == b'\n') {
                let mut l: Vec<u8> = self.inbuf.drain(..=p).collect();
                l.pop();
                if l.last() == Some(&b'\r') {
                    l.pop();
                }
                return Ok(Some(l));
            }
            if self.fill(vm)? == 0 {
                if self.inbuf.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(std::mem::take(&mut self.inbuf)));
            }
        }
    }

    fn set_error(&mut self, msg: &str) {
        self.code = 0;
        self.lines.clear();
        self.msg = msg.as_bytes().to_vec();
    }

    /// Reads a reply ("ddd text", or "ddd-" lines up to "ddd text"); false
    /// when the connection failed (the message is the error's text).
    pub fn getresp(&mut self, vm: &mut Vm) -> bool {
        self.lines.clear();
        let mut first: Option<[u8; 3]> = None;
        loop {
            let l = match self.line(vm) {
                Ok(Some(l)) => l,
                Ok(None) => {
                    self.set_error("Connection closed by the server");
                    return false;
                }
                Err(e) => {
                    let n =
                        e.raw_os_error()
                            .unwrap_or(if e.kind() == std::io::ErrorKind::TimedOut {
                                libc::ETIMEDOUT
                            } else {
                                libc::EIO
                            });
                    self.set_error(&crate::builtins::exec::strerror(n));
                    return false;
                }
            };
            let digits = l.len() >= 3 && l[..3].iter().all(|b| b.is_ascii_digit());
            match first {
                None => {
                    if l.is_empty() {
                        continue;
                    }
                    if !digits {
                        // Not a reply line: its text is the message (test
                        // bug80901.phpt).
                        self.code = 0;
                        self.msg = l.clone();
                        self.lines.push(l);
                        return true;
                    }
                    let code = [l[0], l[1], l[2]];
                    let multi = l.get(3) == Some(&b'-');
                    self.lines.push(l.clone());
                    if !multi {
                        return self.finish(code, &l);
                    }
                    first = Some(code);
                }
                Some(code) => {
                    self.lines.push(l.clone());
                    if digits && l[..3] == code && l.get(3) != Some(&b'-') {
                        return self.finish(code, &l);
                    }
                }
            }
        }
    }

    fn finish(&mut self, code: [u8; 3], l: &[u8]) -> bool {
        self.code = ((code[0] - b'0') as i32) * 100
            + ((code[1] - b'0') as i32) * 10
            + (code[2] - b'0') as i32;
        self.msg = l.get(4..).unwrap_or(b"").to_vec();
        true
    }

    /// Sends a command line (one write: test servers read a command per
    /// read).
    pub fn putcmd(&mut self, vm: &mut Vm, cmd: &str, arg: Option<&[u8]>) -> bool {
        let mut line = cmd.as_bytes().to_vec();
        if let Some(a) = arg {
            line.push(b' ');
            line.extend_from_slice(a);
        }
        line.extend_from_slice(b"\r\n");
        let mut done = 0;
        while done < line.len() {
            match self.ctl.write(vm, &line[done..]) {
                Ok(0) => {
                    self.set_error("Connection closed by the server");
                    return false;
                }
                Ok(n) => done += n,
                Err(e) => {
                    let n = e.raw_os_error().unwrap_or(libc::EIO);
                    self.set_error(&crate::builtins::exec::strerror(n));
                    return false;
                }
            }
        }
        true
    }

    /// Sends a command and reads its reply.
    pub fn cmd(&mut self, vm: &mut Vm, cmd: &str, arg: Option<&[u8]>) -> bool {
        self.putcmd(vm, cmd, arg) && self.getresp(vm)
    }

    /// TYPE A / TYPE I, sent when the type changes.
    pub fn set_type(&mut self, vm: &mut Vm, ty: Ty) -> bool {
        if self.ty == Some(ty) {
            return true;
        }
        let a: &[u8] = if ty == Ty::Ascii { b"A" } else { b"I" };
        if !self.cmd(vm, "TYPE", Some(a)) || self.code != 200 {
            return false;
        }
        self.ty = Some(ty);
        true
    }

    /// AUTH TLS (or AUTH SSL), the handshake, PBSZ 0 and PROT P (RFC
    /// 4217), before USER.
    pub fn start_tls(&mut self, vm: &mut Vm, a: &Args) -> R<bool> {
        if !self.cmd(vm, "AUTH", Some(b"TLS")) {
            return Ok(false);
        }
        if self.code != 234 && self.code != 334 {
            if !self.cmd(vm, "AUTH", Some(b"SSL")) {
                return Ok(false);
            }
            if self.code != 234 && self.code != 334 {
                return Ok(false);
            }
        }
        self.ctl.ctx = Some(no_verify_context());
        let r = crate::builtins::openssl::tls::enable(vm, a, &mut self.ctl, None, false)?;
        if r != crate::builtins::openssl::tls::Enabled::Yes {
            self.set_error("SSL/TLS handshake failed");
            return Ok(false);
        }
        if !self.cmd(vm, "PBSZ", Some(b"0")) {
            return Ok(false);
        }
        if !self.cmd(vm, "PROT", Some(b"P")) {
            return Ok(false);
        }
        self.ssl_data = self.code == 200;
        Ok(true)
    }

    /// USER/PASS.
    pub fn login(&mut self, vm: &mut Vm, a: &Args, user: &[u8], pass: &[u8]) -> R<bool> {
        if self.use_ssl && !self.start_tls(vm, a)? {
            return Ok(false);
        }
        if !self.cmd(vm, "USER", Some(user)) {
            return Ok(false);
        }
        if self.code == 230 {
            return Ok(true);
        }
        if self.code != 331 {
            return Ok(false);
        }
        if !self.cmd(vm, "PASS", Some(pass)) {
            return Ok(false);
        }
        Ok(self.code == 230)
    }

    /// A listening socket for PORT, on the control connection's address.
    fn port_listener(&mut self, vm: &mut Vm) -> Option<OwnedFd> {
        let fd = self.ctl.fd.as_raw_fd();
        // SAFETY: zeroed storage for getsockname.
        let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        // SAFETY: storage of the given length.
        if unsafe {
            libc::getsockname(
                fd,
                (&mut st as *mut libc::sockaddr_storage).cast(),
                &mut len,
            )
        } != 0
        {
            return None;
        }
        let family = st.ss_family as i32;
        // SAFETY: plain socket calls on a fresh descriptor.
        unsafe {
            let l = libc::socket(family, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
            if l < 0 {
                return None;
            }
            let lfd = OwnedFd::from_raw_fd(l);
            // port 0: any free port
            if family == libc::AF_INET {
                (*(&mut st as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in>()).sin_port =
                    0;
            } else {
                (*(&mut st as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in6>())
                    .sin6_port = 0;
            }
            if libc::bind(l, (&st as *const libc::sockaddr_storage).cast(), len) != 0
                || libc::listen(l, 1) != 0
            {
                return None;
            }
            let mut len2 = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            if libc::getsockname(
                l,
                (&mut st as *mut libc::sockaddr_storage).cast(),
                &mut len2,
            ) != 0
            {
                return None;
            }
            let arg = if family == libc::AF_INET {
                let sin = &*(&st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>();
                let ip = sin.sin_addr.s_addr.to_ne_bytes();
                let p = u16::from_be(sin.sin_port);
                format!(
                    "{},{},{},{},{},{}",
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3],
                    p >> 8,
                    p & 0xff
                )
            } else {
                let sin = &*(&st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>();
                format!(
                    "|2|{}|{}|",
                    std::net::Ipv6Addr::from(sin.sin6_addr.s6_addr),
                    u16::from_be(sin.sin6_port)
                )
            };
            let cmd = if family == libc::AF_INET {
                "PORT"
            } else {
                "EPRT"
            };
            if !self.cmd(vm, cmd, Some(arg.as_bytes())) || self.code != 200 {
                return None;
            }
            Some(lfd)
        }
    }

    /// PASV: the address the server listens on.
    fn pasv_addr(&mut self, vm: &mut Vm) -> Option<std::net::SocketAddr> {
        if !self.cmd(vm, "PASV", None) || self.code != 227 {
            return None;
        }
        let msg = self.msg.clone();
        let start = msg.iter().position(|b| b.is_ascii_digit())?;
        let nums: Vec<u32> = msg[start..]
            .split(|&b| b == b',' || b == b')' || b == b' ')
            .filter(|p| !p.is_empty())
            .take(6)
            .filter_map(|p| std::str::from_utf8(p).ok()?.parse().ok())
            .collect();
        if nums.len() != 6 || nums.iter().any(|&n| n > 255) {
            return None;
        }
        let port = (nums[4] << 8 | nums[5]) as u16;
        let ip = if self.usepasvaddress {
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                nums[0] as u8,
                nums[1] as u8,
                nums[2] as u8,
                nums[3] as u8,
            ))
        } else {
            peer_ip(self.ctl.fd.as_raw_fd())?
        };
        Some(std::net::SocketAddr::new(ip, port))
    }

    /// Starts a transfer: the data connection set up, `cmd` sent and its
    /// preliminary reply (1xx) read. None on failure (the reply explains).
    pub fn data_cmd(
        &mut self,
        vm: &mut Vm,
        a: &Args,
        cmd: &str,
        arg: Option<&[u8]>,
    ) -> R<Option<Sock>> {
        let data = if self.pasv {
            let Some(sa) = self.pasv_addr(vm) else {
                return Ok(None);
            };
            let target = match sa {
                std::net::SocketAddr::V4(v) => format!("tcp://{}:{}", v.ip(), v.port()),
                std::net::SocketAddr::V6(v) => format!("tcp://[{}]:{}", v.ip(), v.port()),
            };
            let Ok(s) =
                crate::builtins::sockets::connect_client(target.as_bytes(), Some(self.timeout))
            else {
                self.set_error("Unable to connect to the data port");
                return Ok(None);
            };
            if !self.cmd(vm, cmd, arg) || !(100..200).contains(&self.code) {
                return Ok(None);
            }
            s
        } else {
            let Some(l) = self.port_listener(vm) else {
                return Ok(None);
            };
            if !self.cmd(vm, cmd, arg) || !(100..200).contains(&self.code) {
                return Ok(None);
            }
            let mut p = libc::pollfd {
                fd: l.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ms = self.timeout.as_millis().min(i32::MAX as u128) as i32;
            // SAFETY: one pollfd.
            if unsafe { libc::poll(&mut p, 1, ms) } <= 0 {
                self.set_error("Connection timed out");
                return Ok(None);
            }
            // SAFETY: accept on our listener.
            let n = unsafe {
                libc::accept4(
                    l.as_raw_fd(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                )
            };
            if n < 0 {
                self.set_error(&crate::builtins::exec::strerror(
                    super::super::socketext::errno(),
                ));
                return Ok(None);
            }
            // SAFETY: a fresh descriptor.
            Sock::new(unsafe { OwnedFd::from_raw_fd(n) }, Kind::Tcp)
        };
        let mut data = data;
        data.timeout = Some(self.timeout);
        if self.ssl_data {
            data.ctx = Some(no_verify_context());
            data.host = self.ctl.host.clone();
            let r = crate::builtins::openssl::tls::enable(vm, a, &mut data, None, false)?;
            if r != crate::builtins::openssl::tls::Enabled::Yes {
                self.set_error("SSL/TLS handshake failed on the data connection");
                return Ok(None);
            }
        }
        Ok(Some(data))
    }

    /// Reads a whole data connection.
    pub fn read_all(vm: &mut Vm, data: &mut Sock) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 16384];
        loop {
            match data.read(vm, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        out
    }

    /// Closes a data connection (close_notify on TLS).
    pub fn close_data(vm: &mut Vm, mut data: Sock) {
        let _ = data.close(vm);
        // SAFETY: shutdown before the descriptor drops.
        unsafe { libc::shutdown(data.fd.as_raw_fd(), libc::SHUT_RDWR) };
    }

    /// A listing command (NLST, LIST, MLSD): the lines, None on failure.
    pub fn list(
        &mut self,
        vm: &mut Vm,
        a: &Args,
        cmd: &str,
        arg: Option<&[u8]>,
    ) -> R<Option<Vec<Vec<u8>>>> {
        if !self.set_type(vm, Ty::Ascii) {
            return Ok(None);
        }
        let Some(mut data) = self.data_cmd(vm, a, cmd, arg)? else {
            // A server with nothing to list may answer 226 without a data
            // connection (test bug39458-2.phpt).
            return Ok(if self.code == 226 {
                Some(Vec::new())
            } else {
                None
            });
        };
        let body = Self::read_all(vm, &mut data);
        Self::close_data(vm, data);
        if !self.getresp(vm) || !(self.code == 226 || self.code == 250) {
            return Ok(None);
        }
        let mut lines = Vec::new();
        let mut rest = &body[..];
        while !rest.is_empty() {
            match rest.windows(2).position(|w| w == b"\r\n") {
                Some(p) => {
                    lines.push(rest[..p].to_vec());
                    rest = &rest[p + 2..];
                }
                None => {
                    lines.push(rest.to_vec());
                    break;
                }
            }
        }
        Ok(Some(lines))
    }

    /// Ends an unfinished non-blocking transfer (its final reply read).
    pub fn abort_nb(&mut self, vm: &mut Vm) {
        if let Some(nb) = self.nb.take() {
            Self::close_data(vm, nb.data);
            self.getresp(vm);
        }
    }

    /// QUIT and close; false when the server didn't answer (test
    /// 004.phpt: it hung up after a failed login).
    pub fn quit(&mut self, vm: &mut Vm) -> bool {
        if self.closed {
            return true;
        }
        self.abort_nb(vm);
        let ok = self.putcmd(vm, "QUIT", None) && self.getresp(vm);
        let _ = self.ctl.close(vm);
        self.closed = true;
        ok
    }
}

fn peer_ip(fd: i32) -> Option<std::net::IpAddr> {
    // SAFETY: zeroed storage for getpeername.
    let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    // SAFETY: storage of the given length.
    if unsafe {
        libc::getpeername(
            fd,
            (&mut st as *mut libc::sockaddr_storage).cast(),
            &mut len,
        )
    } != 0
    {
        return None;
    }
    match st.ss_family as i32 {
        libc::AF_INET => {
            // SAFETY: an AF_INET address.
            let sin =
                unsafe { &*(&st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>() };
            Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(
                sin.sin_addr.s_addr.to_ne_bytes(),
            )))
        }
        libc::AF_INET6 => {
            // SAFETY: an AF_INET6 address.
            let sin =
                unsafe { &*(&st as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>() };
            Some(std::net::IpAddr::V6(std::net::Ipv6Addr::from(
                sin.sin6_addr.s6_addr,
            )))
        }
        _ => None,
    }
}

/// ASCII downloads: CRLF becomes LF (RFC 959's NVT-ASCII lines to local
/// lines). `cr` carries a CR from the end of the previous chunk.
pub fn from_ascii(input: &[u8], cr: &mut bool, last: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for &b in input {
        if *cr {
            *cr = false;
            if b == b'\n' {
                out.push(b'\n');
                continue;
            }
            out.push(b'\r');
        }
        if b == b'\r' {
            *cr = true;
        } else {
            out.push(b);
        }
    }
    if last && *cr {
        *cr = false;
        out.push(b'\r');
    }
    out
}

/// ASCII uploads: local LF line ends become CRLF (an existing CRLF stays).
pub fn to_ascii(input: &[u8], cr: &mut bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + input.len() / 16);
    for &b in input {
        if b == b'\n' && !*cr {
            out.push(b'\r');
        }
        out.push(b);
        *cr = b == b'\r';
    }
    out
}
