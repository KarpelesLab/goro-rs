//! sapi/fpm/fpm/fpm_sockets.c: the listening sockets (inherited through
//! FPM_SOCKETS on reload).

use super::events::Cleanup;
use super::scoreboard;
use super::zlog::{self, ALERT, DEBUG, ERROR, NOTICE, SYSERROR, WARNING};
use super::{Domain, Fpm};
use crate::zlog;

const ENV_SOCKET_SET_MAX: usize = 256;
const ENV_SOCKET_SET_SIZE: usize = 128;

pub struct ListeningSocket {
    pub refcount: i32,
    pub sock: i32,
    pub typ: Domain,
    pub key: String,
}

/// fpm_sockets_domain_from_address()
pub fn domain_from_address(addr: &str) -> Domain {
    if addr.contains(':') {
        return Domain::Inet;
    }
    if addr.bytes().all(|c| c.is_ascii_digit()) {
        return Domain::Inet;
    }
    Domain::Unix
}

fn env_name(idx: usize) -> String {
    if idx == 0 {
        "FPM_SOCKETS".to_string()
    } else {
        format!("FPM_SOCKETS_{}", idx)
    }
}

/// fpm_socket_get_listening_queue() (TCP_INFO): (cur_lq, max_lq)
pub fn get_listening_queue(sock: i32) -> Option<(u32, u32)> {
    let mut info: libc::tcp_info = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::tcp_info>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            sock,
            libc::IPPROTO_TCP,
            libc::TCP_INFO,
            &mut info as *mut _ as *mut libc::c_void,
            &mut len,
        )
    } < 0
    {
        zlog!(SYSERROR, "failed to retrieve TCP_INFO for socket");
        return None;
    }
    if info.tcpi_sacked == 0 {
        return None;
    }
    Some((info.tcpi_unacked, info.tcpi_sacked))
}

/// The textual address key of a socket address (inet_ntop() + ":port", or
/// the unix socket path).
fn sa_key(sa: &libc::sockaddr_storage) -> Option<String> {
    match sa.ss_family as i32 {
        libc::AF_INET => {
            let s: &libc::sockaddr_in = unsafe { &*(sa as *const _ as *const libc::sockaddr_in) };
            let ip = std::net::Ipv4Addr::from(u32::from_be(s.sin_addr.s_addr));
            Some(format!("{}:{}", ip, u16::from_be(s.sin_port)))
        }
        libc::AF_INET6 => {
            let s: &libc::sockaddr_in6 = unsafe { &*(sa as *const _ as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(s.sin6_addr.s6_addr);
            Some(format!("{}:{}", ip, u16::from_be(s.sin6_port)))
        }
        libc::AF_UNIX => {
            let s: &libc::sockaddr_un = unsafe { &*(sa as *const _ as *const libc::sockaddr_un) };
            let p: Vec<u8> = s
                .sun_path
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            Some(String::from_utf8_lossy(&p).into_owned())
        }
        _ => None,
    }
}

fn sa_ip_string(sa: &libc::sockaddr) -> String {
    let st = sa as *const libc::sockaddr as *const libc::sockaddr_storage;
    let st = unsafe { &*st };
    match sa_key(st) {
        Some(k) => match k.rfind(':') {
            Some(p) => k[..p].to_string(),
            None => k,
        },
        None => String::new(),
    }
}

/// fpm_socket_unix_test_connect()
fn unix_test_connect(sa: &libc::sockaddr_un, len: libc::socklen_t) -> bool {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return false;
    }
    let r = unsafe { libc::connect(fd, sa as *const _ as *const libc::sockaddr, len) };
    unsafe { libc::close(fd) };
    r == 0
}

impl Fpm {
    /// fpm_sockets_cleanup()
    pub fn sockets_cleanup(&mut self, which: Cleanup) {
        let mut env_value = String::new();
        let mut sets: Vec<String> = Vec::new();
        for (i, ls) in self.sockets.iter().enumerate() {
            if which != Cleanup::ParentExec {
                unsafe { libc::close(ls.sock) };
            } else {
                if i % ENV_SOCKET_SET_SIZE == 0 && i != 0 {
                    sets.push(std::mem::take(&mut env_value));
                }
                if !env_value.is_empty() {
                    env_value.push(',');
                }
                env_value.push_str(&format!("{}={}", ls.key, ls.sock));
            }
            if which == Cleanup::ParentExitMain && ls.typ == Domain::Unix {
                let _ = std::fs::remove_file(&ls.key);
            }
        }
        if which == Cleanup::ParentExec && !self.sockets.is_empty() {
            sets.push(env_value);
            for (i, v) in sets.iter().enumerate() {
                unsafe { std::env::set_var(env_name(i), v) };
            }
            unsafe { std::env::remove_var(env_name(sets.len())) };
        }
        self.sockets.clear();
    }

    /// fpm_sockets_hash_op(FPM_GET_USE_SOCKET)
    fn sockets_get_use(&mut self, key: &str) -> Option<i32> {
        for ls in self.sockets.iter_mut() {
            if ls.key == key {
                ls.refcount += 1;
                return Some(ls.sock);
            }
        }
        None
    }

    /// fpm_sockets_new_listening_socket()
    fn sockets_new_listening_socket(
        &mut self,
        wp: usize,
        sa: *const libc::sockaddr,
        socklen: libc::socklen_t,
    ) -> i32 {
        let family = unsafe { (*sa).sa_family } as i32;
        let sock = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
        if sock < 0 {
            zlog!(SYSERROR, "failed to create new listening socket: socket()");
            return -1;
        }
        let flags: libc::c_int = 1;
        if unsafe {
            libc::setsockopt(
                sock,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &flags as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        } < 0
        {
            zlog!(WARNING, "failed to change socket attribute");
        }
        let mut saved_umask = 0;
        let is_unix = self.pools[wp].listen_address_domain == Domain::Unix;
        let path = if is_unix {
            let su = unsafe { &*(sa as *const libc::sockaddr_un) };
            let p: Vec<u8> = su
                .sun_path
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            Some(p)
        } else {
            None
        };
        if let Some(p) = &path {
            let su = unsafe { &*(sa as *const libc::sockaddr_un) };
            if unix_test_connect(su, socklen) {
                zlog!(
                    ERROR,
                    "Another FPM instance seems to already listen on {}",
                    String::from_utf8_lossy(p)
                );
                unsafe { libc::close(sock) };
                return -1;
            }
            let cp = std::ffi::CString::new(p.clone()).unwrap_or_default();
            unsafe { libc::unlink(cp.as_ptr()) };
            saved_umask = unsafe { libc::umask(0o777 ^ self.pools[wp].socket_mode) };
        }
        if unsafe { libc::bind(sock, sa, socklen) } < 0 {
            zlog!(
                SYSERROR,
                "unable to bind listening socket for address '{}'",
                self.pools[wp]
                    .config
                    .listen_address
                    .as_deref()
                    .unwrap_or("")
            );
            if is_unix {
                unsafe { libc::umask(saved_umask) };
            }
            unsafe { libc::close(sock) };
            return -1;
        }
        if let Some(p) = &path {
            unsafe { libc::umask(saved_umask) };
            if self.unix_set_socket_permissions(wp, p) < 0 {
                unsafe { libc::close(sock) };
                return -1;
            }
        }
        if unsafe { libc::listen(sock, self.pools[wp].config.listen_backlog) } < 0 {
            zlog!(
                SYSERROR,
                "failed to listen to address '{}'",
                self.pools[wp]
                    .config
                    .listen_address
                    .as_deref()
                    .unwrap_or("")
            );
            unsafe { libc::close(sock) };
            return -1;
        }
        sock
    }

    /// fpm_sockets_get_listening_socket()
    fn sockets_get_listening_socket(
        &mut self,
        wp: usize,
        sa: *const libc::sockaddr,
        socklen: libc::socklen_t,
    ) -> i32 {
        let mut st: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        unsafe {
            std::ptr::copy_nonoverlapping(
                sa as *const u8,
                &mut st as *mut _ as *mut u8,
                (socklen as usize).min(std::mem::size_of::<libc::sockaddr_storage>()),
            );
        }
        let key = sa_key(&st).unwrap_or_default();
        if let Some(s) = self.sockets_get_use(&key) {
            return s;
        }
        let sock = self.sockets_new_listening_socket(wp, sa, socklen);
        let typ = self.pools[wp].listen_address_domain;
        self.sockets.push(ListeningSocket {
            refcount: 1,
            sock,
            typ,
            key,
        });
        sock
    }

    /// fpm_socket_af_inet_socket_by_addr()
    fn socket_af_inet_socket_by_addr(&mut self, wp: usize, addr: &str, port: &str) -> i32 {
        let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
        hints.ai_family = libc::AF_UNSPEC;
        hints.ai_socktype = libc::SOCK_STREAM;
        let caddr = std::ffi::CString::new(addr).unwrap_or_default();
        let cport = std::ffi::CString::new(port).unwrap_or_default();
        let mut servinfo: *mut libc::addrinfo = std::ptr::null_mut();
        let status =
            unsafe { libc::getaddrinfo(caddr.as_ptr(), cport.as_ptr(), &hints, &mut servinfo) };
        if status != 0 {
            let msg = unsafe { std::ffi::CStr::from_ptr(libc::gai_strerror(status)) }
                .to_string_lossy()
                .into_owned();
            zlog!(ERROR, "getaddrinfo: {}", msg);
            return -1;
        }
        let mut sock = -1;
        let mut p = servinfo;
        while !p.is_null() {
            let ai = unsafe { &*p };
            let tmp = sa_ip_string(unsafe { &*ai.ai_addr });
            if sock < 0 {
                sock = self.sockets_get_listening_socket(wp, ai.ai_addr, ai.ai_addrlen);
                if sock != -1 {
                    zlog!(
                        DEBUG,
                        "Found address for {}, socket opened on {}",
                        addr,
                        tmp
                    );
                }
            } else {
                zlog!(
                    WARNING,
                    "Found multiple addresses for {}, {} ignored",
                    addr,
                    tmp
                );
            }
            p = ai.ai_next;
        }
        unsafe { libc::freeaddrinfo(servinfo) };
        sock
    }

    /// fpm_socket_af_inet_listening_socket()
    fn socket_af_inet_listening_socket(&mut self, wp: usize) -> i32 {
        let dup = self.pools[wp]
            .config
            .listen_address
            .clone()
            .unwrap_or_default();
        let (addr, port_str, port): (Option<String>, String, i64) = match dup.rfind(':') {
            Some(p) => {
                let port_str = dup[p + 1..].to_string();
                let mut a = dup[..p].to_string();
                if a.starts_with('[') && a.ends_with(']') && a.len() >= 2 {
                    a = a[1..a.len() - 1].to_string();
                }
                let port = super::conf::atoi(&port_str);
                (Some(a), port_str, port)
            }
            None if dup.bytes().all(|c| c.is_ascii_digit()) => {
                (None, dup.clone(), super::conf::atoi(&dup))
            }
            None => (None, String::from("(null)"), 0),
        };
        if !(1..=65535).contains(&port) {
            zlog!(ERROR, "invalid port value '{}'", port_str);
            return -1;
        }
        match addr {
            Some(a) => self.socket_af_inet_socket_by_addr(wp, &a, &port_str),
            None => {
                let old = zlog::set_level(ALERT);
                let mut sock = self.socket_af_inet_socket_by_addr(wp, "::", &port_str);
                zlog::set_level(old);
                if sock < 0 {
                    zlog!(
                        NOTICE,
                        "Failed implicitly binding to ::, retrying with 0.0.0.0"
                    );
                    sock = self.socket_af_inet_socket_by_addr(wp, "0.0.0.0", &port_str);
                }
                sock
            }
        }
    }

    /// fpm_socket_af_unix_listening_socket()
    fn socket_af_unix_listening_socket(&mut self, wp: usize) -> i32 {
        let mut sa: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        let socket_length = sa.sun_path.len();
        let addr = self.pools[wp]
            .config
            .listen_address
            .clone()
            .unwrap_or_default();
        let ab = addr.as_bytes();
        let n = ab.len().min(socket_length - 1);
        for (k, &c) in ab[..n].iter().enumerate() {
            sa.sun_path[k] = c as libc::c_char;
        }
        if ab.len() >= socket_length {
            zlog!(
                WARNING,
                "[pool {}] cannot bind to UNIX socket '{}' as path is too long (found length: {}, maximal length: {}), trying cut socket path instead '{}'",
                self.pools[wp].config.name(),
                addr,
                ab.len(),
                socket_length,
                String::from_utf8_lossy(&ab[..n])
            );
        }
        sa.sun_family = libc::AF_UNIX as libc::sa_family_t;
        self.sockets_get_listening_socket(
            wp,
            &sa as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
        )
    }

    /// fpm_sockets_init_main()
    pub fn sockets_init_main(&mut self) -> i32 {
        // import inherited sockets
        for i in 0..ENV_SOCKET_SET_MAX {
            let Ok(inherited) = std::env::var(env_name(i)) else {
                break;
            };
            for item in inherited.split(',') {
                if item.is_empty() {
                    continue;
                }
                if let Some(eq) = item.find('=') {
                    let mut sockpath = item[..eq].to_string();
                    sockpath.truncate(255);
                    let fd_no = super::conf::atoi(&item[eq + 1..]) as i32;
                    let typ = domain_from_address(&sockpath);
                    zlog!(
                        NOTICE,
                        "using inherited socket fd={}, \"{}\"",
                        fd_no,
                        sockpath
                    );
                    self.sockets.push(ListeningSocket {
                        refcount: 0,
                        sock: fd_no,
                        typ,
                        key: sockpath,
                    });
                }
            }
        }
        for i in 0..self.pools.len() {
            let sock = match self.pools[i].listen_address_domain {
                Domain::Inet => self.socket_af_inet_listening_socket(i),
                Domain::Unix => {
                    if self.unix_resolve_socket_permissions(i) < 0 {
                        return -1;
                    }
                    self.socket_af_unix_listening_socket(i)
                }
            };
            self.pools[i].listening_socket = sock;
            if sock == -1 {
                return -1;
            }
            if self.pools[i].listen_address_domain == Domain::Inet {
                if let Some((_, lq_len)) = get_listening_queue(sock) {
                    scoreboard::update(
                        -1,
                        -1,
                        -1,
                        lq_len as i32,
                        -1,
                        -1,
                        0,
                        0,
                        scoreboard::ACTION_SET,
                        self.pools[i].scoreboard,
                    );
                }
            }
        }
        // close unused sockets that was inherited
        let mut k = 0;
        while k < self.sockets.len() {
            if self.sockets[k].refcount == 0 {
                let ls = self.sockets.remove(k);
                unsafe { libc::close(ls.sock) };
                if ls.typ == Domain::Unix {
                    let _ = std::fs::remove_file(&ls.key);
                }
            } else {
                k += 1;
            }
        }
        0
    }
}
