//! FPM's own log (PHP manual: FPM global configuration, error_log,
//! log_level, log_limit): lines "[DD-Mon-YYYY HH:MM:SS] LEVEL: message"
//! in the master, "LEVEL: message" on a worker's stderr (which the master
//! decorates when it catches workers' output). Line formats as the tests
//! read them (sapi/fpm/tests/logtool.inc).

use std::cell::RefCell;
use std::os::fd::RawFd;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Debug,
    Notice,
    Warning,
    Error,
    Alert,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "debug" => Level::Debug,
            "notice" => Level::Notice,
            "warning" => Level::Warning,
            "error" => Level::Error,
            "alert" => Level::Alert,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Notice => "NOTICE",
            Level::Warning => "WARNING",
            Level::Error => "ERROR",
            Level::Alert => "ALERT",
        }
    }
}

struct State {
    level: Level,
    /// Where lines go.
    fd: RawFd,
    /// A worker: no time stamp (the master adds its own).
    worker: bool,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            level: Level::Notice,
            fd: 2,
            worker: false,
        })
    };
}

pub fn set_level(l: Level) {
    STATE.with(|s| s.borrow_mut().level = l);
}

pub fn level() -> Level {
    STATE.with(|s| s.borrow().level)
}

pub fn set_fd(fd: RawFd) {
    STATE.with(|s| s.borrow_mut().fd = fd);
}

pub fn fd() -> RawFd {
    STATE.with(|s| s.borrow().fd)
}

/// From now on lines are a worker's: "LEVEL: message" on stderr.
pub fn set_worker() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.worker = true;
        s.fd = 2;
    });
}

pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The local time as a libc tm.
pub fn local_tm(t: i64) -> libc::tm {
    // SAFETY: localtime_r with valid pointers.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    unsafe {
        libc::localtime_r(&tt, &mut tm);
    }
    tm
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_us() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// "[30-Oct-2026 12:00:00]".
pub fn stamp() -> String {
    let tm = local_tm(now_secs());
    format!(
        "[{:02}-{}-{} {:02}:{:02}:{:02}]",
        tm.tm_mday,
        MONTHS[tm.tm_mon.clamp(0, 11) as usize],
        tm.tm_year + 1900,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

pub fn write_fd(fd: RawFd, b: &[u8]) {
    let _ = super::fcgi::write_all(fd, b);
}

/// Logs a message at `level` (when log_level lets it through).
pub fn zlog(level: Level, msg: &str) {
    let (min, fd, worker) = STATE.with(|s| {
        let s = s.borrow();
        (s.level, s.fd, s.worker)
    });
    if level < min {
        return;
    }
    let line = if worker {
        format!("{}: {}\n", level.name(), msg)
    } else {
        format!("{} {}: {}\n", stamp(), level.name(), msg)
    };
    write_fd(fd, line.as_bytes());
}

/// A message about a pool: "[pool NAME] message".
pub fn pool(level: Level, pool: &str, msg: &str) {
    zlog(level, &format!("[pool {}] {}", pool, msg));
}
