//! Embedded MySQL/MariaDB fixture for mysqli tests.
//!
//! At test-runner startup we try to:
//!   1. Locate a `mariadbd` or `mysqld` binary.
//!   2. Initialize a fresh data directory in `$TMPDIR/goro-mysqli-<pid>/`.
//!   3. Spawn the daemon listening on an ephemeral TCP port + unix socket.
//!   4. Wait up to ~10s for it to be ready.
//!   5. Create the `test` database.
//!   6. Export connection params via env vars so getenv("MYSQL_TEST_*") works
//!      from inside the PHP tests (forked child processes inherit env).
//!
//! On test-runner shutdown (Drop) we SIGTERM the daemon and rm -rf the tmpdir.
//!
//! If the binary is missing or startup fails, we just don't set the env vars
//! and mysqli tests will keep being skipped — no regression.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct EmbeddedMysql {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub passwd: String,
    pub db: String,
    pub socket: PathBuf,
    pub tmpdir: PathBuf,
    pub child: Option<Child>,
}

impl Drop for EmbeddedMysql {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Try to gracefully shut down with SIGTERM, then wait briefly.
            #[cfg(unix)]
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if start.elapsed() > Duration::from_secs(3) {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.tmpdir);
    }
}

fn find_mysqld() -> Option<PathBuf> {
    let candidates = [
        "/usr/sbin/mariadbd",
        "/usr/bin/mariadbd",
        "/usr/sbin/mysqld",
        "/usr/bin/mysqld",
        "/usr/local/sbin/mariadbd",
        "/usr/local/bin/mariadbd",
        "/usr/local/sbin/mysqld",
        "/usr/local/bin/mysqld",
    ];
    for c in candidates {
        if std::path::Path::new(c).exists() {
            return Some(PathBuf::from(c));
        }
    }
    None
}

/// A command for the server binaries, run without the address space limit
/// the goro binary sets for itself (mysqld needs much more virtual memory).
fn server_command<S: AsRef<std::ffi::OsStr>>(bin: S) -> Command {
    let mut cmd = Command::new(bin);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: only async-signal-safe libc calls between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                let mut lim = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::getrlimit(libc::RLIMIT_AS, &mut lim) == 0 {
                    lim.rlim_cur = lim.rlim_max;
                    libc::setrlimit(libc::RLIMIT_AS, &lim);
                }
                Ok(())
            });
        }
    }
    cmd
}

/// Find a free TCP port by binding a socket and asking the OS.
fn find_free_port() -> Option<u16> {
    use std::net::TcpListener;
    TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

/// Attempt to start the embedded server. Returns None on any failure.
pub fn try_start() -> Option<EmbeddedMysql> {
    let bin = find_mysqld()?;
    let is_mariadb = bin
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.contains("mariadb"))
        .unwrap_or(false);

    let pid = std::process::id();
    let tmpdir = std::env::temp_dir().join(format!("goro-mysqli-{}", pid));
    // Clean up any stale dir
    let _ = std::fs::remove_dir_all(&tmpdir);
    std::fs::create_dir_all(&tmpdir).ok()?;
    let datadir = tmpdir.join("data");
    let socket = tmpdir.join("mysql.sock");
    let pidfile = tmpdir.join("mysql.pid");
    let errlog = tmpdir.join("err.log");
    let port = find_free_port()?;

    // 1. Initialize the data directory.
    let init_status = if is_mariadb {
        let install_db = ["/usr/bin/mariadb-install-db", "/usr/bin/mysql_install_db"]
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(PathBuf::from);
        if let Some(install_bin) = install_db {
            server_command(install_bin)
                .arg(format!("--datadir={}", datadir.display()))
                .arg("--auth-root-authentication-method=normal")
                .arg("--skip-test-db")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
        } else {
            server_command(&bin)
                .arg("--initialize-insecure")
                .arg(format!("--datadir={}", datadir.display()))
                .arg(format!("--log-error={}", errlog.display()))
                .arg(format!("--socket={}", socket.display()))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
        }
    } else {
        server_command(&bin)
            .arg("--initialize-insecure")
            .arg(format!("--datadir={}", datadir.display()))
            .arg(format!("--log-error={}", errlog.display()))
            .arg(format!("--socket={}", socket.display()))
            .arg("--skip-mysqlx")
            .arg("--innodb-buffer-pool-size=16M")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };

    match init_status {
        Ok(s) if s.success() => {}
        _ => {
            let _ = std::fs::remove_dir_all(&tmpdir);
            return None;
        }
    }

    // 2. Spawn the daemon.
    let mut cmd = server_command(&bin);
    cmd.arg(format!("--datadir={}", datadir.display()))
        .arg(format!("--socket={}", socket.display()))
        .arg(format!("--pid-file={}", pidfile.display()))
        .arg(format!("--log-error={}", errlog.display()))
        .arg(format!("--port={}", port))
        .arg("--bind-address=127.0.0.1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !is_mariadb {
        cmd.arg("--skip-mysqlx")
            .arg("--innodb-buffer-pool-size=16M")
            .arg("--default-authentication-plugin=mysql_native_password")
            // Each test connects fresh. Raise the connection caps and
            // shorten timeouts so connections recycle fast under load.
            .arg("--max-connections=1000")
            .arg("--max-connect-errors=100000")
            .arg("--wait-timeout=60")
            .arg("--interactive-timeout=60")
            .arg("--thread-cache-size=64")
            .arg("--performance-schema=OFF")
            // LOAD DATA LOCAL INFILE tests need it
            .arg("--local-infile=1")
            .arg("--skip-log-bin");
    } else {
        cmd.arg("--skip-networking=0")
            .arg("--local-infile=1")
            .arg("--max-connections=1000")
            .arg("--skip-log-bin");
    }

    // The server must not outlive the runner, even when the runner is
    // killed (timeouts, ^C): have the kernel send it SIGTERM when its
    // parent goes away.
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = cmd.spawn().ok()?;

    // 3. Wait for socket to be ready.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ready = false;
    while Instant::now() < deadline {
        if socket.exists() {
            if std::net::TcpStream::connect_timeout(
                &format!("127.0.0.1:{}", port).parse().ok()?,
                Duration::from_millis(200),
            )
            .is_ok()
            {
                ready = true;
                break;
            }
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        if let Ok(mut f) = std::fs::File::open(&errlog) {
            let mut buf = String::new();
            let _ = f.read_to_string(&mut buf);
            eprintln!(
                "[goro-phpt] embedded mysqld failed to start; tail of err.log:\n{}",
                buf.lines()
                    .rev()
                    .take(20)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        let _ = std::fs::remove_dir_all(&tmpdir);
        return None;
    }

    // 4. Create the `test` database. Use a quick TCP+MySQL handshake via
    //    a minimal client: shell out to the system `mysql` client if present.
    let mysql_cli = ["/usr/bin/mysql", "/usr/local/bin/mysql", "/usr/bin/mariadb"]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(PathBuf::from);
    if let Some(cli) = mysql_cli {
        let _ = Command::new(&cli)
            .arg(format!("--socket={}", socket.display()))
            .arg("-uroot")
            .arg("-e")
            .arg("CREATE DATABASE IF NOT EXISTS test;")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    Some(EmbeddedMysql {
        host: "127.0.0.1".to_string(),
        port,
        user: "root".to_string(),
        passwd: String::new(),
        db: "test".to_string(),
        socket,
        tmpdir,
        child: Some(child),
    })
}

// ── Global state ──────────────────────────────────────────────────────

static FIXTURE: Mutex<Option<EmbeddedMysql>> = Mutex::new(None);
static INIT: std::sync::Once = std::sync::Once::new();
static AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Initialize the embedded mysql fixture and set MYSQL_TEST_* env vars.
/// Idempotent: safe to call multiple times.
/// Returns true if the fixture is available.
pub fn ensure_started() -> bool {
    INIT.call_once(|| {
        // An externally provided server (MYSQL_TEST_* already set, as for
        // php-src's run-tests.php) is used as is.
        if std::env::var_os("MYSQL_TEST_HOST").is_some()
            && std::env::var_os("GORO_MYSQL_FIXTURE").is_none()
        {
            AVAILABLE.store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        if let Some(fx) = try_start() {
            // Export env vars BEFORE spawning any test child processes.
            unsafe {
                std::env::set_var("MYSQL_TEST_HOST", &fx.host);
                std::env::set_var("MYSQL_TEST_PORT", fx.port.to_string());
                std::env::set_var("MYSQL_TEST_USER", &fx.user);
                std::env::set_var("MYSQL_TEST_PASSWD", &fx.passwd);
                std::env::set_var("MYSQL_TEST_DB", &fx.db);
                std::env::set_var("MYSQL_TEST_SOCKET", fx.socket.display().to_string());
                std::env::set_var("MYSQL_TEST_SKIP_CONNECT_FAILURE", "0");
            }
            AVAILABLE.store(true, std::sync::atomic::Ordering::SeqCst);
            *FIXTURE.lock().unwrap() = Some(fx);
        }
    });
    AVAILABLE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Is the embedded mysql server up and connectable?
pub fn is_available() -> bool {
    AVAILABLE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Stop the embedded server (called at test-runner exit).
pub fn shutdown() {
    let mut guard = FIXTURE.lock().unwrap();
    *guard = None;
}
