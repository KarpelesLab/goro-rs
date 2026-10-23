//! sapi/fpm/fpm/fpm_log.c: the access log.

use super::Fpm;
use super::scoreboard::{self, Proc};
use super::zlog::{self, NOTICE, SYSERROR, WARNING, ZlogStream};
use crate::zlog;

/// What the access log of a finished request is made of (the SG() and
/// fcgi parts fpm_log_write() reads).
pub struct LogRequest<'a> {
    pub response_code: i64,
    pub content_length: i64,
    pub headers: &'a [(Vec<u8>, Vec<u8>)],
    pub getenv: &'a dyn Fn(&[u8]) -> Option<Vec<u8>>,
}

impl Fpm {
    /// fpm_log_open()
    pub fn log_open(&mut self, reopen: bool) -> i32 {
        let mut ret = 1;
        for i in 0..self.pools.len() {
            let Some(path) = self.pools[i].config.access_log.clone() else {
                continue;
            };
            ret = 0;
            let cp = std::ffi::CString::new(path.clone()).unwrap_or_default();
            let mut fd = unsafe {
                libc::open(
                    cp.as_ptr(),
                    libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT,
                    (libc::S_IRUSR | libc::S_IWUSR) as libc::c_uint,
                )
            };
            if fd < 0 {
                zlog!(SYSERROR, "failed to open access log ({})", path);
                return -1;
            }
            zlog!(zlog::DEBUG, "open access log ({})", path);
            if reopen {
                unsafe {
                    libc::dup2(fd, self.pools[i].log_fd);
                    libc::close(fd);
                }
                fd = self.pools[i].log_fd;
                self.pctl_kill_all(libc::SIGQUIT);
            } else {
                self.pools[i].log_fd = fd;
            }
            unsafe {
                let f = libc::fcntl(fd, libc::F_GETFD);
                if libc::fcntl(fd, libc::F_SETFD, f | libc::FD_CLOEXEC) < 0 {
                    zlog!(WARNING, "failed to change attribute of access_log");
                }
            }
        }
        ret
    }
}

/// fpm_log_write() with a format to test (access.format validation).
pub fn check_format(fmt: &str) -> Result<(), ()> {
    write_format(fmt, None, None, None).map(|_| ())
}

/// fpm_access_log_suppress()
fn suppress(proc: &Proc, req: &LogRequest, paths: &[String]) -> bool {
    if !scoreboard::cstr(&proc.query_string).is_empty() {
        return false;
    }
    let m = scoreboard::cstr(&proc.request_method);
    if m != b"GET" && m != b"HEAD" {
        return false;
    }
    if req.response_code < 200 || req.response_code > 299 {
        return false;
    }
    if req.content_length > 0 {
        return false;
    }
    let uri = scoreboard::cstr(&proc.request_uri);
    paths.iter().any(|p| p.as_bytes() == uri)
}

/// fpm_log_write() of a request: writes the line to `stream`.
pub fn write_request(
    fmt: &str,
    stream: &mut ZlogStream,
    req: &LogRequest,
    suppress_paths: &[String],
) -> Result<(), ()> {
    let sb = scoreboard::get();
    if sb.is_null() {
        zlog!(
            WARNING,
            "unable to get scoreboard while preparing the access log"
        );
        return Err(());
    }
    let Some(pp) = scoreboard::proc_acquire(std::ptr::null_mut(), -1, false) else {
        zlog!(
            WARNING,
            "[pool {}] Unable to acquire shm slot while preparing the access log",
            String::from_utf8_lossy(scoreboard::cstr(unsafe { &(*sb).pool }))
        );
        return Err(());
    };
    let proc = unsafe { *pp };
    scoreboard::proc_release(pp);
    if suppress(&proc, req, suppress_paths) {
        return Err(());
    }
    let pool = String::from_utf8_lossy(scoreboard::cstr(unsafe { &(*sb).pool })).into_owned();
    write_format(fmt, Some(stream), Some((&proc, req)), Some(&pool))
}

fn strftime_or(fmt: &str, t: i64) -> String {
    zlog::strftime(fmt, t)
}

/// The formatting loop of fpm_log_write(). Without a request it only
/// validates the format (`test`).
fn write_format(
    fmt: &str,
    mut stream: Option<&mut ZlogStream>,
    data: Option<(&Proc, &LogRequest)>,
    pool: Option<&str>,
) -> Result<(), ()> {
    let test = data.is_none();
    let now_epoch = scoreboard::epoch();
    let s = fmt.as_bytes();
    let mut i = 0usize;
    let mut token = false;
    let mut format = String::new();
    if let Some(st) = stream.as_deref_mut() {
        st.start();
    }
    macro_rules! out_str {
        ($b:expr) => {
            if let Some(st) = stream.as_deref_mut() {
                st.str($b);
            }
        };
    }
    macro_rules! over_limit {
        () => {
            stream.as_deref().is_some_and(|st| st.is_over_limit())
        };
    }
    while i < s.len() {
        if !token && s[i] == b'%' {
            token = true;
            format.clear();
            i += 1;
            continue;
        }
        if token {
            token = false;
            let c = s[i];
            match c {
                b'%' => {
                    if let Some(st) = stream.as_deref_mut() {
                        st.char(b'%');
                    }
                }
                b'C' => {
                    let f = format.to_ascii_lowercase();
                    if !(f.is_empty() || f == "total" || f == "user" || f == "system") {
                        zlog!(
                            WARNING,
                            "only 'total', 'user' or 'system' are allowed as a modifier for %{} ('{}')",
                            c as char,
                            format
                        );
                        return Err(());
                    }
                    if let Some((proc, _)) = data {
                        let t = &proc.last_request_cpu;
                        let total = if f == "user" {
                            t.utime + t.cutime
                        } else if f == "system" {
                            t.stime + t.cstime
                        } else {
                            t.utime + t.stime + t.cutime + t.cstime
                        };
                        let d = proc.cpu_duration.sec as f64
                            + proc.cpu_duration.usec as f64 / 1_000_000.0;
                        let v = total as f64 / scoreboard::tick() / d * 100.0;
                        out_str!(format!("{:.2}", v).as_bytes());
                    }
                    format.clear();
                }
                b'd' => {
                    let f = format.to_ascii_lowercase();
                    let d = data.map(|(p, _)| p.duration).unwrap_or_default();
                    let v = if f.is_empty() || f == "seconds" {
                        format!("{:.3}", d.sec as f64 + d.usec as f64 / 1_000_000.0)
                    } else if matches!(
                        f.as_str(),
                        "milliseconds" | "milli" | "miliseconds" | "mili"
                    ) {
                        format!("{:.3}", d.sec as f64 * 1000.0 + d.usec as f64 / 1000.0)
                    } else if f == "microseconds" || f == "micro" {
                        format!("{}", d.sec as u64 * 1_000_000 + d.usec as u64)
                    } else {
                        zlog!(
                            WARNING,
                            "only 'seconds', 'milli', 'milliseconds', 'micro' or 'microseconds' are allowed as a modifier for %{} ('{}')",
                            c as char,
                            format
                        );
                        return Err(());
                    };
                    if !test {
                        out_str!(v.as_bytes());
                    }
                    format.clear();
                }
                b'e' => {
                    if format.is_empty() {
                        zlog!(
                            WARNING,
                            "the name of the environment variable must be set between embraces for %{}",
                            c as char
                        );
                        return Err(());
                    }
                    if let Some((_, req)) = data {
                        let v = (req.getenv)(format.as_bytes());
                        out_str!(v.as_deref().unwrap_or(b"-"));
                    }
                    format.clear();
                }
                b'f' => {
                    if let Some((proc, _)) = data {
                        let v = scoreboard::cstr(&proc.script_filename);
                        out_str!(if v.is_empty() { b"-" } else { v });
                    }
                }
                b'l' => {
                    if let Some((proc, _)) = data {
                        out_str!(format!("{}", proc.content_length).as_bytes());
                    }
                }
                b'm' => {
                    if let Some((proc, _)) = data {
                        let v = scoreboard::cstr(&proc.request_method);
                        out_str!(if v.is_empty() { b"-" } else { v });
                    }
                }
                b'M' => {
                    let f = format.to_ascii_lowercase();
                    let m = data.map(|(p, _)| p.memory).unwrap_or(0);
                    let v = if f.is_empty() || f == "bytes" {
                        m
                    } else if f == "kilobytes" || f == "kilo" {
                        m / 1024
                    } else if f == "megabytes" || f == "mega" {
                        m / 1024 / 1024
                    } else {
                        zlog!(
                            WARNING,
                            "only 'bytes', 'kilo', 'kilobytes', 'mega' or 'megabytes' are allowed as a modifier for %{} ('{}')",
                            c as char,
                            format
                        );
                        return Err(());
                    };
                    if !test {
                        out_str!(format!("{}", v).as_bytes());
                    }
                    format.clear();
                }
                b'n' => {
                    if !test {
                        let p = pool.unwrap_or("");
                        out_str!(if p.is_empty() { "-" } else { p }.as_bytes());
                    }
                }
                b'o' => {
                    if format.is_empty() {
                        zlog!(
                            WARNING,
                            "the name of the header must be set between embraces for %{}",
                            c as char
                        );
                        return Err(());
                    }
                    if let Some((_, req)) = data {
                        let mut written = 0;
                        let fb = format.as_bytes();
                        for (k, v) in req.headers {
                            let mut h = k.clone();
                            h.extend_from_slice(b": ");
                            h.extend_from_slice(v);
                            if h.is_empty() {
                                continue;
                            }
                            if !h.windows(fb.len()).any(|w| w == fb) {
                                continue;
                            }
                            if h.len() <= fb.len() + 2 {
                                continue;
                            }
                            if h[fb.len()] != b':' || h[fb.len() + 1] != b' ' {
                                continue;
                            }
                            let val = &h[fb.len() + 2..];
                            if let Some(st) = stream.as_deref_mut() {
                                written += st.str(if val.is_empty() { b"-" } else { val });
                            }
                            break;
                        }
                        if written == 0 {
                            if let Some(st) = stream.as_deref_mut() {
                                st.char(b'-');
                            }
                        }
                    }
                    format.clear();
                }
                b'p' => {
                    if !test {
                        out_str!(format!("{}", std::process::id()).as_bytes());
                    }
                }
                b'P' => {
                    if !test {
                        out_str!(format!("{}", unsafe { libc::getppid() }).as_bytes());
                    }
                }
                b'q' => {
                    if let Some((proc, _)) = data {
                        out_str!(scoreboard::cstr(&proc.query_string));
                    }
                }
                b'Q' => {
                    if let Some((proc, _)) = data {
                        out_str!(if scoreboard::cstr(&proc.query_string).is_empty() {
                            b""
                        } else {
                            b"?"
                        });
                    }
                }
                b'r' => {
                    if let Some((proc, _)) = data {
                        out_str!(scoreboard::cstr(&proc.request_uri));
                    }
                }
                b'R' => {
                    if !test {
                        let ip = super::fastcgi::get_last_client_ip();
                        out_str!(ip.as_deref().unwrap_or("-").as_bytes());
                    }
                }
                b's' => {
                    if let Some((_, req)) = data {
                        out_str!(format!("{}", req.response_code).as_bytes());
                    }
                }
                b'T' | b't' => {
                    if let Some((proc, _)) = data {
                        let t = if c == b't' {
                            proc.accepted_epoch
                        } else {
                            now_epoch
                        };
                        let f = if format.is_empty() {
                            "%d/%b/%Y:%H:%M:%S %z".to_string()
                        } else {
                            format.clone()
                        };
                        let v = strftime_or(&f, t);
                        let v = &v.as_bytes()[..v.len().min(127)];
                        out_str!(v);
                    }
                    format.clear();
                }
                b'u' => {
                    if let Some((proc, _)) = data {
                        out_str!(scoreboard::cstr(&proc.auth_user));
                    }
                }
                b'{' => {
                    token = true;
                    i += 1;
                    let start = i;
                    while i < s.len() {
                        if s[i] == b'}' {
                            let l = (i - start).min(128);
                            format = String::from_utf8_lossy(&s[start..start + l]).into_owned();
                            break;
                        }
                        i += 1;
                    }
                    if i + 1 >= s.len() {
                        zlog!(WARNING, "missing closing embrace in the access.format");
                        return Err(());
                    }
                }
                _ => {
                    zlog!(
                        WARNING,
                        "Invalid token in the access.format (%{})",
                        c as char
                    );
                    return Err(());
                }
            }
            if s.get(i).copied() != Some(b'}') && !format.is_empty() {
                zlog!(
                    WARNING,
                    "embrace is not allowed for modifier %{}",
                    c as char
                );
                return Err(());
            }
            i += 1;
            if over_limit!() {
                zlog!(
                    NOTICE,
                    "the log buffer is over the configured limit. The access log request has been truncated."
                );
                break;
            }
            continue;
        }
        if over_limit!() {
            zlog!(
                NOTICE,
                "the log buffer is over the configured limit. The access log request has been truncated."
            );
            break;
        }
        if !test {
            if let Some(st) = stream.as_deref_mut() {
                st.char(s[i]);
            }
        }
        i += 1;
    }
    if !test {
        if let Some(st) = stream {
            st.finish();
        }
    }
    Ok(())
}
