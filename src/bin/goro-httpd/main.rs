//! goro-httpd: an HTTP/HTTPS web server SAPI built on the httpsd crate.
//!
//! It runs PHP the way php-fpm behind a web server (or mod_php) does: a
//! document root, `index.php` directory indexes, scripts with a PATH_INFO,
//! static files served directly, and requests for anything that does not
//! exist routed to a front controller (`/index.php`, like WordPress'
//! `.htaccess` rewrite rules).
//!
//! Connections are served by a pool of worker threads. Each request runs
//! in a fresh engine on the worker thread that reads it (the engine is not
//! `Send`); every worker has its own current directory (unshare(CLONE_FS))
//! so scripts can change directory like php_execute_script() does.

mod handler;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::ffi::OsStrExt;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use goro::engine::{self, IniSettings};
use httpsd::session::{Session, SessionConfig};
use httpsd::tls::TlsAcceptor;

use handler::{PhpHandler, Site};

const USAGE: &str = "Usage: goro-httpd [options]

Serve a document root over HTTP and HTTPS, running PHP scripts.

  -t, --docroot DIR        document root (default: the current directory)
  -l, --listen ADDR        plain HTTP listener, e.g. 127.0.0.1:8080 (repeatable;
                           default 127.0.0.1:8080 when no listener is given)
  -s, --tls-listen ADDR    HTTPS listener (repeatable; needs a certificate)
      --tls-cert FILE      PEM certificate chain for HTTPS (leaf first)
      --tls-key FILE       PEM private key for HTTPS
      --self-signed        HTTPS with an ephemeral self-signed certificate
  -w, --workers N          worker threads (default: number of CPUs, max 32;
                           0 serves one connection at a time, in the main
                           thread, from the first listener: for debugging)
      --front-controller P script requests for missing files go to
                           (default /index.php; \"none\" disables it)
      --server-name NAME   SERVER_NAME (default: the request's Host)
      --max-body BYTES     largest request body accepted (default 64M)
      --no-compress        no gzip/deflate response compression
  -c FILE                  php.ini to load
  -n                       no php.ini (the default)
  -d KEY[=VALUE]           define an INI entry
  -q, --quiet              no access log on stderr
  -h, --help               this help
";

struct Options {
    docroot: std::path::PathBuf,
    listen: Vec<String>,
    tls_listen: Vec<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    self_signed: bool,
    workers: usize,
    front_controller: Option<String>,
    server_name: Option<String>,
    max_body: usize,
    compress: bool,
    ini: IniSettings,
    quiet: bool,
}

fn parse_size(s: &str) -> Option<usize> {
    let s = s.trim();
    let (num, mult) = match s.chars().last()?.to_ascii_lowercase() {
        'k' => (&s[..s.len() - 1], 1024),
        'm' => (&s[..s.len() - 1], 1024 * 1024),
        'g' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    num.parse::<usize>().ok().map(|n| n * mult)
}

fn parse_args() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut o = Options {
        docroot: std::env::current_dir().map_err(|e| e.to_string())?,
        listen: Vec::new(),
        tls_listen: Vec::new(),
        tls_cert: None,
        tls_key: None,
        self_signed: false,
        workers: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(32),
        front_controller: Some("/index.php".to_string()),
        server_name: None,
        max_body: 64 * 1024 * 1024,
        compress: true,
        // The engine is not interrupted by max_execution_time timers here
        // (they are per process, the workers are threads).
        ini: vec![("max_execution_time".to_string(), "0".to_string())],
        quiet: false,
    };
    let mut ini_file: Option<String> = None;
    let mut defines: Vec<(String, String)> = Vec::new();
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{} needs a value", name));
        match a.as_str() {
            "-h" | "--help" => {
                print!("{}", USAGE);
                std::process::exit(0);
            }
            "-t" | "--docroot" => o.docroot = value(&a)?.into(),
            "-l" | "--listen" => o.listen.push(value(&a)?),
            "-s" | "--tls-listen" => o.tls_listen.push(value(&a)?),
            "--tls-cert" => o.tls_cert = Some(value(&a)?),
            "--tls-key" => o.tls_key = Some(value(&a)?),
            "--self-signed" => o.self_signed = true,
            "-w" | "--workers" => {
                o.workers = value(&a)?
                    .parse::<usize>()
                    .map_err(|_| "invalid worker count".to_string())?
            }
            "--front-controller" => {
                let v = value(&a)?;
                o.front_controller = match v.as_str() {
                    "" | "none" | "off" => None,
                    p if p.starts_with('/') => Some(p.to_string()),
                    p => Some(format!("/{}", p)),
                }
            }
            "--server-name" => o.server_name = Some(value(&a)?),
            "--max-body" => {
                o.max_body = parse_size(&value(&a)?).ok_or("invalid --max-body size")?
            }
            "--no-compress" => o.compress = false,
            "-c" => ini_file = Some(value(&a)?),
            "-n" => ini_file = None,
            "-d" => {
                let d = value(&a)?;
                let (k, v) = match d.split_once('=') {
                    Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
                    None => (d.trim().to_string(), "1".to_string()),
                };
                defines.push((k, v));
            }
            "-q" | "--quiet" => o.quiet = true,
            _ => return Err(format!("unknown option: {}\n\n{}", a, USAGE)),
        }
    }
    if let Some(f) = ini_file {
        let data = std::fs::read(&f).map_err(|e| format!("{}: {}", f, e))?;
        engine::parse_ini_text(&data, &mut o.ini);
    }
    o.ini.extend(defines);
    if o.listen.is_empty() && o.tls_listen.is_empty() {
        o.listen.push("127.0.0.1:8080".to_string());
    }
    o.docroot =
        std::fs::canonicalize(&o.docroot).map_err(|e| format!("{}: {}", o.docroot.display(), e))?;
    if !o.docroot.is_dir() {
        return Err(format!("{} is not a directory", o.docroot.display()));
    }
    Ok(o)
}

fn tls_acceptor(o: &Options) -> Result<Option<TlsAcceptor>, String> {
    if o.tls_listen.is_empty() {
        return Ok(None);
    }
    let acceptor = match (&o.tls_cert, &o.tls_key) {
        (Some(c), Some(k)) => TlsAcceptor::from_pem_files(c, k)
            .map_err(|e| format!("cannot load the TLS certificate: {}", e))?,
        (None, None) if o.self_signed => {
            let mut names: Vec<&str> = vec!["localhost", "127.0.0.1", "::1"];
            if let Some(n) = &o.server_name {
                names.insert(0, n);
            }
            TlsAcceptor::self_signed(&names)
                .map_err(|e| format!("cannot make a self-signed certificate: {}", e))?
        }
        _ => {
            return Err(
                "HTTPS listeners need --tls-cert and --tls-key (or --self-signed)".to_string(),
            );
        }
    };
    Ok(Some(acceptor))
}

/// An accepted connection waiting for a worker.
struct Conn {
    stream: TcpStream,
    peer: SocketAddr,
    local: SocketAddr,
    secure: bool,
}

/// How long a connection may wait for its first request, and between the
/// requests of a kept-alive connection (a worker is tied up meanwhile).
const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(5);

fn serve_connection(c: Conn, site: &Arc<Site>, tls: Option<&TlsAcceptor>) {
    let mut stream = c.stream;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(FIRST_REQUEST_TIMEOUT));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let handler = PhpHandler::new(site.clone(), c.peer, c.local, c.secure);
    let mut cfg = SessionConfig::new(Arc::new(handler));
    cfg.server_name = Some(format!("goro-httpd (PHP/{})", goro::PHP_VERSION));
    cfg.limits.max_body_bytes = site.max_body;
    cfg.compression.enabled = site.compress;
    let mut session = match (c.secure, tls) {
        (true, Some(acceptor)) => match acceptor.accept() {
            Ok(t) => Session::tls(cfg, t),
            Err(_) => return,
        },
        _ => Session::plain(cfg),
    };
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let failed = session.received(&buf[..n]).is_err();
        // Flush the responses (and TLS records) produced, including the
        // error response of a malformed request.
        let mut wrote = false;
        loop {
            match session.to_send() {
                Ok(out) if !out.is_empty() => {
                    wrote = true;
                    if stream.write_all(&out).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(_) => return,
            }
            if !session.has_output() {
                break;
            }
        }
        if failed || session.wants_close() {
            break;
        }
        if wrote && !session.handshaking() {
            let _ = stream.set_read_timeout(Some(KEEP_ALIVE_TIMEOUT));
        }
    }
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn worker(rx: Arc<Mutex<mpsc::Receiver<Conn>>>, site: Arc<Site>, tls: Option<TlsAcceptor>) {
    // A current directory of this thread's own: scripts chdir() into
    // their directory (and may chdir() themselves).
    #[cfg(target_os = "linux")]
    unsafe {
        libc::unshare(libc::CLONE_FS);
    }
    loop {
        let conn = match rx.lock() {
            Ok(r) => r.recv(),
            Err(_) => return,
        };
        match conn {
            Ok(c) => serve_connection(c, &site, tls.as_ref()),
            Err(_) => return,
        }
    }
}

fn bind(addr: &str) -> Result<TcpListener, String> {
    TcpListener::bind(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))
}

fn main() {
    unsafe {
        // Writes to closed connections fail instead of killing the server.
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("goro-httpd: {}", e);
            std::process::exit(1);
        }
    };
    let tls = match tls_acceptor(&opts) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("goro-httpd: {}", e);
            std::process::exit(1);
        }
    };
    let mut listeners = Vec::new();
    for (addrs, secure) in [(&opts.listen, false), (&opts.tls_listen, true)] {
        for a in addrs {
            match bind(a) {
                Ok(l) => listeners.push((l, secure)),
                Err(e) => {
                    eprintln!("goro-httpd: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
    if opts.workers > 0 {
        // The max_execution_time timer is per process: with requests on
        // several threads it is not enforced (see the ini default above).
        goro_ext_standard::misc::disable_execution_timeouts();
    }
    let site = Arc::new(Site::new(
        opts.docroot.clone(),
        opts.ini.clone(),
        opts.front_controller.clone(),
        opts.server_name.clone(),
        opts.max_body,
        opts.compress,
        !opts.quiet,
    ));

    if opts.workers == 0 {
        let (listener, secure) = listeners.swap_remove(0);
        eprintln!("goro-httpd: serving in the main thread");
        for stream in listener.incoming().flatten() {
            if let (Ok(peer), Ok(local)) = (stream.peer_addr(), stream.local_addr()) {
                let conn = Conn {
                    stream,
                    peer,
                    local,
                    secure,
                };
                serve_connection(conn, &site, tls.as_ref());
            }
        }
        return;
    }
    let (tx, rx) = mpsc::sync_channel::<Conn>(1024);
    let rx = Arc::new(Mutex::new(rx));
    for i in 0..opts.workers {
        let (rx, site, tls) = (rx.clone(), site.clone(), tls.clone());
        std::thread::Builder::new()
            .name(format!("worker-{}", i))
            .stack_size(16 * 1024 * 1024)
            .spawn(move || worker(rx, site, tls))
            .expect("cannot start a worker thread");
    }

    eprintln!(
        "goro-httpd (PHP {}) serving {} with {} workers",
        goro::PHP_VERSION,
        String::from_utf8_lossy(opts.docroot.as_os_str().as_bytes()),
        opts.workers
    );
    let mut acceptors = Vec::new();
    for (listener, secure) in listeners {
        if let Ok(a) = listener.local_addr() {
            eprintln!(
                "Listening on {}://{}",
                if secure { "https" } else { "http" },
                a
            );
        }
        let tx = tx.clone();
        acceptors.push(std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                };
                let (Ok(peer), Ok(local)) = (stream.peer_addr(), stream.local_addr()) else {
                    continue;
                };
                let conn = Conn {
                    stream,
                    peer,
                    local,
                    secure,
                };
                if tx.send(conn).is_err() {
                    return;
                }
            }
        }));
    }
    drop(tx);
    for a in acceptors {
        let _ = a.join();
    }
}
