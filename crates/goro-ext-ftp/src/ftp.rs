//! A port of php-src's ext/ftp/ftp.c: the FTP protocol client.

use std::ffi::{CStr, CString};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::crypto::{CryptoSetup, CryptoStream};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::streams;

pub const FTP_BUFSIZE: usize = 4096;
pub const FTP_DEFAULT_TIMEOUT: i64 = 90;

pub const PHP_FTP_FAILED: i64 = 0;
pub const PHP_FTP_FINISHED: i64 = 1;
pub const PHP_FTP_MOREDATA: i64 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FtpType {
    /// (not set yet)
    None,
    Ascii,
    Image,
}

impl FtpType {
    pub fn from_mode(mode: i64) -> Option<FtpType> {
        match mode {
            1 => Some(FtpType::Ascii),
            2 => Some(FtpType::Image),
            _ => None,
        }
    }
}

/// databuf_t
pub struct Data {
    listener: i32,
    pub fd: i32,
    buf: Box<[u8; FTP_BUFSIZE + 1]>,
    crypto: Option<Box<dyn CryptoStream>>,
}

/// ftpbuf_t
pub struct Ftp {
    pub fd: i32,
    localaddr: libc::sockaddr_storage,
    pub resp: i32,
    /// the last response text (NUL-terminated)
    pub inbuf: Box<[u8; FTP_BUFSIZE + 1]>,
    extra: Option<usize>,
    extralen: usize,
    pwd: Option<Vec<u8>>,
    syst: Option<Vec<u8>>,
    typ: FtpType,
    pasv: i32,
    pasvaddr: libc::sockaddr_storage,
    pub timeout_sec: i64,
    pub autoseek: bool,
    pub usepasvaddress: bool,
    pub data: Option<Box<Data>>,
    /// output/input stream of a "nonblocking" transfer
    pub stream: Option<i64>,
    pub nb: bool,
    lastch: u8,
    /// recv = false / send = true
    pub direction: bool,
    pub closestream: bool,
    pub use_ssl: bool,
    use_ssl_for_data: bool,
    old_ssl: bool,
    ssl_active: bool,
    crypto: Option<Box<dyn CryptoStream>>,
    /// The PHP function warnings are reported for.
    pub func: &'static str,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn set_errno(e: i32) {
    unsafe {
        *libc::__errno_location() = e;
    }
}

pub fn strerror(e: i32) -> String {
    unsafe { CStr::from_ptr(libc::strerror(e)) }
        .to_string_lossy()
        .into_owned()
}

/// The C string at the start of `b`.
pub fn cstr(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..end]
}

fn warn(vm: &mut Vm, func: &str, msg: &str) {
    vm.emit_warning(&format!("{}(): {}", func, msg));
}

fn close_fd(fd: i32) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}

/// my_poll(): poll() for `timeout` ms, restarted (with what is left) when
/// interrupted.
fn my_poll(fd: i32, events: i16, timeout: i32) -> i32 {
    let mut timeout_ns: i128 = timeout as i128 * 1_000_000;
    loop {
        let start = std::time::Instant::now();
        let mut p = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let ms = (timeout_ns / 1_000_000) as i32;
        let n = unsafe { libc::poll(&mut p, 1, ms) };
        if n == -1 && errno() == libc::EINTR {
            let delta = start.elapsed().as_nanos() as i128;
            if delta > timeout_ns || delta == 0 {
                set_errno(libc::ETIMEDOUT);
                return n;
            }
            timeout_ns -= delta;
            continue;
        }
        if n > 0 && p.revents & libc::POLLNVAL != 0 {
            // php_pollfd_for(): an invalid descriptor
            set_errno(libc::EBADF);
            return -1;
        }
        return n;
    }
}

const POLL_READABLE: i16 = libc::POLLIN | libc::POLLERR | libc::POLLHUP;

fn sockaddr_size(sa: &libc::sockaddr_storage) -> libc::socklen_t {
    match sa.ss_family as i32 {
        libc::AF_INET6 => std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
        _ => std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
    }
}

/// php_connect_nonb(): 0 or -1 (with errno).
fn connect_nonb(
    fd: i32,
    sa: &libc::sockaddr_storage,
    len: libc::socklen_t,
    timeout_sec: i64,
) -> i32 {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        let r = libc::connect(fd, sa as *const _ as *const libc::sockaddr, len);
        let mut error = 0;
        if r != 0 {
            if errno() != libc::EINPROGRESS {
                return -1;
            }
            let n = my_poll(
                fd,
                libc::POLLOUT | libc::POLLERR | libc::POLLHUP,
                (timeout_sec.saturating_mul(1000)).min(i32::MAX as i64) as i32,
            );
            if n == 0 {
                set_errno(libc::ETIMEDOUT);
                libc::fcntl(fd, libc::F_SETFL, flags);
                return -1;
            }
            if n < 0 {
                libc::fcntl(fd, libc::F_SETFL, flags);
                return -1;
            }
            let mut len = std::mem::size_of::<i32>() as libc::socklen_t;
            if libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                &mut error as *mut i32 as *mut libc::c_void,
                &mut len,
            ) != 0
            {
                error = errno();
            }
        }
        libc::fcntl(fd, libc::F_SETFL, flags);
        if error != 0 {
            set_errno(error);
            return -1;
        }
    }
    0
}

fn ipv6_works() -> bool {
    let s = unsafe { libc::socket(libc::PF_INET6, libc::SOCK_DGRAM, 0) };
    if s < 0 {
        return false;
    }
    close_fd(s);
    true
}

/// php_network_connect_socket_to_host() (no error string: only the
/// resolver failure warns).
fn connect_to_host(vm: &mut Vm, func: &str, host: &[u8], port: u16, timeout_sec: i64) -> i32 {
    let Ok(chost) = CString::new(host.to_vec()) else {
        return -1;
    };
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    hints.ai_family = if ipv6_works() {
        libc::AF_UNSPEC
    } else {
        libc::AF_INET
    };
    hints.ai_socktype = libc::SOCK_STREAM;
    let mut res: *mut libc::addrinfo = std::ptr::null_mut();
    let n = unsafe { libc::getaddrinfo(chost.as_ptr(), std::ptr::null(), &hints, &mut res) };
    if n != 0 || res.is_null() {
        let msg = if n != 0 {
            unsafe { CStr::from_ptr(libc::gai_strerror(n)) }
                .to_string_lossy()
                .into_owned()
        } else {
            "No address associated with hostname".to_string()
        };
        warn(
            vm,
            func,
            &format!(
                "php_network_getaddresses: getaddrinfo for {} failed: {}",
                String::from_utf8_lossy(host),
                msg
            ),
        );
        return -1;
    }
    let mut addrs = Vec::new();
    let mut p = res;
    while !p.is_null() {
        let ai = unsafe { &*p };
        let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let len = (ai.ai_addrlen as usize).min(std::mem::size_of::<libc::sockaddr_storage>());
        unsafe {
            std::ptr::copy_nonoverlapping(
                ai.ai_addr as *const u8,
                &mut ss as *mut _ as *mut u8,
                len,
            );
        }
        addrs.push(ss);
        p = ai.ai_next;
    }
    unsafe { libc::freeaddrinfo(res) };
    for mut sa in addrs {
        match sa.ss_family as i32 {
            libc::AF_INET => {
                let sin = unsafe { &mut *(&mut sa as *mut _ as *mut libc::sockaddr_in) };
                sin.sin_port = port.to_be();
            }
            libc::AF_INET6 => {
                let sin6 = unsafe { &mut *(&mut sa as *mut _ as *mut libc::sockaddr_in6) };
                sin6.sin6_port = port.to_be();
            }
            _ => continue,
        }
        let fd = unsafe { libc::socket(sa.ss_family as i32, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            continue;
        }
        if connect_nonb(fd, &sa, sockaddr_size(&sa), timeout_sec) == 0 {
            return fd;
        }
        close_fd(fd);
    }
    -1
}

/// The TLS setup of a client connection (SSL_CTX_new(SSLv23_client_method()),
/// without peer verification).
fn new_crypto(vm: &mut Vm, func: &str) -> Option<Box<dyn CryptoStream>> {
    let hooks = vm.crypto_hooks?;
    let opts = crypto_opts();
    let setup = CryptoSetup {
        is_client: true,
        // TLS 1.0 to 1.3
        method: 8 | 16 | 32 | 64,
        url_name: None,
        opts: Some(&opts),
        blocked: true,
        func,
    };
    (hooks.setup)(vm, &setup)
}

fn crypto_opts() -> PhpArray {
    let mut o = PhpArray::new();
    for k in ["verify_peer", "verify_peer_name", "SNI_enabled"] {
        o.set(
            ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
            Value::False,
        );
    }
    o.set(
        ArrayKey::String(PhpString::from_bytes(b"allow_self_signed")),
        Value::True,
    );
    o
}

/// SSL_connect() on `fd`: false after the "handshake failed" warning.
fn handshake(vm: &mut Vm, func: &str, c: &mut Box<dyn CryptoStream>, fd: i32, what: &str) -> bool {
    let opts = crypto_opts();
    let saved = vm.error_reporting;
    vm.error_reporting = 0;
    let r = c.enable(vm, func, fd, true, -1, Some(&opts));
    vm.error_reporting = saved;
    if r.result != 1 {
        warn(vm, func, what);
        return false;
    }
    true
}

impl Ftp {
    /// ftp_open()
    pub fn open(
        vm: &mut Vm,
        func: &'static str,
        host: &[u8],
        port: u16,
        timeout_sec: i64,
    ) -> Option<Box<Ftp>> {
        let fd = connect_to_host(
            vm,
            func,
            host,
            if port != 0 { port } else { 21 },
            timeout_sec,
        );
        if fd == -1 {
            return None;
        }
        let mut ftp = Box::new(Ftp {
            fd,
            localaddr: unsafe { std::mem::zeroed() },
            resp: 0,
            inbuf: Box::new([0; FTP_BUFSIZE + 1]),
            extra: None,
            extralen: 0,
            pwd: None,
            syst: None,
            typ: FtpType::None,
            pasv: 0,
            pasvaddr: unsafe { std::mem::zeroed() },
            timeout_sec,
            autoseek: true,
            usepasvaddress: true,
            data: None,
            stream: None,
            nb: false,
            lastch: 0,
            direction: false,
            closestream: false,
            use_ssl: false,
            use_ssl_for_data: false,
            old_ssl: false,
            ssl_active: false,
            crypto: None,
            func,
        });
        let mut size = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if unsafe {
            libc::getsockname(
                fd,
                &mut ftp.localaddr as *mut _ as *mut libc::sockaddr,
                &mut size,
            )
        } != 0
        {
            let e = errno();
            warn(
                vm,
                func,
                &format!("getsockname failed: {} ({})", strerror(e), e),
            );
            return None;
        }
        if !ftp.getresp(vm) || ftp.resp != 220 {
            return None;
        }
        Some(ftp)
    }

    /// The response text as a C string.
    pub fn inbuf_str(&self) -> &[u8] {
        cstr(&self.inbuf[..])
    }

    /// ftp_close() without the stream (closed by the caller, which has the
    /// VM): the data and control connections.
    pub fn close_connections(&mut self, vm: Option<&mut Vm>) {
        self.data_close();
        if self.fd != -1 {
            if self.ssl_active {
                let mut c = self.crypto.take();
                if let Some(c) = c.as_mut() {
                    ssl_shutdown(vm, self.func, self.fd, c);
                }
                self.ssl_active = false;
            }
            close_fd(self.fd);
            self.fd = -1;
        }
        self.pwd = None;
        self.syst = None;
    }

    /// ftp_quit()
    pub fn quit(&mut self, vm: &mut Vm) -> bool {
        if !self.putcmd(vm, b"QUIT", None) {
            return false;
        }
        if !self.getresp(vm) || self.resp != 221 {
            return false;
        }
        self.pwd = None;
        true
    }

    /// ftp_login()
    pub fn login(&mut self, vm: &mut Vm, user: &[u8], pass: &[u8]) -> bool {
        if self.use_ssl && !self.ssl_active {
            if !self.putcmd(vm, b"AUTH", Some(b"TLS")) {
                return false;
            }
            if !self.getresp(vm) {
                return false;
            }
            if self.resp != 234 {
                if !self.putcmd(vm, b"AUTH", Some(b"SSL")) {
                    return false;
                }
                if !self.getresp(vm) {
                    return false;
                }
                if self.resp != 334 {
                    return false;
                }
                self.old_ssl = true;
                self.use_ssl_for_data = true;
            }
            let Some(mut c) = new_crypto(vm, self.func) else {
                warn(vm, self.func, "Failed to create the SSL context");
                return false;
            };
            if !handshake(vm, self.func, &mut c, self.fd, "SSL/TLS handshake failed") {
                return false;
            }
            self.crypto = Some(c);
            self.ssl_active = true;
            if !self.old_ssl {
                if !self.putcmd(vm, b"PBSZ", Some(b"0")) {
                    return false;
                }
                if !self.getresp(vm) {
                    return false;
                }
                if !self.putcmd(vm, b"PROT", Some(b"P")) {
                    return false;
                }
                if !self.getresp(vm) {
                    return false;
                }
                self.use_ssl_for_data = (200..=299).contains(&self.resp);
            }
        }
        if !self.putcmd(vm, b"USER", Some(user)) {
            return false;
        }
        if !self.getresp(vm) {
            return false;
        }
        if self.resp == 230 {
            return true;
        }
        if self.resp != 331 {
            return false;
        }
        if !self.putcmd(vm, b"PASS", Some(pass)) {
            return false;
        }
        if !self.getresp(vm) {
            return false;
        }
        self.resp == 230
    }

    /// ftp_syst()
    pub fn syst(&mut self, vm: &mut Vm) -> Option<Vec<u8>> {
        if let Some(s) = &self.syst {
            return Some(s.clone());
        }
        if !self.putcmd(vm, b"SYST", None) {
            return None;
        }
        if !self.getresp(vm) || self.resp != 215 {
            return None;
        }
        let text = self.inbuf_str();
        let start = text.iter().position(|&c| c != b' ').unwrap_or(text.len());
        let rest = &text[start..];
        let end = rest.iter().position(|&c| c == b' ').unwrap_or(rest.len());
        let s = rest[..end].to_vec();
        self.syst = Some(s.clone());
        Some(s)
    }

    /// ftp_pwd()
    pub fn pwd(&mut self, vm: &mut Vm) -> Option<Vec<u8>> {
        if let Some(p) = &self.pwd {
            return Some(p.clone());
        }
        if !self.putcmd(vm, b"PWD", None) {
            return None;
        }
        if !self.getresp(vm) || self.resp != 257 {
            return None;
        }
        let text = self.inbuf_str();
        let a = text.iter().position(|&c| c == b'"')? + 1;
        let b = text[a..].iter().rposition(|&c| c == b'"')? + a;
        let p = text[a..b].to_vec();
        self.pwd = Some(p.clone());
        Some(p)
    }

    /// A command and a response code (ftp_exec(), ftp_rmdir(), ...).
    fn simple(&mut self, vm: &mut Vm, cmd: &[u8], args: Option<&[u8]>, ok: &[i32]) -> bool {
        if !self.putcmd(vm, cmd, args) {
            return false;
        }
        self.getresp(vm) && ok.contains(&self.resp)
    }

    pub fn exec(&mut self, vm: &mut Vm, cmd: &[u8]) -> bool {
        self.simple(vm, b"SITE EXEC", Some(cmd), &[200])
    }

    /// ftp_raw()
    pub fn raw(&mut self, vm: &mut Vm, cmd: &[u8]) -> Option<PhpArray> {
        if !self.putcmd(vm, cmd, None) {
            return None;
        }
        let mut a = PhpArray::new();
        while self.readline(vm) {
            let line = self.inbuf_str().to_vec();
            a.push(Value::String(PhpString::from_vec(line)));
            let b = &self.inbuf;
            if b[0].is_ascii_digit()
                && b[1].is_ascii_digit()
                && b[2].is_ascii_digit()
                && b[3] == b' '
            {
                break;
            }
        }
        Some(a)
    }

    pub fn chdir(&mut self, vm: &mut Vm, dir: &[u8]) -> bool {
        self.pwd = None;
        self.simple(vm, b"CWD", Some(dir), &[250])
    }

    pub fn cdup(&mut self, vm: &mut Vm) -> bool {
        self.pwd = None;
        self.simple(vm, b"CDUP", None, &[250])
    }

    /// ftp_mkdir()
    pub fn mkdir(&mut self, vm: &mut Vm, dir: &[u8]) -> Option<Vec<u8>> {
        if !self.putcmd(vm, b"MKD", Some(dir)) {
            return None;
        }
        if !self.getresp(vm) || self.resp != 257 {
            return None;
        }
        let text = self.inbuf_str();
        let Some(a) = text.iter().position(|&c| c == b'"') else {
            return Some(dir.to_vec());
        };
        let a = a + 1;
        let b = text[a..].iter().rposition(|&c| c == b'"')? + a;
        Some(text[a..b].to_vec())
    }

    pub fn rmdir(&mut self, vm: &mut Vm, dir: &[u8]) -> bool {
        self.simple(vm, b"RMD", Some(dir), &[250])
    }

    /// ftp_chmod()
    pub fn chmod(&mut self, vm: &mut Vm, mode: i32, filename: &[u8]) -> bool {
        if filename.is_empty() {
            return false;
        }
        let mut buf = format!("CHMOD {:o} ", mode as u32).into_bytes();
        buf.extend_from_slice(cstr(filename));
        self.simple(vm, b"SITE", Some(&buf), &[200])
    }

    /// ftp_alloc(): (success, the response when one was read)
    pub fn alloc(&mut self, vm: &mut Vm, size: i64) -> (bool, Option<Vec<u8>>) {
        if size <= 0 {
            return (false, None);
        }
        if !self.putcmd(vm, b"ALLO", Some(size.to_string().as_bytes())) {
            return (false, None);
        }
        if !self.getresp(vm) {
            return (false, None);
        }
        let r = self.inbuf_str().to_vec();
        ((200..300).contains(&self.resp), Some(r))
    }

    /// ftp_type()
    fn set_type(&mut self, vm: &mut Vm, typ: FtpType) -> bool {
        if typ == self.typ {
            return true;
        }
        let c: &[u8] = match typ {
            FtpType::Ascii => b"A",
            FtpType::Image => b"I",
            FtpType::None => return false,
        };
        if !self.putcmd(vm, b"TYPE", Some(c)) {
            return false;
        }
        if !self.getresp(vm) || self.resp != 200 {
            return false;
        }
        self.typ = typ;
        true
    }

    /// ftp_pasv()
    pub fn set_pasv(&mut self, vm: &mut Vm, pasv: bool) -> bool {
        if pasv && self.pasv == 2 {
            return true;
        }
        self.pasv = 0;
        if !pasv {
            return true;
        }
        self.pasvaddr = unsafe { std::mem::zeroed() };
        let mut n = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if unsafe {
            libc::getpeername(
                self.fd,
                &mut self.pasvaddr as *mut _ as *mut libc::sockaddr,
                &mut n,
            )
        } < 0
        {
            return false;
        }
        if self.pasvaddr.ss_family as i32 == libc::AF_INET6 {
            // try EPSV first
            if !self.putcmd(vm, b"EPSV", None) {
                return false;
            }
            if !self.getresp(vm) {
                return false;
            }
            if self.resp == 229 {
                let text = self.inbuf_str().to_vec();
                let Some(open) = text.iter().position(|&c| c == b'(') else {
                    return false;
                };
                let mut i = open + 1;
                let Some(&delim) = text.get(i) else {
                    return false;
                };
                let mut k = 0;
                while i < text.len() && k < 3 {
                    if text[i] == delim {
                        k += 1;
                    }
                    i += 1;
                }
                let (port, end) = strtoul(&text, i);
                if end == i || text.get(end) != Some(&delim) {
                    return false;
                }
                let sin6 =
                    unsafe { &mut *(&mut self.pasvaddr as *mut _ as *mut libc::sockaddr_in6) };
                sin6.sin6_port = (port as u16).to_be();
                self.pasv = 2;
                return true;
            }
        }
        // fall back to PASV
        if !self.putcmd(vm, b"PASV", None) {
            return false;
        }
        if !self.getresp(vm) || self.resp != 227 {
            return false;
        }
        let text = self.inbuf_str().to_vec();
        let start = text
            .iter()
            .position(|c| c.is_ascii_digit())
            .unwrap_or(text.len());
        let Some(b) = scan_six(&text[start..]) else {
            return false;
        };
        let c: Vec<u8> = b.iter().map(|&v| v as u8).collect();
        let sin = unsafe { &mut *(&mut self.pasvaddr as *mut _ as *mut libc::sockaddr_in) };
        if self.usepasvaddress {
            sin.sin_addr.s_addr = u32::from_ne_bytes([c[0], c[1], c[2], c[3]]);
        }
        sin.sin_port = u16::from_ne_bytes([c[4], c[5]]);
        self.pasv = 2;
        true
    }

    /// ftp_get()
    pub fn get(
        &mut self,
        vm: &mut Vm,
        outstream: i64,
        path: &[u8],
        typ: FtpType,
        resumepos: i64,
    ) -> bool {
        let ok = self.get_inner(vm, outstream, path, typ, resumepos);
        if !ok {
            self.data_close();
        }
        ok
    }

    fn rest_and(&mut self, vm: &mut Vm, pos: i64, cmd: &[u8], path: &[u8]) -> bool {
        if pos > 0 {
            if !self.putcmd(vm, b"REST", Some(pos.to_string().as_bytes())) {
                return false;
            }
            if !self.getresp(vm) || self.resp != 350 {
                return false;
            }
        }
        if !self.putcmd(vm, cmd, Some(path)) {
            return false;
        }
        self.getresp(vm) && (self.resp == 150 || self.resp == 125)
    }

    fn get_inner(
        &mut self,
        vm: &mut Vm,
        outstream: i64,
        path: &[u8],
        typ: FtpType,
        resumepos: i64,
    ) -> bool {
        if !self.set_type(vm, typ) {
            return false;
        }
        if !self.getdata(vm) {
            return false;
        }
        if !self.rest_and(vm, resumepos, b"RETR", path) {
            return false;
        }
        if !self.data_accept(vm) {
            return false;
        }
        loop {
            let rcvd = self.data_recv(vm);
            if rcvd == 0 {
                break;
            }
            if rcvd < 0 {
                return false;
            }
            let rcvd = rcvd as usize;
            let buf = self.data.as_ref().unwrap().buf.clone();
            if typ == FtpType::Ascii {
                // "\r\n" -> "\n"; a lone "\r" is dropped
                let mut out = Vec::with_capacity(rcvd);
                let mut ptr = 0;
                let e = rcvd;
                while e > ptr {
                    let Some(off) = buf[ptr..e].iter().position(|&c| c == b'\r') else {
                        break;
                    };
                    let s = ptr + off;
                    out.extend_from_slice(&buf[ptr..s]);
                    let mut s = s;
                    if buf[s + 1] == b'\n' {
                        s += 1;
                        out.push(b'\n');
                    }
                    ptr = s + 1;
                }
                if ptr < e {
                    out.extend_from_slice(&buf[ptr..e]);
                }
                streams::write(vm, outstream, &out);
            } else if rcvd != streams::write(vm, outstream, &buf[..rcvd]) {
                return false;
            }
        }
        self.data_close();
        self.getresp(vm) && (self.resp == 226 || self.resp == 250)
    }

    /// ftp_send_stream_to_data_socket()
    fn send_stream(&mut self, vm: &mut Vm, instream: i64, typ: FtpType, once: bool) -> bool {
        let dfd = self.data.as_ref().map(|d| d.fd).unwrap_or(-1);
        if typ == FtpType::Ascii {
            let mut buf: Vec<u8> = Vec::with_capacity(FTP_BUFSIZE);
            while !streams::eof(vm, instream) {
                let room = FTP_BUFSIZE - buf.len();
                let Some(line) = streams::get_line(vm, instream, room) else {
                    break;
                };
                if line.is_empty() {
                    break;
                }
                buf.extend_from_slice(&line);
                // Replace \n with \r\n
                if buf.last() == Some(&b'\n') {
                    let l = buf.len();
                    buf[l - 1] = b'\r';
                    buf.push(b'\n');
                }
                if FTP_BUFSIZE - buf.len().min(FTP_BUFSIZE) < 2 {
                    if self.my_send(vm, dfd, &buf) != buf.len() as i64 {
                        return false;
                    }
                    buf.clear();
                    if once {
                        break;
                    }
                }
            }
            if !buf.is_empty() && self.my_send(vm, dfd, &buf) != buf.len() as i64 {
                return false;
            }
        } else {
            while !streams::eof(vm, instream) {
                let Some(chunk) = streams::read(vm, instream, FTP_BUFSIZE) else {
                    return false;
                };
                if chunk.is_empty() {
                    break;
                }
                if self.my_send(vm, dfd, &chunk) != chunk.len() as i64 {
                    return false;
                }
                if once {
                    break;
                }
            }
        }
        true
    }

    /// ftp_put() / ftp_append()
    pub fn put(
        &mut self,
        vm: &mut Vm,
        path: &[u8],
        instream: i64,
        typ: FtpType,
        startpos: i64,
        append: bool,
    ) -> bool {
        let ok = self.put_inner(vm, path, instream, typ, startpos, append);
        if !ok {
            self.data_close();
        }
        ok
    }

    fn put_inner(
        &mut self,
        vm: &mut Vm,
        path: &[u8],
        instream: i64,
        typ: FtpType,
        startpos: i64,
        append: bool,
    ) -> bool {
        if !self.set_type(vm, typ) {
            return false;
        }
        if !self.getdata(vm) {
            return false;
        }
        let cmd: &[u8] = if append { b"APPE" } else { b"STOR" };
        if !self.rest_and(vm, if append { 0 } else { startpos }, cmd, path) {
            return false;
        }
        if !self.data_accept(vm) {
            return false;
        }
        if !self.send_stream(vm, instream, typ, false) {
            return false;
        }
        self.data_close();
        self.getresp(vm) && matches!(self.resp, 226 | 250 | 200)
    }

    /// ftp_size()
    pub fn size(&mut self, vm: &mut Vm, path: &[u8]) -> i64 {
        if !self.set_type(vm, FtpType::Image) {
            return -1;
        }
        if !self.putcmd(vm, b"SIZE", Some(path)) {
            return -1;
        }
        if !self.getresp(vm) || self.resp != 213 {
            return -1;
        }
        strtol(self.inbuf_str())
    }

    /// ftp_mdtm()
    pub fn mdtm(&mut self, vm: &mut Vm, path: &[u8]) -> i64 {
        if !self.putcmd(vm, b"MDTM", Some(path)) {
            return -1;
        }
        if !self.getresp(vm) || self.resp != 213 {
            return -1;
        }
        let text = self.inbuf_str();
        let start = text
            .iter()
            .position(|c| c.is_ascii_digit())
            .unwrap_or(text.len());
        let Some(f) = scan_digits(&text[start..], &[4, 2, 2, 2, 2, 2]) else {
            return -1;
        };
        unsafe {
            let mut tm: libc::tm = std::mem::zeroed();
            tm.tm_year = f[0] - 1900;
            tm.tm_mon = f[1] - 1;
            tm.tm_mday = f[2];
            tm.tm_hour = f[3];
            tm.tm_min = f[4];
            tm.tm_sec = f[5];
            tm.tm_isdst = -1;
            // figure out the GMT offset
            let stamp = libc::time(std::ptr::null_mut());
            let mut gmt: libc::tm = std::mem::zeroed();
            if libc::gmtime_r(&stamp, &mut gmt).is_null() {
                return -1;
            }
            gmt.tm_isdst = -1;
            tm.tm_sec += (stamp - libc::mktime(&mut gmt)) as i32;
            tm.tm_isdst = gmt.tm_isdst;
            libc::mktime(&mut tm) as i64
        }
    }

    pub fn delete(&mut self, vm: &mut Vm, path: &[u8]) -> bool {
        self.simple(vm, b"DELE", Some(path), &[250])
    }

    /// ftp_rename()
    pub fn rename(&mut self, vm: &mut Vm, src: &[u8], dest: &[u8]) -> bool {
        self.simple(vm, b"RNFR", Some(src), &[350]) && self.simple(vm, b"RNTO", Some(dest), &[250])
    }

    /// ftp_site()
    pub fn site(&mut self, vm: &mut Vm, cmd: &[u8]) -> bool {
        if !self.putcmd(vm, b"SITE", Some(cmd)) {
            return false;
        }
        self.getresp(vm) && (200..300).contains(&self.resp)
    }

    /// ftp_putcmd(): "cmd args\r\n" (args as a C string).
    fn putcmd(&mut self, vm: &mut Vm, cmd: &[u8], args: Option<&[u8]>) -> bool {
        let c = cstr(cmd);
        if c.iter().any(|&b| b == b'\r' || b == b'\n') {
            return false;
        }
        let mut data = Vec::new();
        match args {
            Some(a) if !a.is_empty() && a[0] != 0 => {
                if cmd.len() + a.len() + 4 > FTP_BUFSIZE {
                    return false;
                }
                let a = cstr(a);
                if a.iter().any(|&b| b == b'\r' || b == b'\n') {
                    return false;
                }
                data.extend_from_slice(c);
                data.push(b' ');
                data.extend_from_slice(a);
            }
            _ => {
                if cmd.len() + 3 > FTP_BUFSIZE {
                    return false;
                }
                data.extend_from_slice(c);
            }
        }
        data.extend_from_slice(b"\r\n");
        // Clear the inbuf and extra-lines buffer
        self.inbuf[0] = 0;
        self.extra = None;
        self.my_send(vm, self.fd, &data) == data.len() as i64
    }

    /// ftp_readline()
    fn readline(&mut self, vm: &mut Vm) -> bool {
        let mut size: isize = FTP_BUFSIZE as isize;
        let mut rcvd: isize = 0;
        if let Some(e) = self.extra {
            let l = self.extralen;
            self.inbuf.copy_within(e..e + l, 0);
            rcvd = l as isize;
        }
        let mut data: usize = 0;
        loop {
            size -= rcvd;
            let mut eol = data;
            while rcvd > 0 {
                if self.inbuf[eol] == b'\r' {
                    self.inbuf[eol] = 0;
                    let mut extra = eol + 1;
                    if rcvd > 1 && self.inbuf[eol + 1] == b'\n' {
                        extra += 1;
                        rcvd -= 1;
                    }
                    rcvd -= 1;
                    self.extralen = rcvd as usize;
                    self.extra = if rcvd == 0 { None } else { Some(extra) };
                    return true;
                } else if self.inbuf[eol] == b'\n' {
                    self.inbuf[eol] = 0;
                    rcvd -= 1;
                    self.extralen = rcvd as usize;
                    self.extra = if rcvd == 0 { None } else { Some(eol + 1) };
                    return true;
                }
                rcvd -= 1;
                eol += 1;
            }
            data = eol;
            let n = size.max(0) as usize;
            let mut tmp = vec![0u8; n];
            rcvd = self.my_recv(vm, self.fd, &mut tmp, true) as isize;
            if rcvd < 1 {
                self.inbuf[data] = 0;
                return false;
            }
            self.inbuf[data..data + rcvd as usize].copy_from_slice(&tmp[..rcvd as usize]);
            if size == 0 {
                break;
            }
        }
        self.inbuf[data] = 0;
        false
    }

    /// ftp_getresp()
    fn getresp(&mut self, vm: &mut Vm) -> bool {
        self.resp = 0;
        loop {
            if !self.readline(vm) {
                return false;
            }
            let b = &self.inbuf;
            if b[0].is_ascii_digit()
                && b[1].is_ascii_digit()
                && b[2].is_ascii_digit()
                && b[3] == b' '
            {
                break;
            }
        }
        let b = &self.inbuf;
        self.resp = 100 * (b[0] - b'0') as i32 + 10 * (b[1] - b'0') as i32 + (b[2] - b'0') as i32;
        self.inbuf.copy_within(4..FTP_BUFSIZE, 0);
        if let Some(e) = self.extra {
            self.extra = Some(e.saturating_sub(4));
        }
        true
    }

    /// The TLS layer of descriptor `s` (control or data connection), if any.
    fn crypto_for(&mut self, s: i32) -> Option<&mut Box<dyn CryptoStream>> {
        if !self.use_ssl {
            return None;
        }
        if s == self.fd {
            if self.ssl_active {
                return self.crypto.as_mut();
            }
            return None;
        }
        if self.use_ssl_for_data {
            if let Some(d) = self.data.as_mut() {
                if d.fd == s {
                    return d.crypto.as_mut();
                }
            }
        }
        None
    }

    /// my_send(): all of `buf`, or -1 after a warning.
    fn my_send(&mut self, vm: &mut Vm, s: i32, buf: &[u8]) -> i64 {
        let mut off = 0;
        while off < buf.len() {
            let n = my_poll(
                s,
                libc::POLLOUT,
                (self.timeout_sec.wrapping_mul(1000)) as i32,
            );
            if n < 1 {
                if n == 0 {
                    set_errno(libc::ETIMEDOUT);
                }
                let e = errno();
                warn(vm, self.func, &strerror(e));
                return -1;
            }
            let func = self.func;
            let sent: isize = match self.crypto_for(s) {
                Some(c) => match c.write(s, &buf[off..], true, -1) {
                    Ok(k) => k as isize,
                    Err(_) => {
                        warn(vm, func, "SSL write failed");
                        return -1;
                    }
                },
                None => loop {
                    let k = unsafe {
                        libc::send(
                            s,
                            buf[off..].as_ptr() as *const libc::c_void,
                            buf.len() - off,
                            libc::MSG_NOSIGNAL,
                        )
                    };
                    if k == -1 && errno() == libc::EINTR {
                        continue;
                    }
                    break k;
                },
            };
            if sent == -1 {
                return -1;
            }
            off += sent as usize;
        }
        buf.len() as i64
    }

    /// my_recv(): the bytes read, 0 at EOF, -1 after an error.
    fn my_recv(&mut self, vm: &mut Vm, s: i32, buf: &mut [u8], _ctl: bool) -> i64 {
        let pending = self.crypto_for(s).map(|c| c.pending()).unwrap_or(0);
        if pending == 0 {
            let n = my_poll(
                s,
                POLL_READABLE,
                (self.timeout_sec.wrapping_mul(1000)) as i32,
            );
            if n < 1 {
                if n == 0 {
                    set_errno(libc::ETIMEDOUT);
                }
                let e = errno();
                warn(vm, self.func, &strerror(e));
                return -1;
            }
        }
        let func = self.func;
        match self.crypto_for(s) {
            Some(c) => match c.read(s, buf, true, -1) {
                Ok(k) => k as i64,
                Err(_) => {
                    warn(vm, func, "SSL read failed");
                    -1
                }
            },
            None => loop {
                let k =
                    unsafe { libc::recv(s, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
                if k == -1 && errno() == libc::EINTR {
                    continue;
                }
                break k as i64;
            },
        }
    }

    /// my_recv() on the data connection into its buffer.
    fn data_recv(&mut self, vm: &mut Vm) -> i64 {
        let Some(d) = self.data.as_mut() else {
            return -1;
        };
        let fd = d.fd;
        let mut tmp = vec![0u8; FTP_BUFSIZE];
        let n = self.my_recv(vm, fd, &mut tmp, false);
        if n > 0 {
            let d = self.data.as_mut().unwrap();
            d.buf[..n as usize].copy_from_slice(&tmp[..n as usize]);
        }
        n
    }

    /// data_available()
    fn data_available(&mut self, vm: &mut Vm, s: i32, silent: bool) -> bool {
        if self.crypto_for(s).map(|c| c.pending()).unwrap_or(0) > 0 {
            return true;
        }
        let n = my_poll(s, POLL_READABLE, 1000);
        if n < 1 {
            if n == 0 {
                if silent {
                    return false;
                }
                set_errno(libc::ETIMEDOUT);
            }
            let e = errno();
            warn(vm, self.func, &strerror(e));
            return false;
        }
        true
    }

    /// data_writeable()
    fn data_writeable(&mut self, vm: &mut Vm, s: i32) -> bool {
        let n = my_poll(s, libc::POLLOUT, 1000);
        if n < 1 {
            if n == 0 {
                set_errno(libc::ETIMEDOUT);
            }
            let e = errno();
            warn(vm, self.func, &strerror(e));
            return false;
        }
        true
    }

    /// ftp_getdata(): the data connection is set up in self.data.
    fn getdata(&mut self, vm: &mut Vm) -> bool {
        let func = self.func;
        if self.pasv != 0 && !self.set_pasv(vm, true) {
            return false;
        }
        let family = self.localaddr.ss_family as i32;
        let fd = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            let e = errno();
            warn(
                vm,
                func,
                &format!("socket() failed: {} ({})", strerror(e), e),
            );
            return false;
        }
        let mut data = Box::new(Data {
            listener: -1,
            fd: -1,
            buf: Box::new([0; FTP_BUFSIZE + 1]),
            crypto: None,
        });
        if self.pasv != 0 {
            // clear the ready status
            self.pasv = 1;
            let size = sockaddr_size(&self.pasvaddr);
            if connect_nonb(fd, &self.pasvaddr, size, self.timeout_sec) == -1 {
                let e = errno();
                warn(
                    vm,
                    func,
                    &format!("php_connect_nonb() failed: {} ({})", strerror(e), e),
                );
                close_fd(fd);
                return false;
            }
            data.fd = fd;
            self.data = Some(data);
            return true;
        }
        // active (normal) connection: bind to a local address
        let mut addr: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        addr.ss_family = family as libc::sa_family_t;
        let mut size = sockaddr_size(&addr);
        if unsafe { libc::bind(fd, &addr as *const _ as *const libc::sockaddr, size) } != 0 {
            let e = errno();
            warn(vm, func, &format!("bind() failed: {} ({})", strerror(e), e));
            close_fd(fd);
            return false;
        }
        if unsafe { libc::getsockname(fd, &mut addr as *mut _ as *mut libc::sockaddr, &mut size) }
            != 0
        {
            let e = errno();
            warn(
                vm,
                func,
                &format!("getsockname() failed: {} ({})", strerror(e), e),
            );
            close_fd(fd);
            return false;
        }
        if unsafe { libc::listen(fd, 5) } != 0 {
            let e = errno();
            warn(
                vm,
                func,
                &format!("listen() failed: {} ({})", strerror(e), e),
            );
            close_fd(fd);
            return false;
        }
        data.listener = fd;
        if family == libc::AF_INET6 {
            // need to use EPRT
            let sin6 = unsafe { &*(&self.localaddr as *const _ as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            let port = u16::from_be(
                unsafe { &*(&addr as *const _ as *const libc::sockaddr_in6) }.sin6_port,
            );
            let arg = format!("|2|{}|{}|", ip, port);
            if !self.putcmd(vm, b"EPRT", Some(arg.as_bytes()))
                || !self.getresp(vm)
                || self.resp != 200
            {
                close_fd(fd);
                return false;
            }
            self.data = Some(data);
            return true;
        }
        // send the PORT
        let sin = unsafe { &*(&self.localaddr as *const _ as *const libc::sockaddr_in) };
        let ip = sin.sin_addr.s_addr.to_ne_bytes();
        let port = unsafe { &*(&addr as *const _ as *const libc::sockaddr_in) }
            .sin_port
            .to_ne_bytes();
        let arg = format!(
            "{},{},{},{},{},{}",
            ip[0], ip[1], ip[2], ip[3], port[0], port[1]
        );
        if !self.putcmd(vm, b"PORT", Some(arg.as_bytes())) || !self.getresp(vm) || self.resp != 200
        {
            close_fd(fd);
            return false;
        }
        self.data = Some(data);
        true
    }

    /// data_accept()
    fn data_accept(&mut self, vm: &mut Vm) -> bool {
        let func = self.func;
        let timeout = self.timeout_sec;
        let Some(d) = self.data.as_mut() else {
            return false;
        };
        if d.fd == -1 {
            let listener = d.listener;
            let n = my_poll(listener, POLL_READABLE, timeout.wrapping_mul(1000) as i32);
            let fd = if n < 1 {
                if n == 0 {
                    set_errno(libc::ETIMEDOUT);
                }
                let e = errno();
                warn(vm, func, &strerror(e));
                -1
            } else {
                unsafe { libc::accept(listener, std::ptr::null_mut(), std::ptr::null_mut()) }
            };
            close_fd(listener);
            let d = self.data.as_mut().unwrap();
            d.listener = -1;
            d.fd = fd;
            if fd == -1 {
                self.data = None;
                return false;
            }
        }
        if self.use_ssl && self.use_ssl_for_data {
            let Some(mut c) = new_crypto(vm, func) else {
                warn(vm, func, "data_accept: failed to create the SSL handle");
                return false;
            };
            let fd = self.data.as_ref().unwrap().fd;
            if !handshake(
                vm,
                func,
                &mut c,
                fd,
                "data_accept: SSL/TLS handshake failed",
            ) {
                return false;
            }
            self.data.as_mut().unwrap().crypto = Some(c);
        }
        true
    }

    /// data_close()
    pub fn data_close(&mut self) {
        let Some(mut d) = self.data.take() else {
            return;
        };
        if d.listener != -1 {
            close_fd(d.listener);
        }
        if d.fd != -1 {
            if let Some(c) = d.crypto.as_mut() {
                ssl_shutdown(None, self.func, d.fd, c);
            }
            close_fd(d.fd);
        }
    }

    /// ftp_genlist(): the lines of a listing.
    pub fn genlist(&mut self, vm: &mut Vm, cmd: &[u8], path: &[u8]) -> Option<Vec<Vec<u8>>> {
        let r = self.genlist_inner(vm, cmd, path);
        if r.is_none() {
            self.data_close();
        }
        r
    }

    fn genlist_inner(&mut self, vm: &mut Vm, cmd: &[u8], path: &[u8]) -> Option<Vec<Vec<u8>>> {
        if !self.set_type(vm, FtpType::Ascii) {
            return None;
        }
        if !self.getdata(vm) {
            return None;
        }
        if !self.putcmd(vm, cmd, Some(path)) {
            return None;
        }
        if !self.getresp(vm) || !matches!(self.resp, 150 | 125 | 226) {
            return None;
        }
        // some servers don't open a ftp-data connection if the directory is empty
        if self.resp == 226 {
            self.data_close();
            return Some(Vec::new());
        }
        if !self.data_accept(vm) {
            return None;
        }
        let mut all = Vec::new();
        loop {
            let n = self.data_recv(vm);
            if n == 0 {
                break;
            }
            if n < 0 {
                return None;
            }
            all.extend_from_slice(&self.data.as_ref().unwrap().buf[..n as usize]);
        }
        self.data_close();
        let mut ret = Vec::new();
        let mut cur = Vec::new();
        let mut lastch = 0u8;
        for &ch in &all {
            if ch == b'\n' && lastch == b'\r' {
                cur.pop();
                ret.push(cstr(&cur).to_vec());
                cur.clear();
            } else {
                cur.push(ch);
            }
            lastch = ch;
        }
        if !self.getresp(vm) || (self.resp != 226 && self.resp != 250) {
            return None;
        }
        Some(ret)
    }

    /// ftp_nb_get()
    pub fn nb_get(
        &mut self,
        vm: &mut Vm,
        outstream: i64,
        path: &[u8],
        typ: FtpType,
        resumepos: i64,
    ) -> i64 {
        if self.data.is_some() {
            // abort the transfer in action
            self.data_close();
            if !self.getresp(vm) || (self.resp != 226 && self.resp != 250) {
                self.data_close();
                return PHP_FTP_FAILED;
            }
        }
        if !self.set_type(vm, typ)
            || !self.getdata(vm)
            || !self.rest_and(vm, resumepos, b"RETR", path)
            || !self.data_accept(vm)
        {
            self.data_close();
            return PHP_FTP_FAILED;
        }
        self.stream = Some(outstream);
        self.lastch = 0;
        self.nb = true;
        self.nb_continue_read(vm)
    }

    /// ftp_nb_continue_read()
    pub fn nb_continue_read(&mut self, vm: &mut Vm) -> i64 {
        let Some(fd) = self.data.as_ref().map(|d| d.fd) else {
            self.nb = false;
            return PHP_FTP_FAILED;
        };
        if !self.data_available(vm, fd, false) {
            return PHP_FTP_MOREDATA;
        }
        let typ = self.typ;
        let mut lastch = self.lastch;
        let stream = self.stream.unwrap_or(0);
        let rcvd = self.data_recv(vm);
        if rcvd != 0 {
            if rcvd < 0 {
                self.nb = false;
                self.data_close();
                return PHP_FTP_FAILED;
            }
            let buf = &self.data.as_ref().unwrap().buf[..rcvd as usize];
            if typ == FtpType::Ascii {
                let mut out = Vec::with_capacity(buf.len());
                for &c in buf {
                    if lastch == b'\r' && c != b'\n' {
                        out.push(b'\r');
                    }
                    if c != b'\r' {
                        out.push(c);
                    }
                    lastch = c;
                }
                streams::write(vm, stream, &out);
            } else {
                let b = buf.to_vec();
                if b.len() != streams::write(vm, stream, &b) {
                    self.nb = false;
                    self.data_close();
                    return PHP_FTP_FAILED;
                }
            }
            self.lastch = lastch;
            return PHP_FTP_MOREDATA;
        }
        if typ == FtpType::Ascii && lastch == b'\r' {
            streams::write(vm, stream, b"\r");
        }
        self.data_close();
        if !self.getresp(vm) || (self.resp != 226 && self.resp != 250) {
            self.nb = false;
            self.data_close();
            return PHP_FTP_FAILED;
        }
        self.nb = false;
        PHP_FTP_FINISHED
    }

    /// ftp_nb_put()
    pub fn nb_put(
        &mut self,
        vm: &mut Vm,
        path: &[u8],
        instream: i64,
        typ: FtpType,
        startpos: i64,
    ) -> i64 {
        if !self.set_type(vm, typ)
            || !self.getdata(vm)
            || !self.rest_and(vm, startpos, b"STOR", path)
            || !self.data_accept(vm)
        {
            self.data_close();
            return PHP_FTP_FAILED;
        }
        self.stream = Some(instream);
        self.lastch = 0;
        self.nb = true;
        self.nb_continue_write(vm)
    }

    /// ftp_nb_continue_write()
    pub fn nb_continue_write(&mut self, vm: &mut Vm) -> i64 {
        let Some(fd) = self.data.as_ref().map(|d| d.fd) else {
            self.nb = false;
            return PHP_FTP_FAILED;
        };
        if !self.data_writeable(vm, fd) {
            return PHP_FTP_MOREDATA;
        }
        let stream = self.stream.unwrap_or(0);
        let typ = self.typ;
        if !self.send_stream(vm, stream, typ, true) {
            self.data_close();
            self.nb = false;
            return PHP_FTP_FAILED;
        }
        if !streams::eof(vm, stream) {
            return PHP_FTP_MOREDATA;
        }
        self.data_close();
        if !self.getresp(vm) || (self.resp != 226 && self.resp != 250) {
            self.data_close();
            self.nb = false;
            return PHP_FTP_FAILED;
        }
        self.nb = false;
        PHP_FTP_FINISHED
    }
}

impl Drop for Ftp {
    fn drop(&mut self) {
        self.close_connections(None);
    }
}

/// ftp_ssl_shutdown(): close_notify, then read until the peer's.
fn ssl_shutdown(vm: Option<&mut Vm>, func: &str, fd: i32, c: &mut Box<dyn CryptoStream>) {
    let done = !c.disable(fd);
    if done {
        return;
    }
    let mut buf = [0u8; 256];
    for _ in 0..64 {
        if c.pending() == 0 && my_poll(fd, POLL_READABLE, 1000) < 1 {
            break;
        }
        match c.read(fd, &mut buf, true, 1000) {
            Ok(0) | Err(_) => break,
            Ok(_) => continue,
        }
    }
    let _ = (vm, func);
}

/// strtoul() from `i`: (value, index after the digits)
fn strtoul(s: &[u8], mut i: usize) -> (u64, usize) {
    let start = i;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let ds = i;
    let mut v: u64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((s[i] - b'0') as u64);
        i += 1;
    }
    if i == ds {
        return (0, start);
    }
    (if neg { v.wrapping_neg() } else { v }, i)
}

/// sscanf("%lu,%lu,%lu,%lu,%lu,%lu")
fn scan_six(s: &[u8]) -> Option<[u64; 6]> {
    let mut out = [0u64; 6];
    let mut i = 0;
    for (k, slot) in out.iter_mut().enumerate() {
        if k > 0 {
            if s.get(i) != Some(&b',') {
                return None;
            }
            i += 1;
        }
        let (v, end) = strtoul(s, i);
        if end == i {
            return None;
        }
        *slot = v;
        i = end;
    }
    Some(out)
}

/// sscanf() of fixed-width %Nd fields one after the other.
fn scan_digits(s: &[u8], widths: &[usize]) -> Option<Vec<i32>> {
    let mut out = Vec::new();
    let mut i = 0;
    for &w in widths {
        while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            i += 1;
        }
        let mut j = i;
        let mut neg = false;
        if j < s.len() && (s[j] == b'+' || s[j] == b'-') && w > 1 {
            neg = s[j] == b'-';
            j += 1;
        }
        let ds = j;
        let mut v: i64 = 0;
        while j < s.len() && j - i < w && s[j].is_ascii_digit() {
            v = v * 10 + (s[j] - b'0') as i64;
            j += 1;
        }
        if j == ds {
            return None;
        }
        out.push(if neg { -v } else { v } as i32);
        i = j;
    }
    Some(out)
}

/// ZEND_ATOL(): strtol(s, NULL, 10)
fn strtol(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        let d = (s[i] - b'0') as i64;
        v = match v.checked_mul(10).and_then(|x| {
            if neg {
                x.checked_sub(d)
            } else {
                x.checked_add(d)
            }
        }) {
            Some(x) => x,
            None => return if neg { i64::MIN } else { i64::MAX },
        };
        i += 1;
    }
    v
}

/// ftp_mlsd_parse_line(): Err(warning) for a malformed line.
pub fn mlsd_parse_line(input: &[u8]) -> Result<PhpArray, &'static str> {
    let input = cstr(input);
    let mut ht = PhpArray::new();
    let Some(sp) = input.iter().position(|&c| c == b' ') else {
        return Err("Missing pathname in MLSD response");
    };
    ht.set(
        ArrayKey::String(PhpString::from_bytes(b"name")),
        Value::String(PhpString::from_bytes(&input[sp + 1..])),
    );
    let end = sp;
    let mut i = 0;
    while i < end {
        let Some(semi) = input[i..end].iter().position(|&c| c == b';').map(|p| p + i) else {
            return Err("Malformed fact in MLSD response");
        };
        let Some(eq) = input[i..semi]
            .iter()
            .position(|&c| c == b'=')
            .map(|p| p + i)
        else {
            return Err("Malformed fact in MLSD response");
        };
        // (zend_hash_str_update(): always a string key)
        let k = ArrayKey::String(PhpString::from_bytes(&input[i..eq]));
        ht.set(
            k,
            Value::String(PhpString::from_bytes(&input[eq + 1..semi])),
        );
        i = semi + 1;
    }
    Ok(ht)
}
