//! The built-in web server, `goro -S addr:port [-t docroot] [router.php]`
//! (PHP manual: Built-in web server, Command line options).
//!
//! The server process accepts connections and reads requests; each
//! complete request runs in a forked process with a fresh copy of the
//! started engine, so requests share no PHP state. With
//! PHP_CLI_SERVER_WORKERS=n (n > 1) that many worker processes accept on
//! the same socket, and die with the server (test
//! sapi/cli/tests/php_cli_server_pdeathsig.phpt).
//!
//! - [`http1`]: the HTTP/1.x layer (parsing, response transport).
//! - [`request`]: the PHP side of a request.
//! - [`upload`]: multipart/form-data bodies.

pub mod http1;
pub mod request;
pub mod upload;

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::registry::Module;
use crate::{Args, R, Vm};

/// What `goro -S` was given.
pub struct ServerOptions {
    /// "host:port" or "[v6]:port".
    pub addr: Vec<u8>,
    /// `-t`; the current directory by default.
    pub docroot: Option<Vec<u8>>,
    /// The router script.
    pub router: Option<Vec<u8>>,
    /// `-d` settings.
    pub ini: Vec<(Vec<u8>, Vec<u8>)>,
}

/// The server's settings, shared by its requests.
pub struct Config {
    pub docroot: PathBuf,
    pub docroot_str: Vec<u8>,
    /// The router as given (SCRIPT_FILENAME) and its absolute path.
    pub router: Option<(Vec<u8>, PathBuf)>,
    /// The host as given (SERVER_NAME).
    pub host: Vec<u8>,
    pub port: u16,
}

/// Functions only this SAPI has (PHP manual: getallheaders(),
/// apache_request_headers(), apache_response_headers()).
pub static SERVER_MODULE: Module = Module {
    name: "cli_server",
    functions: &[
        ("function getallheaders(): array", getallheaders),
        ("function apache_request_headers(): array", getallheaders),
        (
            "function apache_response_headers(): array",
            apache_response_headers,
        ),
    ],
    listed: false,
    ..Module::EMPTY
};

fn getallheaders(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut a = PhpArray::new();
    for (n, v) in &vm.out.sapi.request_headers {
        a.insert(Key::from_bytes(n), Value::Str(PhpStr::from(v.clone())));
    }
    Ok(Value::Array(a))
}

fn apache_response_headers(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    crate::builtins::headers::commit(vm);
    let mut a = PhpArray::new();
    for h in &vm.out.sapi.headers {
        if let Some(p) = h.iter().position(|&c| c == b':') {
            a.insert(
                Key::from_bytes(h[..p].trim_ascii()),
                Value::Str(PhpStr::from(h[p + 1..].trim_ascii().to_vec())),
            );
        }
    }
    Ok(Value::Array(a))
}

/// The local time as the server log shows it: "Wed Oct 28 22:00:10 2026".
fn log_date() -> String {
    // SAFETY: time and localtime_r with valid pointers.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        libc::localtime_r(&t, &mut tm);
    }
    format!(
        "{} {} {:02} {:02}:{:02}:{:02} {}",
        crate::vm::sapi::DAYS[tm.tm_wday.clamp(0, 6) as usize],
        crate::vm::sapi::MONTHS[tm.tm_mon.clamp(0, 11) as usize],
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        tm.tm_year + 1900
    )
}

/// A server log line on stderr (observed on PHP 8.5: "[date] message").
pub fn log(msg: &str) {
    let line = format!("[{}] {}\n", log_date(), msg);
    let _ = std::io::stderr().write_all(line.as_bytes());
}

/// Splits "host:port" / "[v6]:port".
fn split_addr(a: &[u8]) -> Option<(Vec<u8>, u16)> {
    let (host, port) = if a.first() == Some(&b'[') {
        let close = a.iter().position(|&c| c == b']')?;
        let rest = a[close + 1..].strip_prefix(b":")?;
        (a[1..close].to_vec(), rest)
    } else {
        let p = a.iter().rposition(|&c| c == b':')?;
        (a[..p].to_vec(), &a[p + 1..])
    };
    let port = std::str::from_utf8(port).ok()?.parse::<u16>().ok()?;
    Some((host, port))
}

/// Runs the server; returns the exit status if it stops.
pub fn serve(opts: ServerOptions) -> i32 {
    let addr = String::from_utf8_lossy(&opts.addr).into_owned();
    let Some((host, port)) = split_addr(&opts.addr) else {
        // (observed on PHP 8.5)
        eprintln!("Invalid address: {}", addr);
        return 1;
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let docroot = match &opts.docroot {
        Some(d) => {
            let p = cwd.join(std::ffi::OsStr::from_bytes(d));
            if !p.exists() {
                eprintln!("Directory {} does not exist.", String::from_utf8_lossy(d));
                return 1;
            }
            if !p.is_dir() {
                eprintln!("{} is not a directory.", String::from_utf8_lossy(d));
                return 1;
            }
            p
        }
        None => cwd.clone(),
    };
    let docroot = std::fs::canonicalize(&docroot).unwrap_or(docroot);
    // The router is found once: a script changing directory does not
    // lose it (test sapi/cli/tests/php_cli_server_003).
    let router = opts.router.as_ref().map(|r| {
        let p = cwd.join(std::ffi::OsStr::from_bytes(r));
        let p = if p.is_file() {
            p
        } else {
            docroot.join(std::ffi::OsStr::from_bytes(r))
        };
        (r.clone(), std::fs::canonicalize(&p).unwrap_or(p))
    });
    let listener = match bind(&host, port) {
        Ok(l) => l,
        Err(e) => {
            log(&format!("Failed to listen on {} (reason: {})", addr, e));
            return 1;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    // The cli-server SAPI's defaults (observed on PHP 8.5: errors shown
    // as HTML, no implicit flush, a 30 s time limit), then -d.
    let mut ini: Vec<(Vec<u8>, Vec<u8>)> = [
        ("display_errors", "1"),
        ("html_errors", "1"),
        ("implicit_flush", "0"),
        ("max_execution_time", "30"),
    ]
    .iter()
    .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
    .collect();
    ini.extend(opts.ini);
    let mut vm = crate::startup(&ini, &[&SERVER_MODULE]);
    vm.constants.insert(
        PhpStr::intern(b"PHP_SAPI"),
        Value::Str(PhpStr::from("cli-server")),
    );
    vm.out.sapi.server = true;
    let cfg = Config {
        docroot_str: docroot.as_os_str().as_bytes().to_vec(),
        docroot,
        router,
        host: if opts.addr.first() == Some(&b'[') {
            let mut h = b"[".to_vec();
            h.extend_from_slice(&host);
            h.push(b']');
            h
        } else {
            host.clone()
        },
        port,
    };
    log(&format!(
        "PHP 8.5.4 Development Server (http://{}:{}) started",
        String::from_utf8_lossy(&cfg.host),
        port
    ));
    let workers = std::env::var("PHP_CLI_SERVER_WORKERS")
        .ok()
        .and_then(|w| w.trim().parse::<u32>().ok())
        .unwrap_or(1);
    if workers > 1 {
        let mut pids = Vec::new();
        for _ in 0..workers {
            // SAFETY: the engine thread is the only one doing work; the
            // worker continues with a copy of this process.
            let pid = unsafe { libc::fork() };
            if pid == 0 {
                #[cfg(target_os = "linux")]
                // SAFETY: plain prctl; a worker dies with the server.
                unsafe {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                }
                run_loop(&listener, &mut vm, &cfg);
            }
            if pid > 0 {
                pids.push(pid);
            }
        }
        for pid in pids {
            let mut st = 0;
            // SAFETY: waiting for our own children.
            unsafe { libc::waitpid(pid, &mut st, 0) };
        }
        return 0;
    }
    run_loop(&listener, &mut vm, &cfg)
}

fn bind(host: &[u8], port: u16) -> Result<std::net::TcpListener, String> {
    let addrs = crate::builtins::sockets::resolve(host, port, false)?;
    let mut err = String::from("Cannot assign requested address");
    for sa in addrs {
        match std::net::TcpListener::bind(sa) {
            Ok(l) => return Ok(l),
            Err(e) => err = crate::builtins::exec::strerror(e.raw_os_error().unwrap_or(0)),
        }
    }
    Err(err)
}

/// A connection whose request is still arriving.
struct Pending {
    stream: std::net::TcpStream,
    peer: std::net::SocketAddr,
    buf: Vec<u8>,
}

/// Longest request path served (longer requests are dropped; test
/// sapi/cli/tests/bug73630a).
const MAX_PATH: usize = 4096;

fn run_loop(listener: &std::net::TcpListener, vm: &mut Vm, cfg: &Config) -> ! {
    let _ = listener.set_nonblocking(true);
    let mut conns: Vec<Pending> = Vec::new();
    loop {
        // Finished requests.
        loop {
            let mut st = 0;
            // SAFETY: reaping our own children without blocking.
            if unsafe { libc::waitpid(-1, &mut st, libc::WNOHANG) } <= 0 {
                break;
            }
        }
        let mut fds: Vec<libc::pollfd> = std::iter::once(listener.as_raw_fd())
            .chain(conns.iter().map(|c| c.stream.as_raw_fd()))
            .map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        // SAFETY: fds is a valid array of pollfd.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 200) };
        if n <= 0 {
            continue;
        }
        for i in (0..conns.len()).rev() {
            if fds[i + 1].revents == 0 {
                continue;
            }
            let mut eof = false;
            let mut chunk = [0u8; 16384];
            loop {
                match conns[i].stream.read(&mut chunk) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => conns[i].buf.extend_from_slice(&chunk[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        eof = true;
                        break;
                    }
                }
            }
            match http1::parse(&conns[i].buf) {
                http1::Parse::Done(req) => {
                    let c = conns.remove(i);
                    let path_len = req
                        .target
                        .iter()
                        .position(|&b| b == b'?')
                        .unwrap_or(req.target.len());
                    if path_len > MAX_PATH {
                        log(&format!(
                            "{} Invalid request (Requested URI is too long)",
                            c.peer
                        ));
                        log(&format!("{} Closing", c.peer));
                        continue;
                    }
                    dispatch(listener, &conns, vm, cfg, req, c);
                }
                http1::Parse::Invalid(why) => {
                    let c = conns.remove(i);
                    log(&format!("{} Invalid request ({})", c.peer, why));
                    log(&format!("{} Closing", c.peer));
                }
                http1::Parse::Incomplete if eof => {
                    let c = conns.remove(i);
                    if !c.buf.is_empty() {
                        log(&format!("{} Invalid request (Unexpected EOF)", c.peer));
                    }
                    log(&format!("{} Closing", c.peer));
                }
                http1::Parse::Incomplete => {}
            }
        }
        if fds[0].revents != 0 {
            loop {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        let _ = stream.set_nonblocking(true);
                        log(&format!("{} Accepted", peer));
                        conns.push(Pending {
                            stream,
                            peer,
                            buf: Vec::new(),
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
    }
}

/// Runs a request in its own process.
fn dispatch(
    listener: &std::net::TcpListener,
    others: &[Pending],
    vm: &mut Vm,
    cfg: &Config,
    req: http1::Request,
    c: Pending,
) {
    // SAFETY: the engine thread is the only one doing work; the child
    // continues with a copy of this process and exits when done.
    let pid = unsafe { libc::fork() };
    if pid != 0 {
        if pid < 0 {
            log(&format!("{} Closing", c.peer));
        }
        return;
    }
    // SAFETY: closing descriptors only the parent uses.
    unsafe {
        libc::close(listener.as_raw_fd());
        for o in others {
            libc::close(o.stream.as_raw_fd());
        }
    }
    let _ = c.stream.set_nonblocking(false);
    let peer = c.peer;
    let stream = c.stream;
    let keep = stream.try_clone();
    request::handle(vm, cfg, req, stream, peer);
    // Drop whatever the response holds, then close.
    vm.out.sapi.conn = None;
    if let Ok(mut s) = keep {
        // End the response, then let the client read it before the
        // socket goes: unread input at close would reset the connection.
        let _ = s.shutdown(std::net::Shutdown::Write);
        let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let mut sink = [0u8; 4096];
        while matches!(s.read(&mut sink), Ok(n) if n > 0) {}
    }
    log(&format!("{} Closing", peer));
    // SAFETY: ends the request's process without running the parent's
    // exit handlers.
    unsafe { libc::_exit(0) };
}
