//! Embedded PostgreSQL fixture for the pgsql and pdo_pgsql tests.
//!
//! Like the MySQL fixture (mysqli_test.rs): at runner startup, when tests
//! of ext/pgsql or ext/pdo_pgsql are selected, we
//!   1. locate `initdb` and `postgres` (PATH, /usr/lib/postgresql/<v>/bin,
//!      /usr/lib64/postgresql-<v>/bin, /usr/pgsql-<v>/bin),
//!   2. initdb a fresh cluster in `$TMPDIR/goro-pgsql-<pid>/` (the
//!      `postgres` superuser with password "postgres", SCRAM over TCP),
//!   3. start the server on an ephemeral TCP port and a unix socket in the
//!      temp dir, with fsync off,
//!   4. create the `test` database,
//!   5. export PGSQL_TEST_CONNSTR / PDO_PGSQL_TEST_DSN.
//!
//! The server gets PR_SET_PDEATHSIG so it never outlives the runner, and is
//! stopped (and its directory removed) at the end. Without binaries, or if
//! anything fails, nothing is exported and the tests skip as before.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct EmbeddedPostgres {
    pub port: u16,
    pub tmpdir: PathBuf,
    pub child: Option<Child>,
}

impl Drop for EmbeddedPostgres {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // "fast" shutdown: SIGINT
            unsafe {
                libc::kill(child.id() as i32, libc::SIGINT);
            }
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if start.elapsed() > Duration::from_secs(5) {
                            unsafe {
                                libc::kill(child.id() as i32, libc::SIGQUIT);
                            }
                            std::thread::sleep(Duration::from_millis(200));
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

/// The directory holding both `initdb` and `postgres`
fn find_bindir() -> Option<PathBuf> {
    let has = |d: &Path| d.join("postgres").is_file() && d.join("initdb").is_file();
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            if has(&d) {
                return Some(d);
            }
        }
    }
    // versioned installs, newest first
    let mut cands: Vec<(u32, PathBuf)> = Vec::new();
    for (parent, prefix) in [
        ("/usr/lib/postgresql", ""),
        ("/usr/lib64", "postgresql-"),
        ("/usr/lib", "postgresql-"),
        ("/usr", "pgsql-"),
    ] {
        let Ok(rd) = std::fs::read_dir(parent) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(v) = name.strip_prefix(prefix) else {
                continue;
            };
            let Ok(v) = v.split('.').next().unwrap_or("").parse::<u32>() else {
                continue;
            };
            let bin = e.path().join("bin");
            if has(&bin) {
                cands.push((v, bin));
            }
        }
    }
    cands.sort_by_key(|c| std::cmp::Reverse(c.0));
    cands.into_iter().next().map(|c| c.1)
}

/// A server command without the address space limit of the goro binary
fn server_command<S: AsRef<std::ffi::OsStr>>(bin: S) -> Command {
    let mut cmd = Command::new(bin);
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
    cmd
}

fn find_free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

const PASSWORD: &str = "postgres";

pub fn try_start() -> Option<EmbeddedPostgres> {
    // initdb refuses to run as root
    if unsafe { libc::geteuid() } == 0 {
        return None;
    }
    let bindir = find_bindir()?;
    let pid = std::process::id();
    let mut tmpdir = std::env::temp_dir().join(format!("goro-pgsql-{}", pid));
    // the socket path ("<dir>/.s.PGSQL.<port>") must fit in sun_path
    if tmpdir.as_os_str().len() > 80 {
        tmpdir = PathBuf::from(format!("/tmp/goro-pgsql-{}", pid));
    }
    let _ = std::fs::remove_dir_all(&tmpdir);
    std::fs::create_dir_all(&tmpdir).ok()?;
    let datadir = tmpdir.join("data");
    let pwfile = tmpdir.join("pw");
    let log = tmpdir.join("server.log");
    std::fs::write(&pwfile, format!("{}\n", PASSWORD)).ok()?;
    let port = find_free_port()?;

    let st = server_command(bindir.join("initdb"))
        .arg("-D")
        .arg(&datadir)
        .arg("-U")
        .arg("postgres")
        .arg(format!("--pwfile={}", pwfile.display()))
        .arg("--auth-local=trust")
        .arg("--auth-host=scram-sha-256")
        .arg("-E")
        .arg("UTF8")
        .arg("--locale=C")
        .arg("--no-sync")
        .env_remove("PGDATA")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !matches!(st, Ok(s) if s.success()) {
        let _ = std::fs::remove_dir_all(&tmpdir);
        return None;
    }

    let mut cmd = server_command(bindir.join("postgres"));
    cmd.arg("-D")
        .arg(&datadir)
        .arg("-k")
        .arg(&tmpdir)
        .arg("-p")
        .arg(port.to_string())
        .arg("-c")
        .arg("listen_addresses=127.0.0.1")
        .arg("-c")
        .arg("fsync=off")
        .arg("-c")
        .arg("full_page_writes=off")
        .arg("-c")
        .arg("synchronous_commit=off")
        .arg("-c")
        .arg("max_connections=300")
        .arg("-c")
        .arg("lc_messages=C")
        .arg("-c")
        .arg("logging_collector=off")
        .arg("-c")
        .arg("log_min_messages=fatal")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(
            std::fs::File::create(&log)
                .map(Stdio::from)
                .unwrap_or(Stdio::null()),
        );
    {
        use std::os::unix::process::CommandExt;
        // The server must not outlive the runner, even when the runner is
        // killed: the kernel sends it SIGQUIT when its parent goes away.
        // SAFETY: only async-signal-safe libc calls between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGQUIT);
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn().ok()?;
    let fx = |child: Child| EmbeddedPostgres {
        port,
        tmpdir: tmpdir.clone(),
        child: Some(child),
    };

    // wait until it accepts connections
    let sock = tmpdir.join(format!(".s.PGSQL.{}", port));
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut ready = false;
    while Instant::now() < deadline {
        if sock.exists()
            && std::net::TcpStream::connect_timeout(
                &format!("127.0.0.1:{}", port).parse().ok()?,
                Duration::from_millis(200),
            )
            .is_ok()
        {
            let c = goro_ext_pgsql::libpq::PgConn::connectdb(
                format!(
                    "host={} port={} dbname=postgres user=postgres",
                    tmpdir.display(),
                    port
                )
                .as_bytes(),
            );
            if c.status == goro_ext_pgsql::libpq::ConnStatus::Ok {
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
        if let Ok(buf) = std::fs::read_to_string(&log) {
            let tail: Vec<&str> = buf.lines().rev().take(20).collect();
            eprintln!(
                "[goro-phpt] embedded postgres failed to start; tail of its log:\n{}",
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            );
        }
        drop(fx(child));
        return None;
    }
    let mut c = goro_ext_pgsql::libpq::PgConn::connectdb(
        format!(
            "host={} port={} dbname=postgres user=postgres",
            tmpdir.display(),
            port
        )
        .as_bytes(),
    );
    let ok = matches!(
        c.exec(b"CREATE DATABASE test"),
        Some(r) if r.status == goro_ext_pgsql::libpq::ExecStatus::CommandOk
    );
    c.finish();
    if !ok {
        drop(fx(child));
        return None;
    }
    Some(fx(child))
}

static FIXTURE: Mutex<Option<EmbeddedPostgres>> = Mutex::new(None);
static INIT: std::sync::Once = std::sync::Once::new();
static AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Start the fixture once and export the tests' environment. An external
/// server named by PGSQL_TEST_CONNSTR / PDO_PGSQL_TEST_DSN is used as is.
pub fn ensure_started() -> bool {
    INIT.call_once(|| {
        if std::env::var_os("PGSQL_TEST_CONNSTR").is_some()
            || std::env::var_os("PDO_PGSQL_TEST_DSN").is_some()
        {
            AVAILABLE.store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        if let Some(fx) = try_start() {
            let conn = format!(
                "host=127.0.0.1 port={} dbname=test user=postgres password={}",
                fx.port, PASSWORD
            );
            unsafe {
                std::env::set_var("PGSQL_TEST_CONNSTR", &conn);
                std::env::set_var("PDO_PGSQL_TEST_DSN", format!("pgsql:{}", conn));
            }
            AVAILABLE.store(true, std::sync::atomic::Ordering::SeqCst);
            *FIXTURE.lock().unwrap() = Some(fx);
        }
    });
    AVAILABLE.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn is_available() -> bool {
    AVAILABLE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Whether `path` is a test of the pgsql fixture
pub fn is_pgsql_test(path: &str) -> bool {
    path.contains("/ext/pgsql/tests/") || path.contains("/ext/pdo_pgsql/tests/")
}

/// In the forked child of a pdo_pgsql test: the generic PDO test helpers
/// (PDOTest::factory) connect with PDOTEST_*, which the MySQL fixture may
/// have pointed at MySQL. ext/pdo_pgsql/tests/config.inc only overrides
/// PDOTEST_DSN.
pub fn child_env(filename: &str) {
    if !filename.contains("/ext/pdo_pgsql/tests/") {
        return;
    }
    if let Ok(dsn) = std::env::var("PDO_PGSQL_TEST_DSN") {
        unsafe {
            std::env::set_var("PDOTEST_DSN", dsn);
            std::env::remove_var("PDOTEST_USER");
            std::env::remove_var("PDOTEST_PASS");
            std::env::remove_var("PDOTEST_ATTR");
        }
    }
}

pub fn shutdown() {
    let mut guard = FIXTURE.lock().unwrap();
    *guard = None;
}
