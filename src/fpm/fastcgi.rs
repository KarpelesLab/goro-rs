//! main/fastcgi.c: the FastCGI protocol (records, the request parameters,
//! stdin/stdout/stderr streams, FCGI_GET_VALUES).

use std::cell::{Cell, RefCell};

pub const FCGI_VERSION_1: u8 = 1;
pub const FCGI_BEGIN_REQUEST: u8 = 1;
pub const FCGI_END_REQUEST: u8 = 3;
pub const FCGI_PARAMS: u8 = 4;
pub const FCGI_STDIN: u8 = 5;
pub const FCGI_STDOUT: u8 = 6;
pub const FCGI_STDERR: u8 = 7;
pub const FCGI_GET_VALUES: u8 = 9;
pub const FCGI_GET_VALUES_RESULT: u8 = 10;

const FCGI_KEEP_CONN: u8 = 1;
const FCGI_RESPONDER: u16 = 1;
const FCGI_AUTHORIZER: u16 = 2;
const FCGI_FILTER: u16 = 3;
const FCGI_REQUEST_COMPLETE: u8 = 0;

const FCGI_MAX_LENGTH: usize = 0xffff;
const OUT_BUF_SIZE: usize = 1024 * 8;
const HEADER_LEN: usize = 8;

thread_local! {
    static IN_SHUTDOWN: Cell<bool> = const { Cell::new(false) };
    /// allowed_clients (None: everybody)
    static ALLOWED_CLIENTS: RefCell<Option<Vec<std::net::IpAddr>>> = const { RefCell::new(None) };
    /// client_sa: the address of the last accepted connection
    static CLIENT_SA: RefCell<Option<std::net::SocketAddr>> = const { RefCell::new(None) };
    static CLIENT_IS_UNIX: Cell<bool> = const { Cell::new(false) };
    /// fcgi_mgmt_vars
    static MGMT_VARS: RefCell<Vec<(Vec<u8>, Vec<u8>)>> = RefCell::new(vec![(b"FCGI_MPXS_CONNS".to_vec(), b"0".to_vec())]);
    static LOGGER: Cell<Option<fn(i32, &str)>> = const { Cell::new(None) };
}

/// The listening socket the soft quit handler closes (fpm_globals.listening_socket).
pub static LISTEN_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
/// in_shutdown set from a signal handler (fcgi_terminate()).
pub static TERMINATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_logger(l: fn(i32, &str)) {
    LOGGER.with(|c| c.set(Some(l)));
}

fn fcgi_log(level: i32, msg: &str) {
    if let Some(l) = LOGGER.with(|c| c.get()) {
        l(level, msg);
    } else {
        eprint!("{}", msg);
    }
}

pub fn in_shutdown() -> bool {
    IN_SHUTDOWN.with(|c| c.get()) || TERMINATE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The request parameters (fcgi_hash): lookups by name, iteration in the
/// order fcgi_hash_apply() walks them (the last one added first).
#[derive(Default, Clone)]
pub struct FcgiEnv {
    entries: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    index: std::collections::HashMap<Vec<u8>, usize>,
}

impl FcgiEnv {
    pub fn clean(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    /// fcgi_hash_set()
    pub fn set(&mut self, var: &[u8], val: &[u8]) {
        if let Some(&i) = self.index.get(var) {
            self.entries[i].1 = Some(val.to_vec());
            return;
        }
        self.entries.push((var.to_vec(), Some(val.to_vec())));
        self.index.insert(var.to_vec(), self.entries.len() - 1);
    }

    /// fcgi_hash_del()
    pub fn del(&mut self, var: &[u8]) {
        if let Some(i) = self.index.remove(var) {
            self.entries[i].1 = None;
        }
    }

    pub fn get(&self, var: &[u8]) -> Option<&[u8]> {
        self.index
            .get(var)
            .and_then(|&i| self.entries[i].1.as_deref())
    }

    /// fcgi_hash_apply(): newest first.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.entries
            .iter()
            .rev()
            .filter_map(|(k, v)| v.as_deref().map(|v| (k.as_slice(), v)))
    }

    /// All the names (live or deleted), newest first, as fcgi_read_request()
    /// walks req->env.list for FCGI_GET_VALUES.
    fn all_names(&self) -> Vec<Vec<u8>> {
        self.entries.iter().rev().map(|(k, _)| k.clone()).collect()
    }
}

/// struct _fcgi_request
pub struct FcgiRequest {
    pub listen_socket: i32,
    pub tcp: bool,
    pub fd: i32,
    pub id: i32,
    pub keep: bool,
    nodelay: bool,
    pub ended: bool,
    in_len: usize,
    in_pad: usize,
    out_hdr: Option<usize>,
    out_buf: Vec<u8>,
    pub on_accept: fn(),
    pub on_read: fn(),
    pub on_close: fn(),
    pub has_env: bool,
    pub env: FcgiEnv,
}

fn dummy() {}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn set_errno(e: i32) {
    unsafe { *libc::__errno_location() = e };
}

impl FcgiRequest {
    /// fcgi_init_request()
    pub fn new(
        listen_socket: i32,
        on_accept: Option<fn()>,
        on_read: Option<fn()>,
        on_close: Option<fn()>,
    ) -> Self {
        FcgiRequest {
            listen_socket,
            tcp: false,
            fd: -1,
            id: -1,
            keep: false,
            nodelay: false,
            ended: false,
            in_len: 0,
            in_pad: 0,
            out_hdr: None,
            out_buf: Vec::with_capacity(OUT_BUF_SIZE + 16),
            on_accept: on_accept.unwrap_or(dummy),
            on_read: on_read.unwrap_or(dummy),
            on_close: on_close.unwrap_or(dummy),
            has_env: false,
            env: FcgiEnv::default(),
        }
    }

    /// safe_write()
    fn safe_write(&mut self, buf: &[u8]) -> isize {
        let mut n = 0usize;
        while n != buf.len() {
            set_errno(0);
            let ret = unsafe {
                libc::write(
                    self.fd,
                    buf[n..].as_ptr() as *const libc::c_void,
                    buf.len() - n,
                )
            };
            if ret > 0 {
                n += ret as usize;
            } else if errno() != 0 && errno() != libc::EINTR {
                return ret;
            }
        }
        n as isize
    }

    /// safe_read()
    fn safe_read(&mut self, buf: &mut [u8]) -> isize {
        let mut n = 0usize;
        while n != buf.len() {
            set_errno(0);
            let ret = unsafe {
                libc::read(
                    self.fd,
                    buf[n..].as_mut_ptr() as *mut libc::c_void,
                    buf.len() - n,
                )
            };
            if ret > 0 {
                n += ret as usize;
            } else if ret == 0 && errno() == 0 {
                return n as isize;
            } else if errno() != 0 && errno() != libc::EINTR {
                return ret;
            }
        }
        n as isize
    }

    fn read_header(&mut self) -> Option<(u8, usize, usize, u16)> {
        let mut hdr = [0u8; HEADER_LEN];
        if self.safe_read(&mut hdr) != HEADER_LEN as isize || hdr[0] < FCGI_VERSION_1 {
            return None;
        }
        let len = ((hdr[4] as usize) << 8) | hdr[5] as usize;
        let id = ((hdr[2] as u16) << 8) | hdr[3] as u16;
        Some((hdr[1], len, hdr[6] as usize, id))
    }

    /// fcgi_get_params()
    fn get_params(&mut self, p: &[u8]) -> bool {
        let mut i = 0;
        let end = p.len();
        while i < end {
            let mut name_len = p[i] as usize;
            i += 1;
            if name_len >= 128 {
                if i + 3 >= end {
                    return false;
                }
                name_len = ((name_len & 0x7f) << 24)
                    | ((p[i] as usize) << 16)
                    | ((p[i + 1] as usize) << 8)
                    | p[i + 2] as usize;
                i += 3;
            }
            if i >= end {
                return false;
            }
            let mut val_len = p[i] as usize;
            i += 1;
            if val_len >= 128 {
                if i + 3 >= end {
                    return false;
                }
                val_len = ((val_len & 0x7f) << 24)
                    | ((p[i] as usize) << 16)
                    | ((p[i + 1] as usize) << 8)
                    | p[i + 2] as usize;
                i += 3;
            }
            if name_len + val_len > end - i {
                return false;
            }
            let name = p[i..i + name_len].to_vec();
            let val = p[i + name_len..i + name_len + val_len].to_vec();
            self.env.set(&name, &val);
            i += name_len + val_len;
        }
        true
    }

    /// fcgi_read_request(): 1 = a request, 2 = FCGI_GET_VALUES answered,
    /// 0 = failure.
    fn read_request(&mut self) -> i32 {
        self.keep = false;
        self.ended = false;
        self.in_len = 0;
        self.out_hdr = None;
        self.out_buf.clear();
        if self.has_env {
            self.env.clean();
        } else {
            self.has_env = true;
        }
        let Some((mut typ, mut len, mut padding, mut id)) = self.read_header() else {
            return 0;
        };
        while typ == FCGI_STDIN && len == 0 {
            let Some(h) = self.read_header() else {
                return 0;
            };
            (typ, len, padding, id) = h;
        }
        if len + padding > FCGI_MAX_LENGTH {
            return 0;
        }
        self.id = id as i32;
        let mut buf = vec![0u8; FCGI_MAX_LENGTH + 8];
        if typ == FCGI_BEGIN_REQUEST && len == 8 {
            if self.safe_read(&mut buf[..len + padding]) != (len + padding) as isize {
                return 0;
            }
            self.keep = buf[2] & FCGI_KEEP_CONN != 0;
            if self.keep && self.tcp && !self.nodelay {
                let on: libc::c_int = 1;
                unsafe {
                    libc::setsockopt(
                        self.fd,
                        libc::IPPROTO_TCP,
                        libc::TCP_NODELAY,
                        &on as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                    );
                }
                self.nodelay = true;
            }
            let role = ((buf[0] as u16) << 8) + buf[1] as u16;
            match role {
                FCGI_RESPONDER => self.env.set(b"FCGI_ROLE", b"RESPONDER"),
                FCGI_AUTHORIZER => self.env.set(b"FCGI_ROLE", b"AUTHORIZER"),
                FCGI_FILTER => self.env.set(b"FCGI_ROLE", b"FILTER"),
                _ => return 0,
            }
            let Some((t, l, p, _)) = self.read_header() else {
                return 0;
            };
            (typ, len, padding) = (t, l, p);
            while typ == FCGI_PARAMS && len > 0 {
                if len + padding > FCGI_MAX_LENGTH {
                    return 0;
                }
                if self.safe_read(&mut buf[..len + padding]) != (len + padding) as isize {
                    self.keep = false;
                    return 0;
                }
                let chunk = buf[..len].to_vec();
                if !self.get_params(&chunk) {
                    self.keep = false;
                    return 0;
                }
                let Some((t, l, p, _)) = self.read_header() else {
                    self.keep = false;
                    return 0;
                };
                (typ, len, padding) = (t, l, p);
            }
        } else if typ == FCGI_GET_VALUES {
            if self.safe_read(&mut buf[..len + padding]) != (len + padding) as isize {
                self.keep = false;
                return 0;
            }
            let chunk = buf[..len].to_vec();
            if !self.get_params(&chunk) {
                self.keep = false;
                return 0;
            }
            let mut out = vec![0u8; HEADER_LEN];
            let limit = FCGI_MAX_LENGTH + 8;
            for name in self.env.all_names() {
                let value = MGMT_VARS.with(|m| {
                    m.borrow()
                        .iter()
                        .find(|(k, _)| *k == name)
                        .map(|(_, v)| v.clone())
                });
                let Some(value) = value else {
                    continue;
                };
                if out.len() + 4 + 4 + name.len() + value.len() >= limit {
                    break;
                }
                for l in [name.len(), value.len()] {
                    if l < 0x80 {
                        out.push(l as u8);
                    } else {
                        out.push(((l >> 24) & 0xff) as u8 | 0x80);
                        out.push(((l >> 16) & 0xff) as u8);
                        out.push(((l >> 8) & 0xff) as u8);
                        out.push((l & 0xff) as u8);
                    }
                }
                out.extend_from_slice(&name);
                out.extend_from_slice(&value);
            }
            let len = out.len() - HEADER_LEN;
            let pad = make_header(&mut out, 0, FCGI_GET_VALUES_RESULT, 0, len);
            out.resize(HEADER_LEN + len + pad, 0);
            if self.safe_write(&out) != out.len() as isize {
                self.keep = false;
                return 0;
            }
            return 2;
        } else {
            return 0;
        }
        1
    }

    /// fcgi_read(): the request body.
    pub fn read(&mut self, out: &mut [u8]) -> isize {
        let mut n = 0usize;
        let mut rest = out.len();
        while rest > 0 {
            if self.in_len == 0 {
                match self.read_header() {
                    Some((t, l, p, _)) if t == FCGI_STDIN => {
                        self.in_len = l;
                        self.in_pad = p;
                        if l == 0 {
                            return n as isize;
                        }
                    }
                    _ => {
                        self.keep = false;
                        return 0;
                    }
                }
            }
            let want = self.in_len.min(rest);
            let ret = {
                let (a, _) = out[n..].split_at_mut(want);
                self.safe_read(a)
            };
            if ret < 0 {
                self.keep = false;
                return ret;
            } else if ret > 0 {
                let r = ret as usize;
                self.in_len -= r;
                rest -= r;
                n += r;
                if self.in_len == 0 {
                    if self.in_pad > 0 {
                        let mut pad = vec![0u8; self.in_pad];
                        if self.safe_read(&mut pad) != self.in_pad as isize {
                            self.keep = false;
                            return ret;
                        }
                    }
                } else {
                    return n as isize;
                }
            } else {
                return n as isize;
            }
        }
        n as isize
    }

    /// fcgi_close()
    pub fn close(&mut self, force: bool, destroy: bool) {
        if destroy && self.has_env {
            self.env.clean();
            self.has_env = false;
        }
        if (force || !self.keep) && self.fd >= 0 {
            if !force {
                unsafe { libc::shutdown(self.fd, libc::SHUT_WR) };
                let mut b = [0u8; 8];
                while unsafe { libc::recv(self.fd, b.as_mut_ptr() as *mut libc::c_void, 8, 0) } > 0
                {
                }
            }
            unsafe { libc::close(self.fd) };
            self.nodelay = false;
            self.fd = -1;
            (self.on_close)();
        }
    }

    pub fn is_closed(&self) -> bool {
        self.fd < 0
    }

    /// fcgi_accept_request(): the connection descriptor, or -1.
    pub fn accept_request(&mut self) -> i32 {
        loop {
            if self.fd < 0 {
                loop {
                    if in_shutdown() {
                        return -1;
                    }
                    (self.on_accept)();
                    let mut sa: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
                    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
                    let listen = self.listen_socket;
                    self.fd = unsafe {
                        libc::accept(listen, &mut sa as *mut _ as *mut libc::sockaddr, &mut len)
                    };
                    let accept_errno = errno();
                    set_client_sa(&sa);
                    if self.fd >= 0 && !is_allowed() {
                        fcgi_log(
                            4,
                            &format!(
                                "Connection disallowed: IP address '{}' has been dropped.",
                                get_last_client_ip().unwrap_or_default()
                            ),
                        );
                        unsafe { libc::close(self.fd) };
                        self.fd = -1;
                        continue;
                    }
                    if self.fd < 0
                        && (in_shutdown()
                            || (accept_errno != libc::EINTR && accept_errno != libc::ECONNABORTED))
                    {
                        return -1;
                    }
                    if self.fd >= 0 {
                        let attrs = unsafe { libc::fcntl(self.fd, libc::F_GETFD) };
                        if attrs < 0 {
                            fcgi_log(3, "failed to get attributes of the connection socket");
                        }
                        if unsafe { libc::fcntl(self.fd, libc::F_SETFD, attrs | libc::FD_CLOEXEC) }
                            < 0
                        {
                            fcgi_log(3, "failed to change attribute of the connection socket");
                        }
                        self.tcp = sa.ss_family as i32 != libc::AF_UNIX;
                        let mut fds = libc::pollfd {
                            fd: self.fd,
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        let mut ret;
                        loop {
                            set_errno(0);
                            ret = unsafe { libc::poll(&mut fds, 1, 5000) };
                            if !(ret < 0 && errno() == libc::EINTR) {
                                break;
                            }
                        }
                        if ret > 0 && (fds.revents & libc::POLLIN) != 0 {
                            break;
                        }
                        self.close(true, false);
                    }
                }
            } else if in_shutdown() {
                return -1;
            }
            (self.on_read)();
            let r = self.read_request();
            if r == 1 {
                return self.fd;
            } else if r == 0 {
                self.close(true, true);
            }
        }
    }

    fn open_packet(&mut self, typ: u8) {
        self.out_hdr = Some(self.out_buf.len());
        self.out_buf
            .extend_from_slice(&[FCGI_VERSION_1, typ, 0, 0, 0, 0, 0, 0]);
    }

    fn close_packet(&mut self) {
        if let Some(h) = self.out_hdr.take() {
            let len = self.out_buf.len() - (h + HEADER_LEN);
            let typ = self.out_buf[h + 1];
            let id = self.id;
            let pad = make_header(&mut self.out_buf, h, typ, id, len);
            let l = self.out_buf.len();
            self.out_buf.resize(l + pad, 0);
        }
    }

    /// fcgi_flush()
    pub fn flush(&mut self, end: bool) -> bool {
        self.close_packet();
        if end {
            let pos = self.out_buf.len();
            self.out_buf.resize(pos + 16, 0);
            make_header(&mut self.out_buf, pos, FCGI_END_REQUEST, self.id, 8);
            self.out_buf[pos + 8..pos + 16].copy_from_slice(&[
                0,
                0,
                0,
                0,
                FCGI_REQUEST_COMPLETE,
                0,
                0,
                0,
            ]);
        }
        let buf = std::mem::take(&mut self.out_buf);
        let ok = self.safe_write(&buf) == buf.len() as isize;
        self.out_buf = buf;
        self.out_buf.clear();
        if !ok {
            self.keep = false;
            return false;
        }
        true
    }

    fn limit(&self) -> isize {
        OUT_BUF_SIZE as isize - self.out_buf.len() as isize
    }

    /// fcgi_write()
    pub fn write(&mut self, typ: u8, s: &[u8]) -> isize {
        let len = s.len();
        if len == 0 {
            return 0;
        }
        if let Some(h) = self.out_hdr {
            if self.out_buf[h + 1] != typ {
                self.close_packet();
            }
        }
        let mut limit = self.limit();
        if self.out_hdr.is_none() {
            limit -= HEADER_LEN as isize;
            if limit < 0 {
                limit = 0;
            }
        }
        let limit = limit as usize;
        if len < limit {
            if self.out_hdr.is_none() {
                self.open_packet(typ);
            }
            self.out_buf.extend_from_slice(s);
        } else if len - limit < OUT_BUF_SIZE - HEADER_LEN {
            if limit > 0 {
                if self.out_hdr.is_none() {
                    self.open_packet(typ);
                }
                self.out_buf.extend_from_slice(&s[..limit]);
            }
            if !self.flush(false) {
                return -1;
            }
            if len > limit {
                self.open_packet(typ);
                self.out_buf.extend_from_slice(&s[limit..]);
            }
        } else {
            let mut pos = 0usize;
            self.close_packet();
            while len - pos > 0xffff {
                self.open_packet(typ);
                let h = self.out_hdr.take().unwrap();
                make_header(&mut self.out_buf, h, typ, self.id, 0xfff8);
                if !self.flush(false) {
                    return -1;
                }
                if self.safe_write(&s[pos..pos + 0xfff8]) != 0xfff8 {
                    self.keep = false;
                    return -1;
                }
                pos += 0xfff8;
            }
            let remaining = len - pos;
            let pad = ((remaining + 7) & !7) - remaining;
            let rest = if pad != 0 { 8 - pad } else { 0 };
            self.open_packet(typ);
            let h = self.out_hdr.take().unwrap();
            make_header(&mut self.out_buf, h, typ, self.id, remaining - rest);
            if !self.flush(false) {
                return -1;
            }
            if self.safe_write(&s[pos..len - rest]) != (remaining - rest) as isize {
                self.keep = false;
                return -1;
            }
            if pad != 0 {
                self.open_packet(typ);
                self.out_buf.extend_from_slice(&s[len - rest..]);
            }
        }
        len as isize
    }

    /// fcgi_end()
    pub fn end(&mut self) -> bool {
        let mut ret = true;
        if !self.ended {
            ret = self.flush(true);
            self.ended = true;
        }
        ret
    }

    /// fcgi_finish_request()
    pub fn finish_request(&mut self, force_close: bool) -> bool {
        let mut ret = true;
        if self.fd >= 0 {
            ret = self.end();
            self.close(force_close, true);
        }
        ret
    }

    /// fcgi_getenv()
    pub fn getenv(&self, var: &[u8]) -> Option<Vec<u8>> {
        self.env.get(var).map(|v| v.to_vec())
    }

    /// fcgi_putenv()
    pub fn putenv(&mut self, var: &[u8], val: Option<&[u8]>) -> Option<Vec<u8>> {
        match val {
            None => {
                self.env.del(var);
                None
            }
            Some(v) => {
                self.env.set(var, v);
                Some(v.to_vec())
            }
        }
    }
}

/// fcgi_make_header() at `pos` of `buf`: returns the padding length (the
/// padding bytes after the content are zeroed when present).
fn make_header(buf: &mut [u8], pos: usize, typ: u8, req_id: i32, len: usize) -> usize {
    let pad = ((len + 7) & !7) - len;
    buf[pos] = FCGI_VERSION_1;
    buf[pos + 1] = typ;
    buf[pos + 2] = ((req_id >> 8) & 0xff) as u8;
    buf[pos + 3] = (req_id & 0xff) as u8;
    buf[pos + 4] = ((len >> 8) & 0xff) as u8;
    buf[pos + 5] = (len & 0xff) as u8;
    buf[pos + 6] = pad as u8;
    buf[pos + 7] = 0;
    pad
}

fn set_client_sa(sa: &libc::sockaddr_storage) {
    let fam = sa.ss_family as i32;
    CLIENT_IS_UNIX.with(|c| c.set(fam == libc::AF_UNIX));
    let addr = match fam {
        libc::AF_INET => {
            let s: &libc::sockaddr_in = unsafe { &*(sa as *const _ as *const libc::sockaddr_in) };
            Some(std::net::SocketAddr::new(
                std::net::IpAddr::V4(std::net::Ipv4Addr::from(u32::from_be(s.sin_addr.s_addr))),
                u16::from_be(s.sin_port),
            ))
        }
        libc::AF_INET6 => {
            let s: &libc::sockaddr_in6 = unsafe { &*(sa as *const _ as *const libc::sockaddr_in6) };
            Some(std::net::SocketAddr::new(
                std::net::IpAddr::V6(std::net::Ipv6Addr::from(s.sin6_addr.s6_addr)),
                u16::from_be(s.sin6_port),
            ))
        }
        _ => None,
    };
    CLIENT_SA.with(|c| *c.borrow_mut() = addr);
}

/// fcgi_is_allowed()
fn is_allowed() -> bool {
    if CLIENT_IS_UNIX.with(|c| c.get()) {
        return true;
    }
    ALLOWED_CLIENTS.with(|a| {
        let a = a.borrow();
        let Some(list) = a.as_ref() else {
            return true;
        };
        let Some(sa) = CLIENT_SA.with(|c| *c.borrow()) else {
            return false;
        };
        match sa.ip() {
            std::net::IpAddr::V4(v4) => list.contains(&std::net::IpAddr::V4(v4)),
            std::net::IpAddr::V6(v6) => list.iter().any(|x| match x {
                // the first 12 bytes only, like the memcmp() of fcgi_is_allowed()
                std::net::IpAddr::V6(a) => a.octets()[..12] == v6.octets()[..12],
                std::net::IpAddr::V4(a) => v6.to_ipv4_mapped().is_some_and(|m| m == *a),
            }),
        }
    })
}

/// fcgi_set_allowed_clients()
pub fn set_allowed_clients(ip: Option<&str>) {
    let Some(ip) = ip else {
        return;
    };
    let mut list = Vec::new();
    for cur in ip.split(',') {
        if let Ok(a) = cur.parse::<std::net::Ipv4Addr>() {
            list.push(std::net::IpAddr::V4(a));
        } else if let Ok(a) = cur.parse::<std::net::Ipv6Addr>() {
            list.push(std::net::IpAddr::V6(a));
        } else {
            fcgi_log(
                4,
                &format!("Wrong IP address '{}' in listen.allowed_clients", cur),
            );
        }
    }
    if list.is_empty() {
        fcgi_log(4, "There are no allowed addresses");
    }
    ALLOWED_CLIENTS.with(|a| *a.borrow_mut() = Some(list));
}

/// fcgi_get_last_client_ip()
pub fn get_last_client_ip() -> Option<String> {
    let sa = CLIENT_SA.with(|c| *c.borrow())?;
    Some(match sa.ip() {
        std::net::IpAddr::V4(v4) => v4.to_string(),
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(m) => m.to_string(),
            None => v6.to_string(),
        },
    })
}
