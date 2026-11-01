//! A worker process of a pool: it accepts FastCGI connections on the
//! pool's socket, reads each request, answers ping and status requests
//! itself, and runs each script request in a process forked from it for
//! that request (so requests share no PHP state), writing the access log
//! line once the request is done (PHP manual: FPM configuration, pools;
//! FastCGI Specification §3.5 for connection handling).

use std::net::IpAddr;
use std::os::fd::RawFd;
use std::sync::atomic::Ordering::Relaxed;

use super::capture::FLUSH_MARK;
use super::fcgi;
use super::log::{self, Level, now_us};
use super::master::{Master, PoolRt};
use super::scoreboard::{self, Board};
use crate::Vm;

/// Signals the processes handle through a signalfd.
pub fn signal_set() -> libc::sigset_t {
    // SAFETY: building a signal set.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGUSR1,
            libc::SIGUSR2,
            libc::SIGCHLD,
        ] {
            libc::sigaddset(&mut set, s);
        }
        set
    }
}

/// A signalfd for [`signal_set`] (the signals blocked in this thread).
pub fn make_sigfd() -> RawFd {
    let set = signal_set();
    // SAFETY: blocking the set and opening a signalfd for it.
    unsafe {
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        libc::signalfd(-1, &set, libc::SFD_NONBLOCK | libc::SFD_CLOEXEC)
    }
}

/// The signals waiting on a signalfd.
pub fn read_signals(fd: RawFd) -> Vec<i32> {
    let mut out = Vec::new();
    loop {
        // SAFETY: signalfd_siginfo is plain data.
        let mut si: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
        let n = unsafe {
            libc::read(
                fd,
                &mut si as *mut _ as *mut libc::c_void,
                std::mem::size_of::<libc::signalfd_siginfo>(),
            )
        };
        if n != std::mem::size_of::<libc::signalfd_siginfo>() as isize {
            break;
        }
        out.push(si.ssi_signo as i32);
    }
    out
}

/// Ends this process by `sig` with its default action.
pub fn die_by(sig: i32) -> ! {
    // SAFETY: restoring the default action and raising the signal.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        libc::raise(sig);
        libc::_exit(1);
    }
}

fn exit_now(code: i32) -> ! {
    // SAFETY: ends the process without the parent's exit handlers.
    unsafe { libc::_exit(code) }
}

/// listen.allowed_clients (PHP manual: "List of addresses (IPv4/IPv6)
/// of FastCGI clients which are allowed to connect").
fn allowed_clients(list: &str) -> Vec<IpAddr> {
    let mut out = Vec::new();
    for a in list.split(',') {
        let a = a.trim();
        if a.is_empty() {
            continue;
        }
        match a.parse::<IpAddr>() {
            Ok(ip) => out.push(ip),
            Err(_) => log::zlog(
                Level::Error,
                &format!("Wrong IP address '{}' in listen.allowed_clients", a),
            ),
        }
    }
    if out.is_empty() {
        log::zlog(Level::Error, "There are no allowed addresses");
    }
    out
}

fn plain_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v => v,
    }
}

/// The peer of an accepted socket (None for a Unix socket).
fn peer_ip(fd: RawFd) -> Option<IpAddr> {
    // SAFETY: sockaddr_storage is plain data filled by getpeername.
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let r = unsafe { libc::getpeername(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut len) };
    if r != 0 {
        return None;
    }
    match ss.ss_family as i32 {
        libc::AF_INET => {
            // SAFETY: the family says it is a sockaddr_in.
            let a = unsafe { &*(&ss as *const _ as *const libc::sockaddr_in) };
            Some(IpAddr::V4(std::net::Ipv4Addr::from(u32::from_be(
                a.sin_addr.s_addr,
            ))))
        }
        libc::AF_INET6 => {
            // SAFETY: the family says it is a sockaddr_in6.
            let a = unsafe { &*(&ss as *const _ as *const libc::sockaddr_in6) };
            Some(plain_ip(IpAddr::V6(std::net::Ipv6Addr::from(
                a.sin6_addr.s6_addr,
            ))))
        }
        _ => None,
    }
}

/// What the access log line needs about a finished request.
pub struct Done<'a> {
    pub req: &'a fcgi::Request,
    pub remote: String,
    pub status: i64,
    pub start_us: i64,
    pub end_us: i64,
    pub cpu: (f64, f64),
    pub memory: u64,
    pub script: Vec<u8>,
}

fn strftime(fmt: &str, t: i64) -> String {
    let tm = log::local_tm(t);
    let Ok(cf) = std::ffi::CString::new(fmt) else {
        return String::new();
    };
    let mut buf = vec![0u8; 256];
    // SAFETY: strftime into a sized buffer.
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            cf.as_ptr(),
            &tm,
        )
    };
    buf.truncate(n);
    String::from_utf8_lossy(&buf).into_owned()
}

/// An access log line (PHP manual: access.format; default
/// "%R - %u %t \"%m %r\" %s").
pub fn access_line(fmt: &str, d: &Done, pool: &str) -> Vec<u8> {
    let p = |n: &[u8]| d.req.param(n).unwrap_or(b"").to_vec();
    let uri_full = d
        .req
        .param(b"REQUEST_URI")
        .or(d.req.param(b"SCRIPT_NAME"))
        .unwrap_or(b"")
        .to_vec();
    let uri = match uri_full.iter().position(|&c| c == b'?') {
        Some(q) => uri_full[..q].to_vec(),
        None => uri_full,
    };
    let query = p(b"QUERY_STRING");
    let mut out: Vec<u8> = Vec::new();
    let b = fmt.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let mut arg: Option<String> = None;
        if b.get(i) == Some(&b'{')
            && let Some(e) = b[i..].iter().position(|&c| c == b'}')
        {
            arg = Some(String::from_utf8_lossy(&b[i + 1..i + e]).into_owned());
            i += e + 1;
        }
        let Some(&k) = b.get(i) else { break };
        i += 1;
        let dur_us = (d.end_us - d.start_us).max(0);
        match k {
            b'%' => out.push(b'%'),
            b'C' => {
                let (u, s) = d.cpu;
                let secs = dur_us as f64 / 1e6;
                let v = match arg.as_deref() {
                    Some("user") => u,
                    Some("system") => s,
                    _ => u + s,
                };
                let pct = if secs > 0.0 { v / secs * 100.0 } else { 0.0 };
                out.extend_from_slice(format!("{:.2}", pct).as_bytes());
            }
            b'd' => {
                let s = match arg.as_deref() {
                    Some("milliseconds") | Some("milli") => format!("{:.3}", dur_us as f64 / 1e3),
                    Some("microseconds") | Some("micro") => dur_us.to_string(),
                    _ => format!("{:.3}", dur_us as f64 / 1e6),
                };
                out.extend_from_slice(s.as_bytes());
            }
            b'e' => {
                if let Some(n) = &arg {
                    out.extend_from_slice(&p(n.as_bytes()));
                }
            }
            b'f' => out.extend_from_slice(&d.script),
            b'l' => out.extend_from_slice(
                d.req
                    .param(b"CONTENT_LENGTH")
                    .filter(|v| !v.is_empty())
                    .unwrap_or(b"0"),
            ),
            b'm' => out.extend_from_slice(&p(b"REQUEST_METHOD")),
            b'M' => {
                let v = match arg.as_deref() {
                    Some("kilobytes") | Some("kilo") => d.memory / 1024,
                    Some("megabytes") | Some("mega") => d.memory / (1024 * 1024),
                    _ => d.memory,
                };
                out.extend_from_slice(v.to_string().as_bytes());
            }
            b'n' => out.extend_from_slice(pool.as_bytes()),
            b'o' => out.push(b'-'),
            b'p' => out.extend_from_slice(std::process::id().to_string().as_bytes()),
            // SAFETY: getppid has no preconditions.
            b'P' => out.extend_from_slice(unsafe { libc::getppid() }.to_string().as_bytes()),
            b'q' => out.extend_from_slice(&query),
            b'Q' => {
                if !query.is_empty() {
                    out.push(b'?');
                }
            }
            b'r' => out.extend_from_slice(&uri),
            b'R' => out.extend_from_slice(d.remote.as_bytes()),
            b's' => out.extend_from_slice(d.status.to_string().as_bytes()),
            b't' | b'T' => {
                let t = if k == b't' { d.start_us } else { d.end_us } / 1_000_000;
                let f = arg
                    .as_deref()
                    .map(|a| {
                        a.strip_prefix("begin:")
                            .or(a.strip_prefix("end:"))
                            .unwrap_or(a)
                            .to_string()
                    })
                    .unwrap_or_else(|| "%d/%b/%Y:%H:%M:%S %z".into());
                out.extend_from_slice(strftime(&f, t).as_bytes());
            }
            b'u' => {
                let u = d
                    .req
                    .param(b"REMOTE_USER")
                    .filter(|v| !v.is_empty())
                    .map(|v| v.to_vec())
                    .unwrap_or_else(|| b"-".to_vec());
                out.extend_from_slice(&u);
            }
            b'i' => {
                if let Some(n) = &arg {
                    let mut v = b"HTTP_".to_vec();
                    v.extend(n.bytes().map(|c| {
                        if c == b'-' {
                            b'_'
                        } else {
                            c.to_ascii_uppercase()
                        }
                    }));
                    let val = p(&v);
                    if val.is_empty() {
                        out.push(b'-');
                    } else {
                        out.extend_from_slice(&val);
                    }
                }
            }
            other => {
                out.push(b'%');
                out.push(other);
            }
        }
    }
    out
}

/// The access.suppress_path rule: a successful GET of a listed path
/// without query string or body (tests
/// sapi/fpm/tests/log-suppress-output*.phpt).
fn suppressed(pool: &PoolRt, d: &Done) -> bool {
    if pool.cfg.access_suppress_path.is_empty() {
        return false;
    }
    let req = d.req;
    if req.param(b"REQUEST_METHOD") != Some(b"GET") {
        return false;
    }
    if !(200..300).contains(&d.status) {
        return false;
    }
    if req.param(b"QUERY_STRING").is_some_and(|q| !q.is_empty()) {
        return false;
    }
    let cl = req
        .param(b"CONTENT_LENGTH")
        .map(|v| goro_runtime::convert::str_to_int(v.trim_ascii()))
        .unwrap_or(0);
    if cl != 0 {
        return false;
    }
    let uri = req
        .param(b"REQUEST_URI")
        .or(req.param(b"SCRIPT_NAME"))
        .unwrap_or(b"");
    let path = match uri.iter().position(|&c| c == b'?') {
        Some(q) => &uri[..q],
        None => uri,
    };
    pool.cfg
        .access_suppress_path
        .iter()
        .any(|s| s.as_bytes() == path)
}

fn write_access(m: &Master, pool: &PoolRt, d: &Done) {
    let Some(fd) = pool.access_fd else { return };
    if suppressed(pool, d) {
        return;
    }
    let fmt = pool
        .cfg
        .access_format
        .clone()
        .unwrap_or_else(|| "%R - %u %t \"%m %r\" %s".into());
    let mut line = access_line(&fmt, d, &pool.cfg.name);
    // At most log_limit bytes with the newline, cut with "..." (test
    // sapi/fpm/tests/log-access-extended-limit.phpt).
    let limit = m.cfg.global.log_limit.max(16);
    if line.len() > limit - 1 {
        line.truncate(limit - 4);
        line.extend_from_slice(b"...");
    }
    line.push(b'\n');
    log::write_fd(fd, &line);
}

/// A response FPM makes itself (ping, status page).
fn own_response(id: u16, expose: bool, ctype: &str, body: &[u8]) -> Vec<u8> {
    let mut head = String::new();
    if expose {
        head.push_str("X-Powered-By: PHP/8.5.4\r\n");
    }
    head.push_str("Expires: Thu, 01 Jan 1970 00:00:00 GMT\r\n");
    head.push_str("Cache-Control: no-cache, no-store, must-revalidate, max-age=0\r\n");
    head.push_str(&format!("Content-type: {}\r\n\r\n", ctype));
    let mut data = head.into_bytes();
    data.extend_from_slice(body);
    let mut out = Vec::new();
    fcgi::stream_records(&mut out, fcgi::STDOUT, id, &data);
    fcgi::end_stream(&mut out, fcgi::STDOUT, id);
    fcgi::end_request(&mut out, id, 0, fcgi::REQUEST_COMPLETE);
    out
}

struct W<'a> {
    m: &'a Master,
    pi: usize,
    slot: usize,
    sigfd: RawFd,
    quit: bool,
    vm: Vm,
    allowed: Option<Vec<IpAddr>>,
    catch: bool,
    expose: bool,
    served: i64,
}

impl<'a> W<'a> {
    fn pool(&self) -> &'a PoolRt {
        let m: &'a Master = self.m;
        &m.pools[self.pi]
    }

    /// The pool whose status and ping this pool answers.
    fn main_pool(&self) -> &'a PoolRt {
        let m: &'a Master = self.m;
        match self.pool().status_of {
            Some(i) => &m.pools[i],
            None => self.pool(),
        }
    }

    fn board(&self) -> &'a Board {
        &self.pool().board
    }

    fn signals(&mut self) {
        for s in read_signals(self.sigfd) {
            match s {
                libc::SIGTERM | libc::SIGINT => die_by(libc::SIGTERM),
                libc::SIGQUIT => self.quit = true,
                _ => {}
            }
        }
    }

    fn run(&mut self) -> ! {
        let listen = self.pool().listener.fd;
        loop {
            let sl = self.board().slot(self.slot);
            sl.state.store(scoreboard::IDLE, Relaxed);
            if self.quit {
                exit_now(0);
            }
            let mut fds = [
                libc::pollfd {
                    fd: listen,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.sigfd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: a valid pollfd array.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if n < 0 {
                continue;
            }
            if fds[1].revents != 0 {
                self.signals();
                if self.quit {
                    exit_now(0);
                }
            }
            if fds[0].revents == 0 {
                continue;
            }
            // SAFETY: accepting on our listening socket.
            let conn = unsafe {
                libc::accept4(
                    listen,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    libc::SOCK_CLOEXEC,
                )
            };
            if conn < 0 {
                continue;
            }
            let peer = peer_ip(conn);
            if let (Some(list), Some(ip)) = (&self.allowed, peer)
                && !list.iter().any(|a| plain_ip(*a) == ip)
            {
                log::zlog(
                    Level::Error,
                    &format!(
                        "Connection disallowed: IP address '{}' has been dropped.",
                        ip
                    ),
                );
                // SAFETY: closing the connection we accepted.
                unsafe { libc::close(conn) };
                continue;
            }
            self.connection(conn, peer);
            // SAFETY: closing the connection we accepted.
            unsafe { libc::close(conn) };
            let max = self.pool().cfg.max_requests;
            if max > 0 && self.served >= max {
                exit_now(0);
            }
        }
    }

    fn connection(&mut self, conn: RawFd, peer: Option<IpAddr>) {
        loop {
            let sl = self.board().slot(self.slot);
            sl.state.store(scoreboard::READING, Relaxed);
            let sigfd = self.sigfd;
            let quit = &mut self.quit;
            let mut on_signal = || {
                for s in read_signals(sigfd) {
                    match s {
                        libc::SIGTERM | libc::SIGINT => die_by(libc::SIGTERM),
                        libc::SIGQUIT => *quit = true,
                        _ => {}
                    }
                }
                *quit
            };
            let mut watch = fcgi::Watch {
                fd: sigfd,
                on_signal: &mut on_signal,
            };
            match fcgi::read_request(conn, &mut watch, 1) {
                fcgi::Incoming::Closed | fcgi::Incoming::Interrupted => return,
                fcgi::Incoming::Request(r) => {
                    let keep = self.request(conn, &r, peer);
                    if !keep || !r.keep_conn || self.quit {
                        return;
                    }
                    let max = self.pool().cfg.max_requests;
                    if max > 0 && self.served >= max {
                        return;
                    }
                }
            }
        }
    }

    /// Handles one request; false when the connection cannot go on.
    fn request(&mut self, conn: RawFd, r: &fcgi::Request, peer: Option<IpAddr>) -> bool {
        let main = self.main_pool();
        let head = main.board.head();
        head.accepted.fetch_add(1, Relaxed);
        self.served += 1;
        let sl = self.board().slot(self.slot);
        let start = now_us();
        sl.requests.fetch_add(1, Relaxed);
        sl.req_start.store(start, Relaxed);
        sl.req_end.store(0, Relaxed);
        sl.done.store(0, Relaxed);
        sl.slow_logged.store(0, Relaxed);
        sl.status.store(200, Relaxed);
        sl.memory.store(0, Relaxed);
        sl.method.set(r.param(b"REQUEST_METHOD").unwrap_or(b""));
        sl.uri.set(
            r.param(b"REQUEST_URI")
                .or(r.param(b"SCRIPT_NAME"))
                .unwrap_or(b""),
        );
        sl.query.set(r.param(b"QUERY_STRING").unwrap_or(b""));
        sl.script.set(r.param(b"SCRIPT_FILENAME").unwrap_or(b""));
        sl.user.set(r.param(b"REMOTE_USER").unwrap_or(b""));
        sl.content_length.store(
            r.param(b"CONTENT_LENGTH")
                .map(|v| goro_runtime::convert::str_to_int(v.trim_ascii()).max(0) as u64)
                .unwrap_or(0),
            Relaxed,
        );
        sl.state.store(scoreboard::RUNNING, Relaxed);
        let (_, active) = self.board().counts();
        self.board().head().max_active.fetch_max(active, Relaxed);
        if self.pool().status_of.is_some() {
            main.board
                .head()
                .max_active
                .fetch_max(main.board.counts().1, Relaxed);
        }

        let script_name = r
            .param(b"SCRIPT_NAME")
            .map(|v| v.to_vec())
            .unwrap_or_default();
        let mcfg = &main.cfg;
        let own = if mcfg.ping_path.as_deref().map(str::as_bytes) == Some(&script_name[..]) {
            Some((
                200,
                own_response(
                    r.id,
                    self.expose,
                    "text/plain",
                    mcfg.ping_response.as_bytes(),
                ),
            ))
        } else if mcfg.status_path.as_deref().map(str::as_bytes) == Some(&script_name[..]) {
            let st = super::status::Status::of(
                &main.board,
                &mcfg.name,
                mcfg.pm().name(),
                mcfg.listen_backlog,
                main.listener.fd,
            );
            let q = String::from_utf8_lossy(r.param(b"QUERY_STRING").unwrap_or(b"")).into_owned();
            let (ctype, body) = st.page(&q);
            Some((200, own_response(r.id, self.expose, ctype, body.as_bytes())))
        } else if self.pool().status_of.is_some() {
            // The status socket serves the status page only.
            let mut out = Vec::new();
            fcgi::stream_records(
                &mut out,
                fcgi::STDOUT,
                r.id,
                b"Status: 404 Not Found\r\nContent-type: text/html; charset=UTF-8\r\n\r\nFile not found.\n",
            );
            fcgi::end_stream(&mut out, fcgi::STDOUT, r.id);
            fcgi::end_request(&mut out, r.id, 0, fcgi::REQUEST_COMPLETE);
            Some((404, out))
        } else {
            None
        };
        match own {
            Some((status, out)) => {
                // logged before the answer, so that the log follows the
                // order of the requests
                self.finish_request(r, peer, status, start, (0.0, 0.0));
                let _ = fcgi::write_all(conn, &out);
                true
            }
            None => {
                let (logged, ok) = self.run_child(conn, r, peer, start);
                if !logged {
                    let st = sl.status.load(Relaxed);
                    self.finish_request(r, peer, st, start, (0.0, 0.0));
                }
                ok
            }
        }
    }

    /// The end of a request: the scoreboard, the flush mark of the
    /// worker's output, the access log line.
    fn finish_request(
        &self,
        r: &fcgi::Request,
        peer: Option<IpAddr>,
        status: i64,
        start: i64,
        cpu: (f64, f64),
    ) {
        let sl = self.board().slot(self.slot);
        let end = now_us();
        sl.req_end.store(end, Relaxed);
        sl.state.store(scoreboard::FINISHING, Relaxed);
        sl.last_cpu.store(
            {
                let secs = ((end - start).max(1)) as f64 / 1e6;
                ((cpu.0 + cpu.1) / secs * 100.0).to_bits()
            },
            Relaxed,
        );
        sl.last_memory.store(sl.memory.load(Relaxed), Relaxed);
        if self.catch {
            // ends the request's output lines in the master's log
            log::write_fd(1, FLUSH_MARK);
            log::write_fd(2, FLUSH_MARK);
        }
        let remote = match peer {
            Some(ip) => ip.to_string(),
            None => String::from_utf8_lossy(r.param(b"REMOTE_ADDR").unwrap_or(b"-")).into_owned(),
        };
        let d = Done {
            req: r,
            remote,
            status,
            start_us: start,
            end_us: end,
            cpu,
            memory: sl.memory.load(Relaxed),
            script: r.param(b"SCRIPT_FILENAME").unwrap_or(b"").to_vec(),
        };
        write_access(self.m, self.pool(), &d);
    }

    /// Runs a script request in a forked process; (the request process
    /// ended the request, the connection is still usable).
    fn run_child(
        &mut self,
        conn: RawFd,
        r: &fcgi::Request,
        peer: Option<IpAddr>,
        start: i64,
    ) -> (bool, bool) {
        // SAFETY: this process runs one thread; the child continues with
        // a copy of it and ends with _exit.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return (false, false);
        }
        if pid == 0 {
            self.child(conn, r, peer, start);
        }
        let sl = self.board().slot(self.slot);
        let mut st = 0;
        // SAFETY: rusage is plain data.
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        loop {
            // SAFETY: waiting for our own child without blocking.
            let w = unsafe { libc::wait4(pid, &mut st, libc::WNOHANG, &mut ru) };
            if w == pid {
                break;
            }
            if w < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                break;
            }
            let mut p = libc::pollfd {
                fd: self.sigfd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd.
            unsafe { libc::poll(&mut p, 1, 1000) };
            for s in read_signals(self.sigfd) {
                match s {
                    libc::SIGTERM | libc::SIGINT => {
                        // SAFETY: stopping and reaping our own child.
                        unsafe {
                            libc::kill(pid, libc::SIGKILL);
                            libc::waitpid(pid, &mut st, 0);
                        }
                        die_by(libc::SIGTERM);
                    }
                    libc::SIGQUIT => self.quit = true,
                    _ => {}
                }
            }
        }
        let ok = libc::WIFEXITED(st);
        let done = sl.done.load(Relaxed) != 0;
        if !done {
            // The request process died before ending the response.
            let mut out = Vec::new();
            fcgi::end_stream(&mut out, fcgi::STDOUT, r.id);
            fcgi::end_request(&mut out, r.id, 0, fcgi::REQUEST_COMPLETE);
            let _ = fcgi::write_all(conn, &out);
        }
        (ok && done, ok && done)
    }

    /// The request process.
    fn child(&mut self, conn: RawFd, r: &fcgi::Request, peer: Option<IpAddr>, start: i64) -> ! {
        // SAFETY: signal setup of the new process.
        unsafe {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            libc::close(self.sigfd);
            for s in 1..32 {
                if s != libc::SIGKILL && s != libc::SIGSTOP {
                    libc::signal(s, libc::SIG_DFL);
                }
            }
            libc::signal(libc::SIGPIPE, libc::SIG_IGN);
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
        }
        let main = self.main_pool();
        let pool = self.pool();
        super::request::set_active(super::request::Active {
            fd: conn,
            id: r.id,
            keep_conn: r.keep_conn,
            finished: false,
            stderr_sent: 0,
            pending_head: Vec::new(),
            log_limit: self.m.cfg.global.log_limit,
            log_notices: log::level() <= Level::Notice,
            board: &main.board as *const Board,
            slot: self.slot,
            pool: main.cfg.name.clone(),
            pm: main.cfg.pm().name(),
            listen_backlog: main.cfg.listen_backlog,
            listen_fd: main.listener.fd,
        });
        let exts: Vec<String> = pool
            .cfg
            .security_limit_extensions
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        let admin: Vec<String> = pool
            .cfg
            .php_admin_values
            .iter()
            .filter(|(k, v)| super::request::ini_value_ok(k, v))
            .map(|(k, _)| k.clone())
            .collect();
        let set = super::request::Settings {
            limit_extensions: &exts,
            admin_keys: &admin,
        };
        let status = super::request::run(&mut self.vm, r, &set);
        let sl = self.board().slot(self.slot);
        sl.status.store(status, Relaxed);
        // SAFETY: rusage is plain data filled by getrusage.
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
        let tv = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        self.finish_request(r, peer, status, start, (tv(ru.ru_utime), tv(ru.ru_stime)));
        super::request::finish_response(0);
        exit_now(0);
    }
}

/// The worker's settings for the engine: the base ones, then the pool's
/// php_value and php_admin_value entries (an admin value the setting
/// refuses is left out).
fn engine_ini(m: &Master, pool: &PoolRt) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut ini = m.base_ini.clone();
    for (k, v) in &pool.cfg.php_values {
        ini.push((k.as_bytes().to_vec(), v.as_bytes().to_vec()));
    }
    for (k, v) in &pool.cfg.php_admin_values {
        if super::request::ini_value_ok(k, v) {
            ini.push((k.as_bytes().to_vec(), v.as_bytes().to_vec()));
        }
    }
    ini
}

/// The worker process: never returns.
pub fn run(m: &Master, pi: usize, slot: usize, out: Option<RawFd>, err: Option<RawFd>) -> ! {
    // SAFETY: process setup of the new worker.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
        if libc::getppid() != m.pid {
            libc::_exit(0);
        }
    }
    let pool = &m.pools[pi];
    pool.board
        .slot(slot)
        .pid
        .store(std::process::id() as i32, Relaxed);
    log::set_worker();
    let devnull = |flags| {
        // SAFETY: opening /dev/null.
        unsafe { libc::open(c"/dev/null".as_ptr(), flags) }
    };
    // SAFETY: rearranging this process's standard descriptors.
    unsafe {
        let n = devnull(libc::O_RDONLY);
        if n >= 0 {
            libc::dup2(n, 0);
            libc::close(n);
        }
        match (out, err) {
            (Some(o), Some(e)) => {
                libc::dup2(o, 1);
                libc::dup2(e, 2);
                libc::close(o);
                libc::close(e);
            }
            _ => {
                let n = devnull(libc::O_WRONLY);
                if n >= 0 {
                    libc::dup2(n, 1);
                    libc::dup2(n, 2);
                    libc::close(n);
                }
            }
        }
        // what only the master uses
        libc::close(m.sigfd);
        if let Some(fd) = m.error_log_fd
            && fd > 2
        {
            libc::close(fd);
        }
        for (j, p) in m.pools.iter().enumerate() {
            for c in &p.children {
                for cap in [&c.out, &c.err].into_iter().flatten() {
                    libc::close(cap.fd);
                }
            }
            if j != pi && Some(j) != pool.status_of && p.listener.fd >= 0 {
                libc::close(p.listener.fd);
            }
        }
    }
    // The environment (PHP manual: clear_env, env[]).
    if pool.cfg.clear_env {
        for (k, _) in std::env::vars_os() {
            // SAFETY: this process runs one thread.
            unsafe { std::env::remove_var(k) };
        }
    } else {
        // SAFETY: this process runs one thread.
        unsafe { std::env::remove_var("FPM_SOCKETS") };
    }
    for (k, v) in &pool.cfg.env {
        if !k.is_empty() && !k.contains('=') {
            // SAFETY: this process runs one thread.
            unsafe { std::env::set_var(k, v) };
        }
    }
    if let Some(d) = &pool.cfg.chdir {
        let _ = std::env::set_current_dir(pool.cfg.path(d, &m.prefix));
    }
    let sigfd = make_sigfd();
    let allowed = pool
        .cfg
        .listen_allowed_clients
        .as_deref()
        .filter(|_| pool.listener.unix.is_none())
        .map(allowed_clients);
    // (PHP manual: opcache.enable "can only be disabled at runtime")
    let admin_opcache = pool
        .cfg
        .php_admin_values
        .iter()
        .any(|(k, v)| k == "opcache.enable" && !matches!(v.as_str(), "" | "0"));
    let base_off = m
        .base_ini
        .iter()
        .any(|(k, v)| k == b"opcache.enable" && (v.is_empty() || v == b"0"));
    if admin_opcache && base_off {
        log::zlog(
            Level::Warning,
            "Zend OPcache can't be temporarily enabled. Are you using php_admin_value[opcache.enable]=1 in an individual pool's configuration?",
        );
    }
    let ini = engine_ini(m, pool);
    let vm = crate::startup_with(&ini, &[&super::request::FPM_MODULE], |vm| {
        vm.out.sapi.cgi = true;
        vm.out.sapi.fpm = true;
    });
    let expose = match vm.get_ini(b"expose_php") {
        None => true,
        Some(v) => !matches!(
            &v.trim_ascii().to_ascii_lowercase()[..],
            b"0" | b"off" | b"" | b"false" | b"no" | b"none"
        ),
    };
    let mut w = W {
        m,
        pi,
        slot,
        sigfd,
        quit: false,
        vm,
        allowed,
        catch: out.is_some(),
        expose,
        served: 0,
    };
    w.run()
}
