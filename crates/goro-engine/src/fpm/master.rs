//! The master process: configuration checks, listening sockets, the
//! worker processes of each pool and their process manager (static,
//! dynamic, ondemand), workers' output, logs, and signals (PHP manual:
//! FastCGI Process Manager; FPM configuration; the signals: SIGINT and
//! SIGTERM terminate immediately, SIGQUIT terminates gracefully, SIGUSR1
//! re-opens the log files, SIGUSR2 reloads graceful).

use std::ffi::CString;
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use super::Options;
use super::capture::{Capture, Style};
use super::config::{self, Config, Pm, Pool};
use super::log::{self, Level};
use super::scoreboard::{self, Board};
use super::worker;

/// A listening socket.
pub struct Listener {
    pub fd: RawFd,
    /// The address as configured (the key of inherited sockets).
    pub addr: String,
    /// The socket file of a Unix socket.
    pub unix: Option<PathBuf>,
}

pub struct Child {
    pub pid: i32,
    pub slot: usize,
    pub started: Instant,
    pub out: Option<Capture>,
    pub err: Option<Capture>,
    pub style: Style,
    /// A SIGQUIT was sent (idle process stopped by the process manager).
    pub quitting: bool,
}

/// A pool at run time.
pub struct PoolRt {
    pub cfg: Pool,
    pub listener: Listener,
    pub board: Board,
    pub children: Vec<Child>,
    pub access_fd: Option<RawFd>,
    pub access_path: Option<PathBuf>,
    pub slow_fd: Option<RawFd>,
    /// For the hidden pool of pm.status_listen: the pool it reports.
    pub status_of: Option<usize>,
    /// Dynamic pm: processes to start at the next round.
    spawn_rate: i64,
    warned_max: bool,
    /// Ondemand: a process was started for waiting connections.
    pending_spawn: Option<Instant>,
}

pub struct Master {
    pub opts: Options,
    pub cfg: Config,
    pub prefix: PathBuf,
    pub pools: Vec<PoolRt>,
    pub sigfd: RawFd,
    pub pid: i32,
    pub base_ini: Vec<(Vec<u8>, Vec<u8>)>,
    /// The error log file, when the log goes there.
    pub error_log_fd: Option<RawFd>,
    error_log_path: Option<PathBuf>,
    pid_file: Option<PathBuf>,
    stopping: bool,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn strerror(e: i32) -> String {
    crate::builtins::exec::strerror(e)
}

fn cpath(p: &Path) -> CString {
    CString::new(p.as_os_str().as_bytes()).unwrap_or_default()
}

fn open_append(p: &Path) -> Result<RawFd, i32> {
    let c = cpath(p);
    // SAFETY: opening a file by path.
    let fd = unsafe {
        libc::open(
            c.as_ptr(),
            libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 { Err(errno()) } else { Ok(fd) }
}

fn set_cloexec(fd: RawFd, on: bool) {
    // SAFETY: fcntl on our descriptor.
    unsafe {
        let f = libc::fcntl(fd, libc::F_GETFD);
        let f = if on {
            f | libc::FD_CLOEXEC
        } else {
            f & !libc::FD_CLOEXEC
        };
        libc::fcntl(fd, libc::F_SETFD, f);
    }
}

fn set_nonblock(fd: RawFd) {
    // SAFETY: fcntl on our descriptor.
    unsafe {
        let f = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, f | libc::O_NONBLOCK);
    }
}

fn signame(s: i32) -> &'static str {
    match s {
        libc::SIGHUP => "SIGHUP",
        libc::SIGINT => "SIGINT",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGILL => "SIGILL",
        libc::SIGABRT => "SIGABRT",
        libc::SIGBUS => "SIGBUS",
        libc::SIGFPE => "SIGFPE",
        libc::SIGKILL => "SIGKILL",
        libc::SIGUSR1 => "SIGUSR1",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGUSR2 => "SIGUSR2",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGALRM => "SIGALRM",
        libc::SIGTERM => "SIGTERM",
        libc::SIGCHLD => "SIGCHLD",
        _ => "?",
    }
}

/// A user or group name, or a number.
fn lookup_user(name: &str) -> Result<u32, i32> {
    if let Ok(n) = name.parse::<u32>() {
        return Ok(n);
    }
    let c = CString::new(name).map_err(|_| 0)?;
    // SAFETY: getpwnam with a valid name; the result is read at once.
    unsafe {
        *libc::__errno_location() = 0;
        let p = libc::getpwnam(c.as_ptr());
        if p.is_null() {
            Err(errno())
        } else {
            Ok((*p).pw_uid)
        }
    }
}

fn lookup_group(name: &str) -> Result<u32, i32> {
    if let Ok(n) = name.parse::<u32>() {
        return Ok(n);
    }
    let c = CString::new(name).map_err(|_| 0)?;
    // SAFETY: getgrnam with a valid name; the result is read at once.
    unsafe {
        *libc::__errno_location() = 0;
        let p = libc::getgrnam(c.as_ptr());
        if p.is_null() {
            Err(errno())
        } else {
            Ok((*p).gr_gid)
        }
    }
}

/// The names of listen.acl_users or listen.acl_groups (comma-separated).
fn acl_names(v: &Option<String>) -> Vec<&str> {
    v.as_deref()
        .map(|l| {
            l.split(',')
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The POSIX ACL of a Unix socket (PHP manual: listen.acl_users,
/// listen.acl_groups): read and write for the named users and groups, in
/// the kernel's `system.posix_acl_access` format (a version, then tag,
/// permissions and id entries ordered by tag and id; acl(5), xattr(7)).
fn set_acl(pool: &Pool, path: &Path, mode: u32) -> Result<(), i32> {
    const USER_OBJ: u16 = 0x01;
    const USER: u16 = 0x02;
    const GROUP_OBJ: u16 = 0x04;
    const GROUP: u16 = 0x08;
    const MASK: u16 = 0x10;
    const OTHER: u16 = 0x20;
    const NO_ID: u32 = u32::MAX;
    let mut users: Vec<u32> = acl_names(&pool.listen_acl_users)
        .into_iter()
        .filter_map(|n| lookup_user(n).ok())
        .collect();
    let mut groups: Vec<u32> = acl_names(&pool.listen_acl_groups)
        .into_iter()
        .filter_map(|n| lookup_group(n).ok())
        .collect();
    if users.is_empty() && groups.is_empty() {
        return Ok(());
    }
    users.sort_unstable();
    users.dedup();
    groups.sort_unstable();
    groups.dedup();
    let mut entries = vec![(USER_OBJ, ((mode >> 6) & 7) as u16, NO_ID)];
    entries.extend(users.into_iter().map(|u| (USER, 6, u)));
    entries.push((GROUP_OBJ, ((mode >> 3) & 7) as u16, NO_ID));
    entries.extend(groups.into_iter().map(|g| (GROUP, 6, g)));
    entries.push((MASK, 6, NO_ID));
    entries.push((OTHER, (mode & 7) as u16, NO_ID));
    let mut data = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in entries {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&perm.to_le_bytes());
        data.extend_from_slice(&id.to_le_bytes());
    }
    let c = cpath(path);
    // SAFETY: setting an attribute from a valid buffer on our socket file.
    let r = unsafe {
        libc::setxattr(
            c.as_ptr(),
            c"system.posix_acl_access".as_ptr(),
            data.as_ptr() as *const libc::c_void,
            data.len(),
            0,
        )
    };
    if r == 0 { Ok(()) } else { Err(errno()) }
}

/// Longest Unix socket path (sun_path less its terminating zero).
const UNIX_PATH_MAX: usize = 107;

/// Checks the configuration as FPM does before starting; problems are
/// logged. `test` is the -t run (which also checks the users and groups
/// that a non-root run ignores).
fn check(cfg: &mut Config, test: bool) -> bool {
    let g = &cfg.global;
    if g.log_limit < 512 {
        log::zlog(Level::Error, "log_limit must be greater than 512");
        return false;
    }
    if cfg.pools.is_empty() {
        log::zlog(
            Level::Error,
            "No pool defined. at least one pool section must be specified in config file",
        );
        return false;
    }
    // SAFETY: geteuid has no preconditions.
    let root = unsafe { libc::geteuid() } == 0;
    for p in cfg.pools.iter_mut() {
        let name = p.name.clone();
        let err = |m: &str| {
            log::pool(Level::Error, &name, m);
            false
        };
        if p.listen.is_none() {
            return err("there is no 'listen' directive");
        }
        if test || root {
            if let Some(u) = &p.user
                && lookup_user(u).is_err()
            {
                return err(&format!("cannot get uid for user '{}'", u));
            }
            if let Some(gr) = &p.group
                && lookup_group(gr).is_err()
            {
                return err(&format!("cannot get gid for group '{}'", gr));
            }
        }
        let listen = p.listen.clone().unwrap_or_default();
        if listen.starts_with('/') {
            if let Some(o) = &p.listen_owner
                && let Err(e) = lookup_user(o)
            {
                let why = if e == 0 {
                    "No such user".to_string()
                } else {
                    strerror(e)
                };
                return err(&format!("cannot get uid for user '{}': {}", o, why));
            }
            if let Some(gr) = &p.listen_group
                && let Err(e) = lookup_group(gr)
            {
                let why = if e == 0 {
                    "No such group".to_string()
                } else {
                    strerror(e)
                };
                return err(&format!("cannot get gid for group '{}': {}", gr, why));
            }
            if test && listen.len() > UNIX_PATH_MAX {
                return err(&format!(
                    "cannot bind to UNIX socket '{}' as path is too long (found length: {}, maximal length: {})",
                    listen,
                    listen.len(),
                    UNIX_PATH_MAX
                ));
            }
        }
        for u in acl_names(&p.listen_acl_users) {
            if lookup_user(u).is_err() {
                return err(&format!("cannot get uid for user '{}'", u));
            }
        }
        for gr in acl_names(&p.listen_acl_groups) {
            if lookup_group(gr).is_err() {
                return err(&format!("cannot get gid for group '{}'", gr));
            }
        }
        if !test && !root {
            if p.user.is_some() {
                log::pool(
                    Level::Notice,
                    &name,
                    "'user' directive is ignored when FPM is not running as root",
                );
            }
            if p.group.is_some() {
                log::pool(
                    Level::Notice,
                    &name,
                    "'group' directive is ignored when FPM is not running as root",
                );
            }
        }
        let Some(pm) = p.pm else {
            return err("the process manager is missing (static, dynamic or ondemand)");
        };
        if p.max_children < 1 {
            return err("pm.max_children must be a positive value");
        }
        if pm == Pm::Dynamic {
            if p.min_spare_servers <= 0 {
                return err(&format!(
                    "pm.min_spare_servers({}) must be a positive value",
                    p.min_spare_servers
                ));
            }
            if p.max_spare_servers <= 0 {
                return err(&format!(
                    "pm.max_spare_servers({}) must be a positive value",
                    p.max_spare_servers
                ));
            }
            if p.min_spare_servers > p.max_children || p.max_spare_servers > p.max_children {
                return err(&format!(
                    "pm.min_spare_servers({}) and pm.max_spare_servers({}) cannot be greater than pm.max_children({})",
                    p.min_spare_servers, p.max_spare_servers, p.max_children
                ));
            }
            if p.max_spare_servers < p.min_spare_servers {
                return err(&format!(
                    "pm.max_spare_servers({}) must not be less than pm.min_spare_servers({})",
                    p.max_spare_servers, p.min_spare_servers
                ));
            }
            if p.start_servers <= 0 {
                p.start_servers =
                    p.min_spare_servers + (p.max_spare_servers - p.min_spare_servers) / 2;
                log::pool(
                    Level::Notice,
                    &name,
                    &format!(
                        "pm.start_servers is not set. It's been set to {}.",
                        p.start_servers
                    ),
                );
            } else if p.start_servers < p.min_spare_servers || p.start_servers > p.max_spare_servers
            {
                return err(&format!(
                    "pm.start_servers({}) must not be less than pm.min_spare_servers({}) and not greater than pm.max_spare_servers({})",
                    p.start_servers, p.min_spare_servers, p.max_spare_servers
                ));
            }
            if p.max_spawn_rate < 1 {
                return err(&format!(
                    "pm.max_spawn_rate({}) must be a positive value",
                    p.max_spawn_rate
                ));
            }
        }
        if pm == Pm::Ondemand && p.process_idle_timeout < 1 {
            return err(&format!(
                "pm.process_idle_timeout({}s) must be greater than 0s",
                p.process_idle_timeout
            ));
        }
        for (what, v) in [
            ("pm.status_path", &p.status_path),
            ("ping.path", &p.ping_path),
        ] {
            if let Some(v) = v
                && !v.starts_with('/')
            {
                return err(&format!("the {} '{}' must start with a '/'", what, v));
            }
        }
        if p.request_slowlog_timeout > 0 && p.slowlog.is_none() {
            return err("'slowlog' must be specified for use with 'request_slowlog_timeout'");
        }
        if p.max_requests < 0 {
            return err(&format!(
                "pm.max_requests({}) must be a positive value",
                p.max_requests
            ));
        }
    }
    true
}

/// "ip:port", "[ip6]:port", "port" or a Unix socket path.
enum Addr {
    Tcp(Vec<std::net::SocketAddr>, bool),
    Unix(PathBuf),
}

fn parse_listen(s: &str) -> Result<Addr, String> {
    if s.starts_with('/') {
        return Ok(Addr::Unix(PathBuf::from(s)));
    }
    let (host, port) = if let Some(r) = s.strip_prefix('[') {
        let close = r
            .find(']')
            .ok_or_else(|| format!("invalid address '{}'", s))?;
        let rest = r[close + 1..].strip_prefix(':').unwrap_or("");
        (Some(r[..close].to_string()), rest.to_string())
    } else if let Some(p) = s.rfind(':') {
        (Some(s[..p].to_string()), s[p + 1..].to_string())
    } else {
        (None, s.to_string())
    };
    let port: u32 = port
        .trim()
        .parse()
        .map_err(|_| format!("invalid port value '{}'", port))?;
    if port == 0 || port > 65535 {
        return Err(format!("invalid port value '{}'", port));
    }
    let port = port as u16;
    match host.as_deref() {
        None => Ok(Addr::Tcp(
            vec![std::net::SocketAddr::from(([0u16; 8], port))],
            true,
        )),
        Some("*") => Ok(Addr::Tcp(
            vec![std::net::SocketAddr::from(([0u8; 4], port))],
            false,
        )),
        Some("localhost") => Ok(Addr::Tcp(
            vec![std::net::SocketAddr::from(([127, 0, 0, 1], port))],
            false,
        )),
        Some(h) => match h.parse::<std::net::IpAddr>() {
            Ok(ip) => Ok(Addr::Tcp(vec![std::net::SocketAddr::new(ip, port)], false)),
            Err(_) => Err(format!(
                "Failed to resolve '{}': only numeric addresses are supported",
                h
            )),
        },
    }
}

fn sockaddr(a: &std::net::SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    // SAFETY: plain data, filled below for the address family.
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let len = match a {
        std::net::SocketAddr::V4(v4) => {
            let sin = libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: v4.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from(*v4.ip()).to_be(),
                },
                sin_zero: [0; 8],
            };
            // SAFETY: sockaddr_storage is large enough for sockaddr_in.
            unsafe { std::ptr::write(&mut ss as *mut _ as *mut libc::sockaddr_in, sin) };
            std::mem::size_of::<libc::sockaddr_in>()
        }
        std::net::SocketAddr::V6(v6) => {
            let sin6 = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: v6.port().to_be(),
                sin6_flowinfo: 0,
                sin6_addr: libc::in6_addr {
                    s6_addr: v6.ip().octets(),
                },
                sin6_scope_id: 0,
            };
            // SAFETY: sockaddr_storage is large enough for sockaddr_in6.
            unsafe { std::ptr::write(&mut ss as *mut _ as *mut libc::sockaddr_in6, sin6) };
            std::mem::size_of::<libc::sockaddr_in6>()
        }
    };
    (ss, len as libc::socklen_t)
}

/// Moves a listening socket above the low descriptors, so that accepted
/// connections come first in a process's descriptor list (as test
/// sapi/fpm/tests/socket-close-on-exec.phpt reads lsof's output).
fn high_fd(fd: RawFd) -> RawFd {
    // SAFETY: duplicating our descriptor, keeping close-on-exec.
    let n = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 64) };
    if n < 0 {
        return fd;
    }
    // SAFETY: closing the original.
    unsafe { libc::close(fd) };
    n
}

fn bind_tcp(a: &std::net::SocketAddr, backlog: i32) -> Result<RawFd, i32> {
    let fam = if a.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    // SAFETY: socket setup with checked results.
    unsafe {
        let fd = libc::socket(fam, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(errno());
        }
        let one: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &one as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
        if fam == libc::AF_INET6 {
            let zero: libc::c_int = 0;
            libc::setsockopt(
                fd,
                libc::IPPROTO_IPV6,
                libc::IPV6_V6ONLY,
                &zero as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
        let (ss, len) = sockaddr(a);
        if libc::bind(fd, &ss as *const _ as *const libc::sockaddr, len) != 0
            || libc::listen(fd, backlog) != 0
        {
            let e = errno();
            libc::close(fd);
            return Err(e);
        }
        // Several workers wake for one connection: the ones that lose the
        // accept() must not block in it, deaf to signals.
        let fd = high_fd(fd);
        set_nonblock(fd);
        Ok(fd)
    }
}

fn bind_unix(p: &Path, backlog: i32) -> Result<RawFd, i32> {
    let b = p.as_os_str().as_bytes();
    // SAFETY: socket setup with checked results; the path fits sun_path.
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(errno());
        }
        let mut sa: libc::sockaddr_un = std::mem::zeroed();
        sa.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (i, &c) in b.iter().take(UNIX_PATH_MAX).enumerate() {
            sa.sun_path[i] = c as libc::c_char;
        }
        let _ = std::fs::remove_file(p);
        if libc::bind(
            fd,
            &sa as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
        ) != 0
            || libc::listen(fd, backlog) != 0
        {
            let e = errno();
            libc::close(fd);
            return Err(e);
        }
        // Several workers wake for one connection: the ones that lose the
        // accept() must not block in it, deaf to signals.
        let fd = high_fd(fd);
        set_nonblock(fd);
        Ok(fd)
    }
}

/// Sockets passed on by the master before a reload: "addr=fd,addr=fd".
fn inherited() -> Vec<(String, RawFd)> {
    let Some(v) = std::env::var_os("FPM_SOCKETS") else {
        return Vec::new();
    };
    // SAFETY: this process runs one engine thread at this point.
    unsafe { std::env::remove_var("FPM_SOCKETS") };
    let v = v.to_string_lossy().into_owned();
    let mut out = Vec::new();
    for part in v.split(',') {
        if let Some(p) = part.rfind('=')
            && let Ok(fd) = part[p + 1..].parse::<RawFd>()
        {
            out.push((part[..p].to_string(), fd));
        }
    }
    out
}

impl Master {
    fn listen(
        pool: &Pool,
        addr_text: &str,
        inherited: &mut Vec<(String, RawFd)>,
    ) -> Result<Listener, ()> {
        if let Some(i) = inherited.iter().position(|(a, _)| a == addr_text) {
            let (_, fd) = inherited.remove(i);
            set_cloexec(fd, true);
            set_nonblock(fd);
            let unix = addr_text.starts_with('/').then(|| PathBuf::from(addr_text));
            return Ok(Listener {
                fd,
                addr: addr_text.to_string(),
                unix,
            });
        }
        let backlog = if pool.listen_backlog < 0 {
            libc::SOMAXCONN
        } else {
            pool.listen_backlog as i32
        };
        match parse_listen(addr_text) {
            Err(m) => {
                log::zlog(Level::Error, &m);
                Err(())
            }
            Ok(Addr::Unix(mut p)) => {
                let s = p.as_os_str().as_bytes().to_vec();
                if s.len() > UNIX_PATH_MAX {
                    let cut = PathBuf::from(std::ffi::OsStr::from_bytes(&s[..UNIX_PATH_MAX]));
                    log::pool(
                        Level::Warning,
                        &pool.name,
                        &format!(
                            "cannot bind to UNIX socket '{}' as path is too long (found length: {}, maximal length: {}), trying cut socket path instead '{}'",
                            p.display(),
                            s.len(),
                            UNIX_PATH_MAX,
                            cut.display()
                        ),
                    );
                    p = cut;
                }
                match bind_unix(&p, backlog) {
                    Ok(fd) => {
                        let c = cpath(&p);
                        let mode = pool
                            .listen_mode
                            .as_deref()
                            .and_then(|m| u32::from_str_radix(m.trim(), 8).ok())
                            .unwrap_or(0o660);
                        // SAFETY: changing our socket file's mode and owner.
                        unsafe {
                            libc::chmod(c.as_ptr(), mode as libc::mode_t);
                            let uid = pool
                                .listen_owner
                                .as_deref()
                                .and_then(|o| lookup_user(o).ok());
                            let gid = pool
                                .listen_group
                                .as_deref()
                                .and_then(|g| lookup_group(g).ok());
                            if uid.is_some() || gid.is_some() {
                                libc::chown(
                                    c.as_ptr(),
                                    uid.unwrap_or(u32::MAX),
                                    gid.unwrap_or(u32::MAX),
                                );
                            }
                        }
                        if let Err(e) = set_acl(pool, &p, mode) {
                            log::pool(
                                Level::Error,
                                &pool.name,
                                &format!(
                                    "failed to set the ACL of the socket: {} ({})",
                                    strerror(e),
                                    e
                                ),
                            );
                            // SAFETY: closing the socket we just opened.
                            unsafe { libc::close(fd) };
                            return Err(());
                        }
                        Ok(Listener {
                            fd,
                            addr: addr_text.to_string(),
                            unix: Some(p),
                        })
                    }
                    Err(e) => {
                        log::zlog(
                            Level::Error,
                            &format!(
                                "unable to bind listening socket for address '{}': {} ({})",
                                addr_text,
                                strerror(e),
                                e
                            ),
                        );
                        Err(())
                    }
                }
            }
            Ok(Addr::Tcp(addrs, implicit)) => {
                let mut last = 0;
                for a in &addrs {
                    match bind_tcp(a, backlog) {
                        Ok(fd) => {
                            return Ok(Listener {
                                fd,
                                addr: addr_text.to_string(),
                                unix: None,
                            });
                        }
                        Err(e) => last = e,
                    }
                }
                if implicit {
                    // (test sapi/fpm/tests/socket-ipv4-fallback.phpt)
                    log::zlog(
                        Level::Notice,
                        "Failed implicitly binding to ::, retrying with 0.0.0.0",
                    );
                    let port = addrs[0].port();
                    match bind_tcp(&std::net::SocketAddr::from(([0u8; 4], port)), backlog) {
                        Ok(fd) => {
                            return Ok(Listener {
                                fd,
                                addr: addr_text.to_string(),
                                unix: None,
                            });
                        }
                        Err(e) => last = e,
                    }
                }
                log::zlog(
                    Level::Error,
                    &format!(
                        "unable to bind listening socket for address '{}': {} ({})",
                        addr_text,
                        strerror(last),
                        last
                    ),
                );
                Err(())
            }
        }
    }

    fn style(&self, pi: usize, pid: i32) -> Style {
        let p = &self.pools[pi].cfg;
        Style {
            pool: p.name.clone(),
            pid,
            decorate: p.decorate_workers_output,
            limit: self.cfg.global.log_limit,
        }
    }

    /// Starts a worker of pool `pi`.
    fn spawn(&mut self, pi: usize) -> bool {
        let Some(slot) = self.pools[pi].board.alloc() else {
            return false;
        };
        let catch = self.pools[pi].cfg.catch_workers_output;
        let mk = || -> Option<(RawFd, RawFd)> {
            let mut fds = [0; 2];
            // SAFETY: a pipe for the worker's output.
            (unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0)
                .then_some((fds[0], fds[1]))
        };
        let (out, err) = if catch { (mk(), mk()) } else { (None, None) };
        // SAFETY: the engine thread forks; the child continues as a
        // worker and never returns here.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            worker::run(self, pi, slot, out.map(|p| p.1), err.map(|p| p.1));
        }
        for p in [out, err].into_iter().flatten() {
            // SAFETY: closing the write ends the worker owns.
            unsafe { libc::close(p.1) };
        }
        if pid < 0 {
            for p in [out, err].into_iter().flatten() {
                // SAFETY: closing our read ends.
                unsafe { libc::close(p.0) };
            }
            self.pools[pi].board.free(slot);
            log::pool(
                Level::Error,
                &self.pools[pi].cfg.name,
                &format!("fork() failed: {}", strerror(errno())),
            );
            return false;
        }
        self.pools[pi].board.slot(slot).pid.store(pid, Relaxed);
        let style = self.style(pi, pid);
        let caps = |p: Option<(RawFd, RawFd)>, e: bool| {
            p.map(|p| {
                set_nonblock(p.0);
                Capture::new(p.0, e)
            })
        };
        self.pools[pi].children.push(Child {
            pid,
            slot,
            started: Instant::now(),
            out: caps(out, false),
            err: caps(err, true),
            style,
            quitting: false,
        });
        log::pool(
            Level::Notice,
            &self.pools[pi].cfg.name,
            &format!("child {} started", pid),
        );
        true
    }

    /// Reaps exited workers; restarts static pools' processes.
    fn reap(&mut self) {
        loop {
            let mut st = 0;
            // SAFETY: reaping our children without blocking.
            let pid = unsafe { libc::waitpid(-1, &mut st, libc::WNOHANG) };
            if pid <= 0 {
                break;
            }
            let Some((pi, ci)) = self.pools.iter().enumerate().find_map(|(pi, p)| {
                p.children
                    .iter()
                    .position(|c| c.pid == pid)
                    .map(|ci| (pi, ci))
            }) else {
                continue;
            };
            let mut c = self.pools[pi].children.remove(ci);
            for cap in [&mut c.out, &mut c.err].into_iter().flatten() {
                cap.read(&c.style);
                cap.close(&c.style);
                // SAFETY: closing our read end.
                unsafe { libc::close(cap.fd) };
            }
            self.pools[pi].board.free(c.slot);
            let secs = c.started.elapsed().as_secs_f64();
            let name = self.pools[pi].cfg.name.clone();
            if libc::WIFEXITED(st) {
                let code = libc::WEXITSTATUS(st);
                log::pool(
                    if code == 0 {
                        Level::Notice
                    } else {
                        Level::Warning
                    },
                    &name,
                    &format!(
                        "child {} exited with code {} after {:.6} seconds from start",
                        pid, code, secs
                    ),
                );
            } else if libc::WIFSIGNALED(st) {
                let s = libc::WTERMSIG(st);
                let calm =
                    (self.stopping || c.quitting) && (s == libc::SIGTERM || s == libc::SIGQUIT);
                log::pool(
                    if calm { Level::Notice } else { Level::Warning },
                    &name,
                    &format!(
                        "child {} exited on signal {} ({}) after {:.6} seconds from start",
                        pid,
                        s,
                        signame(s),
                        secs
                    ),
                );
            }
            if !self.stopping && self.pools[pi].cfg.pm() == Pm::Static {
                self.spawn(pi);
            }
        }
    }

    fn all_children(&self) -> usize {
        self.pools.iter().map(|p| p.children.len()).sum()
    }

    fn signal_all(&mut self, sig: i32) {
        for p in &self.pools {
            for c in &p.children {
                // SAFETY: signalling our own child.
                unsafe { libc::kill(c.pid, sig) };
            }
        }
    }

    /// Reads workers' output that is waiting.
    fn read_captures(&mut self, ready: &[RawFd]) {
        for p in self.pools.iter_mut() {
            for c in p.children.iter_mut() {
                let style = c.style.clone();
                for cap in [&mut c.out, &mut c.err].into_iter().flatten() {
                    if ready.contains(&cap.fd) {
                        cap.read(&style);
                    }
                }
            }
        }
    }

    fn capture_fds(&self) -> Vec<RawFd> {
        let mut v = Vec::new();
        for p in &self.pools {
            for c in &p.children {
                for cap in [&c.out, &c.err].into_iter().flatten() {
                    v.push(cap.fd);
                }
            }
        }
        v
    }

    /// Stops all workers: `steps` are (signal, how long to wait for the
    /// processes to exit); the processes left get SIGKILL. Returns the
    /// signals that arrived meanwhile.
    fn stop_children(&mut self, steps: &[(i32, Option<Duration>)]) -> Vec<i32> {
        let mut pending = Vec::new();
        for &(sig, wait) in steps {
            if self.all_children() == 0 {
                break;
            }
            self.signal_all(sig);
            let start = Instant::now();
            while self.all_children() > 0 {
                if let Some(w) = wait
                    && start.elapsed() >= w
                {
                    break;
                }
                let got = self.wait_events(100);
                for s in got {
                    if s != libc::SIGCHLD {
                        pending.push(s);
                    }
                }
                self.reap();
            }
        }
        if self.all_children() > 0 {
            self.signal_all(libc::SIGKILL);
            while self.all_children() > 0 {
                self.wait_events(100);
                self.reap();
            }
        }
        pending
    }

    /// Waits up to `ms` for signals or worker output; returns the signals.
    fn wait_events(&mut self, ms: i32) -> Vec<i32> {
        let caps = self.capture_fds();
        let mut fds: Vec<libc::pollfd> = std::iter::once(self.sigfd)
            .chain(caps.iter().copied())
            .map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        // SAFETY: a valid pollfd array.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
        if n <= 0 {
            return Vec::new();
        }
        let ready: Vec<RawFd> = fds[1..]
            .iter()
            .filter(|f| f.revents != 0)
            .map(|f| f.fd)
            .collect();
        self.read_captures(&ready);
        if fds[0].revents != 0 {
            worker::read_signals(self.sigfd)
        } else {
            Vec::new()
        }
    }

    fn cleanup_files(&mut self, sockets: bool) {
        if let Some(p) = &self.pid_file {
            let _ = std::fs::remove_file(p);
        }
        if sockets {
            for p in &self.pools {
                if let Some(u) = &p.listener.unix {
                    let _ = std::fs::remove_file(u);
                }
            }
        }
    }

    fn terminate(&mut self, graceful: bool) -> ! {
        log::zlog(
            Level::Notice,
            if graceful {
                "Finishing ..."
            } else {
                "Terminating ..."
            },
        );
        self.stopping = true;
        let t = self.cfg.global.process_control_timeout;
        if graceful {
            let w = (t > 0).then(|| Duration::from_secs(t as u64));
            self.stop_children(&[
                (libc::SIGQUIT, w),
                (libc::SIGTERM, Some(Duration::from_secs(1))),
            ]);
        } else {
            self.stop_children(&[(libc::SIGTERM, Some(Duration::from_secs(1)))]);
        }
        self.cleanup_files(true);
        log::zlog(Level::Notice, "exiting, bye-bye!");
        // SAFETY: ending the master.
        unsafe { libc::_exit(0) };
    }

    fn reopen_logs(&mut self) {
        if let (Some(fd), Some(p)) = (self.error_log_fd, &self.error_log_path)
            && let Ok(n) = open_append(p)
        {
            // SAFETY: replacing our log descriptor in place.
            unsafe {
                libc::dup2(n, fd);
                libc::close(n);
            }
            set_cloexec(fd, true);
        }
        log::zlog(Level::Notice, "error log file re-opened");
        for p in &self.pools {
            if let (Some(fd), Some(path)) = (p.access_fd, &p.access_path)
                && let Ok(n) = open_append(path)
            {
                // SAFETY: replacing the access log descriptor in place.
                unsafe {
                    libc::dup2(n, fd);
                    libc::close(n);
                }
                set_cloexec(fd, true);
            }
        }
        log::zlog(Level::Notice, "access log file re-opened");
    }

    fn reload(&mut self) -> ! {
        log::zlog(Level::Notice, "Reloading in progress ...");
        self.stopping = true;
        let t = self.cfg.global.process_control_timeout;
        let w = (t > 0).then(|| Duration::from_secs(t as u64));
        let pending = self.stop_children(&[(libc::SIGQUIT, w), (libc::SIGTERM, w)]);
        if pending
            .iter()
            .any(|&s| s == libc::SIGTERM || s == libc::SIGINT)
        {
            self.stopping = false;
            self.terminate(false);
        }
        let argv = &self.opts.argv;
        log::zlog(
            Level::Notice,
            &format!(
                "reloading: {}",
                argv.iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        );
        let mut socks = Vec::new();
        for p in &self.pools {
            if p.listener.fd >= 0 {
                set_cloexec(p.listener.fd, false);
                socks.push(format!("{}={}", p.listener.addr, p.listener.fd));
            }
        }
        // SAFETY: this process runs one engine thread at this point.
        unsafe { std::env::set_var("FPM_SOCKETS", socks.join(",")) };
        let exe = c"/proc/self/exe";
        let args: Vec<CString> = argv
            .iter()
            .map(|a| CString::new(a.as_bytes()).unwrap_or_default())
            .collect();
        let mut ptrs: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: execv with valid NUL-terminated arguments; the blocked
        // signals stay blocked in the new image until it handles them.
        unsafe { libc::execv(exe.as_ptr(), ptrs.as_ptr()) };
        log::zlog(
            Level::Error,
            &format!("failed to reload: execv() failed: {}", strerror(errno())),
        );
        self.cleanup_files(true);
        // SAFETY: ending the master.
        unsafe { libc::_exit(1) };
    }

    /// The process manager's round (once a second): spare processes of
    /// dynamic pools, idle processes of ondemand pools, slow and
    /// timed-out requests.
    fn maintenance(&mut self) {
        let now = log::now_us();
        for pi in 0..self.pools.len() {
            let pm = self.pools[pi].cfg.pm();
            if self.pools[pi].status_of.is_some() {
                continue;
            }
            self.check_requests(pi, now);
            let p = &self.pools[pi];
            let (idle, _) = p.board.counts();
            let starting = p
                .board
                .used()
                .filter(|(_, s)| s.state.load(Relaxed) == scoreboard::STARTING)
                .count() as i64;
            let total = p.children.len() as i64;
            match pm {
                Pm::Dynamic => {
                    let idle = idle as i64 + starting;
                    if idle > p.cfg.max_spare_servers {
                        let victim = p
                            .board
                            .used()
                            .filter(|(_, s)| s.state.load(Relaxed) == scoreboard::IDLE)
                            .min_by_key(|(_, s)| s.idle_since.load(Relaxed))
                            .map(|(_, s)| s.pid.load(Relaxed));
                        if let Some(v) = victim {
                            self.quit_child(pi, v);
                        }
                        self.pools[pi].spawn_rate = 1;
                    } else if idle < p.cfg.min_spare_servers {
                        if total >= p.cfg.max_children {
                            if !p.warned_max {
                                log::pool(
                                    Level::Warning,
                                    &p.cfg.name,
                                    &format!(
                                        "server reached pm.max_children setting ({}), consider raising it",
                                        p.cfg.max_children
                                    ),
                                );
                            }
                            p.board.head().max_children_reached.fetch_add(1, Relaxed);
                            self.pools[pi].warned_max = true;
                        } else {
                            let want = (p.cfg.min_spare_servers - idle)
                                .min(p.spawn_rate)
                                .min(p.cfg.max_children - total)
                                .max(1);
                            for _ in 0..want {
                                self.spawn(pi);
                            }
                            let p = &mut self.pools[pi];
                            p.spawn_rate = (p.spawn_rate * 2).min(p.cfg.max_spawn_rate);
                        }
                    } else {
                        self.pools[pi].spawn_rate = 1;
                        self.pools[pi].warned_max = false;
                    }
                }
                Pm::Ondemand => {
                    let limit = p.cfg.process_idle_timeout * 1_000_000;
                    let victims: Vec<i32> = p
                        .board
                        .used()
                        .filter(|(_, s)| {
                            s.state.load(Relaxed) == scoreboard::IDLE
                                && now - s.idle_since.load(Relaxed) > limit
                        })
                        .map(|(_, s)| s.pid.load(Relaxed))
                        .collect();
                    for v in victims {
                        self.quit_child(pi, v);
                    }
                }
                Pm::Static => {}
            }
        }
    }

    fn quit_child(&mut self, pi: usize, pid: i32) {
        if let Some(c) = self.pools[pi].children.iter_mut().find(|c| c.pid == pid)
            && !c.quitting
        {
            c.quitting = true;
            // SAFETY: signalling our own child.
            unsafe { libc::kill(pid, libc::SIGQUIT) };
        }
    }

    /// request_slowlog_timeout and request_terminate_timeout (PHP manual).
    fn check_requests(&mut self, pi: usize, now: i64) {
        let p = &self.pools[pi];
        let slow = p.cfg.request_slowlog_timeout;
        let term = p.cfg.request_terminate_timeout;
        if slow <= 0 && term <= 0 {
            return;
        }
        let mut kills = Vec::new();
        for (_, s) in p.board.used() {
            let st = s.state.load(Relaxed);
            let running = st == scoreboard::RUNNING
                || (p.cfg.request_terminate_timeout_track_finished && st == scoreboard::FINISHING);
            if !running {
                continue;
            }
            let start = s.req_start.load(Relaxed);
            if start == 0 {
                continue;
            }
            let d = (now - start) as f64 / 1e6;
            let pid = s.pid.load(Relaxed);
            let script = String::from_utf8_lossy(&s.script.get()).into_owned();
            let req = format!(
                "{} {}",
                String::from_utf8_lossy(&s.method.get()),
                String::from_utf8_lossy(&s.uri.get())
            );
            if slow > 0 && d >= slow as f64 && s.slow_logged.load(Relaxed) == 0 {
                s.slow_logged.store(1, Relaxed);
                p.board.head().slow_requests.fetch_add(1, Relaxed);
                log::pool(
                    Level::Warning,
                    &p.cfg.name,
                    &format!(
                        "child {}, script '{}' (request: \"{}\") executing too slow ({:.6} sec), logging",
                        pid, script, req, d
                    ),
                );
                if let Some(fd) = p.slow_fd {
                    let text = format!(
                        "\n{} [pool {}] pid {}\nscript_filename = {}\n",
                        log::stamp(),
                        p.cfg.name,
                        pid,
                        script
                    );
                    log::write_fd(fd, text.as_bytes());
                }
            }
            if term > 0 && d >= term as f64 {
                log::pool(
                    Level::Warning,
                    &p.cfg.name,
                    &format!(
                        "child {}, script '{}' (request: \"{}\") execution timed out ({:.6} sec), terminating",
                        pid, script, req, d
                    ),
                );
                kills.push(pid);
            }
        }
        for pid in kills {
            // SAFETY: signalling our own child.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    /// The main loop.
    fn run_loop(&mut self) -> ! {
        let mut tick = Instant::now();
        loop {
            // ondemand pools: a connection waiting and no process to take
            // it starts one
            let mut watch: Vec<(usize, RawFd)> = Vec::new();
            for (pi, p) in self.pools.iter().enumerate() {
                if p.cfg.pm() != Pm::Ondemand || p.status_of.is_some() {
                    continue;
                }
                let (idle, _) = p.board.counts();
                let starting = p
                    .board
                    .used()
                    .any(|(_, s)| s.state.load(Relaxed) == scoreboard::STARTING);
                let recent = p
                    .pending_spawn
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(200));
                if idle == 0 && !starting && !recent {
                    watch.push((pi, p.listener.fd));
                }
            }
            let caps = self.capture_fds();
            let mut fds: Vec<libc::pollfd> = std::iter::once(self.sigfd)
                .chain(caps.iter().copied())
                .chain(watch.iter().map(|w| w.1))
                .map(|fd| libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            let wait = 1000u128.saturating_sub(tick.elapsed().as_millis()).min(200) as i32;
            // SAFETY: a valid pollfd array.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, wait) };
            let mut sigs = Vec::new();
            if n > 0 {
                let ready: Vec<RawFd> = fds[1..1 + caps.len()]
                    .iter()
                    .filter(|f| f.revents != 0)
                    .map(|f| f.fd)
                    .collect();
                self.read_captures(&ready);
                if fds[0].revents != 0 {
                    sigs = worker::read_signals(self.sigfd);
                }
                for (k, (pi, _)) in watch.iter().enumerate() {
                    if fds[1 + caps.len() + k].revents == 0 {
                        continue;
                    }
                    let p = &self.pools[*pi];
                    if (p.children.len() as i64) < p.cfg.max_children {
                        self.spawn(*pi);
                        self.pools[*pi].pending_spawn = Some(Instant::now());
                    } else {
                        p.board.head().max_children_reached.fetch_add(1, Relaxed);
                        self.pools[*pi].pending_spawn = Some(Instant::now());
                    }
                }
            }
            for s in sigs {
                match s {
                    libc::SIGTERM | libc::SIGINT => self.terminate(false),
                    libc::SIGQUIT => self.terminate(true),
                    libc::SIGUSR1 => self.reopen_logs(),
                    libc::SIGUSR2 => self.reload(),
                    _ => {}
                }
            }
            self.reap();
            if tick.elapsed() >= Duration::from_secs(1) {
                tick = Instant::now();
                self.maintenance();
            }
        }
    }
}

/// The prefix of relative paths (PHP manual: --prefix).
fn resolve(prefix: &Path, p: &str) -> PathBuf {
    let pp = Path::new(p);
    if pp.is_absolute() {
        pp.to_path_buf()
    } else {
        prefix.join(pp)
    }
}

/// Runs php-fpm with parsed options; returns the exit status.
pub fn main(opts: Options) -> i32 {
    let sigfd = worker::make_sigfd();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let prefix = opts
        .prefix
        .as_ref()
        .map(|p| resolve(&cwd, &p.to_string_lossy()))
        .unwrap_or_else(|| cwd.clone());
    let file = opts
        .config
        .as_ref()
        .map(|c| resolve(&cwd, &c.to_string_lossy()))
        .unwrap_or_else(|| prefix.join("etc/php-fpm.conf"));
    // Constants for the values (E_ALL ...): an engine with the modules.
    let consts_vm = crate::Vm::new(&crate::builtins::modules());
    let lookup = |n: &[u8]| -> Option<Vec<u8>> {
        let v = consts_vm.constants.get(&goro_runtime::PhpStr::new(n))?;
        match v {
            goro_runtime::Value::Int(i) => Some(i.to_string().into_bytes()),
            goro_runtime::Value::Str(s) => Some(s.to_vec()),
            goro_runtime::Value::Float(f) => Some(f.to_string().into_bytes()),
            goro_runtime::Value::Bool(true) => Some(b"1".to_vec()),
            goro_runtime::Value::Bool(false) | goro_runtime::Value::Null => Some(Vec::new()),
            _ => None,
        }
    };
    let mut inherited_socks = inherited();
    let fail = || {
        log::zlog(Level::Error, "FPM initialization failed");
        78
    };
    let Some(mut cfg) = config::load(&file, &prefix, &lookup) else {
        return fail();
    };
    drop(consts_vm);
    if let Some(p) = &opts.pid {
        cfg.global.pid = Some(p.to_string_lossy().into_owned());
    }
    if let Some(d) = opts.daemonize {
        cfg.global.daemonize = d;
    }
    log::set_level(cfg.global.log_level);
    let test = opts.test > 0;
    if !check(&mut cfg, test) {
        return fail();
    }
    if test {
        if opts.test > 1 {
            for l in config::dump(&cfg) {
                log::zlog(Level::Notice, &l);
            }
        }
        log::zlog(
            Level::Notice,
            &format!("configuration file {} test is successful", file.display()),
        );
        return 0;
    }
    // The error log (PHP manual: error_log; in the foreground with
    // --force-stderr, or stderr a terminal, the log goes to stderr).
    let fg = !cfg.global.daemonize;
    // SAFETY: isatty has no preconditions.
    let to_stderr = fg && (opts.force_stderr || unsafe { libc::isatty(2) } == 1);
    let mut error_log_fd = None;
    let mut error_log_path = None;
    if cfg.global.error_log != "syslog" {
        let p = resolve(&prefix, &cfg.global.error_log);
        match open_append(&p) {
            Ok(fd) => {
                if to_stderr {
                    // SAFETY: the file is only created.
                    unsafe { libc::close(fd) };
                } else {
                    log::set_fd(fd);
                    error_log_fd = Some(fd);
                    error_log_path = Some(p);
                }
            }
            Err(e) => {
                log::zlog(
                    Level::Error,
                    &format!(
                        "failed to open error_log ({}): {} ({})",
                        p.display(),
                        strerror(e),
                        e
                    ),
                );
                return fail();
            }
        }
    }
    for (addr, fd) in &inherited_socks {
        log::zlog(
            Level::Notice,
            &format!("using inherited socket fd={}, \"{}\"", fd, addr),
        );
    }
    // The pools, with the hidden ones of pm.status_listen.
    let mut pool_cfgs: Vec<(Pool, Option<usize>)> = Vec::new();
    for (i, p) in cfg.pools.iter().enumerate() {
        pool_cfgs.push((p.clone(), None));
        if let Some(sl) = &p.status_listen {
            let mut sp = p.clone();
            sp.listen = Some(sl.clone());
            sp.pm = Some(Pm::Static);
            sp.max_children = 1;
            sp.access_log = None;
            sp.status_only = true;
            sp.listen_allowed_clients = None;
            pool_cfgs.push((sp, Some(i)));
        }
    }
    // map "index in cfg.pools" to the run-time index
    let mut runtime_index = Vec::new();
    let mut pools: Vec<PoolRt> = Vec::new();
    for (pc, status_of) in pool_cfgs {
        let listen = pc.listen.clone().unwrap_or_default();
        let Ok(listener) = Master::listen(&pc, &listen, &mut inherited_socks) else {
            for p in &pools {
                // SAFETY: closing sockets we opened.
                unsafe { libc::close(p.listener.fd) };
            }
            return fail();
        };
        let mut access_fd = None;
        let mut access_path = None;
        if let Some(a) = &pc.access_log {
            let ap = pc.path(a, &prefix);
            match open_append(&ap) {
                Ok(fd) => {
                    access_fd = Some(fd);
                    access_path = Some(ap);
                }
                Err(e) => {
                    log::pool(
                        Level::Error,
                        &pc.name,
                        &format!(
                            "failed to open access log ({}): {} ({})",
                            ap.display(),
                            strerror(e),
                            e
                        ),
                    );
                    return fail();
                }
            }
        }
        let mut slow_fd = None;
        if let Some(s) = &pc.slowlog {
            let sp = pc.path(s, &prefix);
            match open_append(&sp) {
                Ok(fd) => slow_fd = Some(fd),
                Err(e) => {
                    log::pool(
                        Level::Error,
                        &pc.name,
                        &format!(
                            "failed to open slowlog ({}): {} ({})",
                            sp.display(),
                            strerror(e),
                            e
                        ),
                    );
                    return fail();
                }
            }
        }
        let nslots = pc.max_children.max(1) as usize;
        let Some(board) = Board::new(nslots) else {
            log::zlog(Level::Error, "failed to allocate the scoreboard");
            return fail();
        };
        if status_of.is_none() {
            runtime_index.push(pools.len());
        }
        pools.push(PoolRt {
            cfg: pc,
            listener,
            board,
            children: Vec::new(),
            access_fd,
            access_path,
            slow_fd,
            status_of: status_of.map(|i| runtime_index[i]),
            spawn_rate: 1,
            warned_max: false,
            pending_spawn: None,
        });
    }
    // inherited sockets no pool uses any more
    for (_, fd) in inherited_socks {
        // SAFETY: closing a descriptor passed to us.
        unsafe { libc::close(fd) };
    }
    if cfg.global.daemonize {
        daemonize();
    }
    // SAFETY: getpid has no preconditions.
    let pid = unsafe { libc::getpid() };
    let pid_file = cfg.global.pid.as_ref().map(|p| resolve(&prefix, p));
    if let Some(p) = &pid_file
        && let Err(e) = std::fs::write(p, pid.to_string())
    {
        log::zlog(
            Level::Error,
            &format!("Unable to create the PID file ({}): {}", p.display(), e),
        );
        return fail();
    }
    let base_ini = opts.ini.clone();
    let mut m = Master {
        opts,
        cfg,
        prefix,
        pools,
        sigfd,
        pid,
        base_ini,
        error_log_fd,
        error_log_path,
        pid_file,
        stopping: false,
    };
    log::zlog(Level::Notice, &format!("fpm is running, pid {}", pid));
    for pi in 0..m.pools.len() {
        let n = match m.pools[pi].cfg.pm() {
            Pm::Static => m.pools[pi].cfg.max_children,
            Pm::Dynamic => m.pools[pi].cfg.start_servers,
            Pm::Ondemand => 0,
        };
        for _ in 0..n {
            m.spawn(pi);
        }
    }
    log::zlog(Level::Notice, "ready to handle connections");
    m.run_loop()
}

/// Runs in the background (PHP manual: daemonize): the parent exits; the
/// daemon's standard descriptors go to /dev/null. It stays in the
/// process group (no setsid), so whoever started it can still stop the
/// group.
fn daemonize() {
    // SAFETY: the engine thread forks; the parent ends at once.
    let pid = unsafe { libc::fork() };
    if pid > 0 {
        // SAFETY: ending the parent without its exit handlers.
        unsafe { libc::_exit(0) };
    }
    if pid < 0 {
        return;
    }
    // SAFETY: redirecting our standard descriptors.
    unsafe {
        let n = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if n >= 0 {
            libc::dup2(n, 0);
            libc::dup2(n, 1);
            libc::dup2(n, 2);
            libc::close(n);
        }
    }
}
