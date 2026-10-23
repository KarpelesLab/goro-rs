//! sapi/fpm/fpm/zlog.c: the FPM logger (error log lines, the decorated
//! worker output streams and the access log stream).

use std::cell::Cell;

pub const DEBUG: i32 = 1;
pub const NOTICE: i32 = 2;
pub const WARNING: i32 = 3;
pub const ERROR: i32 = 4;
pub const ALERT: i32 = 5;
pub const LEVEL_MASK: i32 = 7;
pub const ACCESS_LOG: i32 = 16;
pub const HAVE_ERRNO: i32 = 0x100;
pub const SYSERROR: i32 = ERROR | HAVE_ERRNO;
pub const SYSLOG_FD: i32 = -2;

/// ZLOG_DEFAULT_LIMIT
pub const DEFAULT_LIMIT: i32 = 1024;
/// ZLOG_MIN_LIMIT
pub const MIN_LIMIT: i32 = 512;

const MAX_BUF_LENGTH: usize = 2048;
const MAX_WRAPPING_PREFIX_LENGTH: usize = 512;

const LEVEL_NAMES: [&str; 6] = ["", "DEBUG", "NOTICE", "WARNING", "ERROR", "ALERT"];

thread_local! {
    static FD: Cell<i32> = const { Cell::new(-1) };
    static FD_IS_STDERR: Cell<bool> = const { Cell::new(false) };
    static LEVEL: Cell<i32> = const { Cell::new(NOTICE) };
    static LIMIT: Cell<i32> = const { Cell::new(DEFAULT_LIMIT) };
    static BUFFERING: Cell<bool> = const { Cell::new(true) };
    static LAUNCHED: Cell<bool> = const { Cell::new(false) };
    static EXTERNAL: Cell<Option<fn(i32, &[u8])>> = const { Cell::new(None) };
    /// fpm_globals.is_child
    static IS_CHILD: Cell<bool> = const { Cell::new(false) };
}

pub fn set_external_logger(l: Option<fn(i32, &[u8])>) {
    EXTERNAL.with(|c| c.set(l));
}

pub fn set_is_child(v: bool) {
    IS_CHILD.with(|c| c.set(v));
}

pub fn is_child() -> bool {
    IS_CHILD.with(|c| c.get())
}

pub fn level_name(log_level: i32) -> &'static str {
    let l = if log_level < 0 { level() } else { log_level };
    if !(DEBUG..=ALERT).contains(&l) {
        return "unknown value";
    }
    LEVEL_NAMES[l as usize]
}

pub fn set_launched() {
    LAUNCHED.with(|c| c.set(true));
}

fn launched() -> bool {
    LAUNCHED.with(|c| c.get())
}

/// zlog_set_fd(): returns the previous descriptor.
pub fn set_fd(fd: i32, is_stderr: bool) -> i32 {
    FD_IS_STDERR.with(|c| c.set(is_stderr));
    FD.with(|c| c.replace(fd))
}

pub fn fd() -> i32 {
    FD.with(|c| c.get())
}

pub fn set_level(v: i32) -> i32 {
    let old = level();
    if (DEBUG..=ALERT).contains(&v) {
        LEVEL.with(|c| c.set(v));
    }
    old
}

pub fn level() -> i32 {
    LEVEL.with(|c| c.get())
}

pub fn set_limit(v: i32) -> i32 {
    LIMIT.with(|c| c.replace(v))
}

fn limit() -> usize {
    LIMIT.with(|c| c.get()).max(0) as usize
}

pub fn set_buffering(v: bool) -> bool {
    BUFFERING.with(|c| c.replace(v))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// localtime_r()
pub fn localtime(t: i64) -> libc::tm {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    unsafe {
        libc::localtime_r(&tt, &mut tm);
    }
    tm
}

/// strftime() in the C locale.
pub fn strftime(fmt: &str, t: i64) -> String {
    let tm = localtime(t);
    let cfmt = std::ffi::CString::new(fmt).unwrap_or_default();
    let mut buf = vec![0u8; 256];
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len() - 1,
            cfmt.as_ptr(),
            &tm,
        )
    };
    buf.truncate(n);
    String::from_utf8_lossy(&buf).into_owned()
}

/// gettimeofday()
pub fn now_tv() -> (i64, i64) {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_secs() as i64, d.subsec_micros() as i64)
}

/// zlog_print_time(): "[23-Oct-2026 10:11:12] " (with microseconds in debug).
pub fn print_time(tv: (i64, i64)) -> String {
    let tm = localtime(tv.0);
    let mut s = format!(
        "[{:02}-{}-{:04} {:02}:{:02}:{:02}",
        tm.tm_mday,
        MONTHS[tm.tm_mon.clamp(0, 11) as usize],
        tm.tm_year + 1900,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    );
    if level() == DEBUG {
        s.push_str(&format!(".{:06}", tv.1));
    }
    s.push_str("] ");
    s
}

fn quiet_write(fd: i32, b: &[u8]) {
    let mut done = 0;
    while done < b.len() {
        let n = unsafe {
            libc::write(
                fd,
                b[done..].as_ptr() as *const libc::c_void,
                b.len() - done,
            )
        };
        if n <= 0 {
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        done += n as usize;
    }
}

/// zlog_buf_prefix(): the full prefix (callers truncate it to their buffer).
fn buf_prefix(function: &str, line: u32, flags: i32, use_syslog: bool) -> Vec<u8> {
    let lname = LEVEL_NAMES[(flags & LEVEL_MASK).clamp(0, 5) as usize];
    let s = if use_syslog {
        if level() == DEBUG {
            format!("[{}] {}(), line {}: ", lname, function, line)
        } else {
            format!("[{}] ", lname)
        }
    } else {
        let mut s = String::new();
        if !is_child() {
            s.push_str(&print_time(now_tv()));
        }
        if level() == DEBUG {
            if !is_child() {
                s.push_str(&format!(
                    "{}: pid {}, {}(), line {}: ",
                    lname,
                    std::process::id(),
                    function,
                    line
                ));
            } else {
                s.push_str(&format!("{}: {}(), line {}: ", lname, function, line));
            }
        } else {
            s.push_str(&format!("{}: ", lname));
        }
        s
    };
    s.into_bytes()
}

fn syslog_write(msg: &[u8]) {
    let priority = match level() {
        DEBUG => libc::LOG_DEBUG,
        NOTICE => libc::LOG_NOTICE,
        WARNING => libc::LOG_WARNING,
        ERROR => libc::LOG_ERR,
        _ => libc::LOG_ALERT,
    };
    let m: Vec<u8> = msg.iter().copied().filter(|&c| c != 0).collect();
    let c = std::ffi::CString::new(m).unwrap_or_default();
    unsafe {
        libc::syslog(priority, c"%s".as_ptr(), c.as_ptr());
    }
}

/// vzlog(): one log line. `errno` is the saved errno for ZLOG_HAVE_ERRNO.
pub fn vzlog(function: &str, line: u32, flags: i32, msg: &str, errno: i32) {
    let buf_size = MAX_BUF_LENGTH;
    if let Some(ext) = EXTERNAL.with(|c| c.get()) {
        // zlog_external(): vsnprintf() into the 2048 bytes buffer
        let m = msg.as_bytes();
        if m.len() >= buf_size {
            let mut b = m[..buf_size - 3].to_vec();
            b.extend_from_slice(b"...");
            ext(flags & LEVEL_MASK, &b);
        } else {
            ext(flags & LEVEL_MASK, m);
        }
    }
    if (flags & LEVEL_MASK) < level() {
        return;
    }
    let use_syslog = fd() == SYSLOG_FD;
    let mut buf = buf_prefix(function, line, flags, use_syslog);
    let mut truncated = false;
    if buf.len() > buf_size - 1 {
        truncated = true;
    } else {
        buf.extend_from_slice(msg.as_bytes());
        if buf.len() >= buf_size {
            truncated = true;
        }
    }
    if !truncated && flags & HAVE_ERRNO != 0 {
        buf.extend_from_slice(format!(": {} ({})", strerror(errno), errno).as_bytes());
        if buf.len() >= limit() {
            truncated = true;
        }
    }
    if truncated {
        // zlog_truncate_buf(buf, MIN(zlog_limit, buf_size), 1)
        let size = limit().min(buf_size);
        buf.truncate(size.saturating_sub(4));
        buf.extend_from_slice(b"...");
    }
    if use_syslog {
        syslog_write(&buf);
        buf.push(b'\n');
    } else {
        buf.push(b'\n');
        let f = fd();
        quiet_write(if f > -1 { f } else { 2 }, &buf);
    }
    if !FD_IS_STDERR.with(|c| c.get())
        && fd() != -1
        && !launched()
        && (flags & LEVEL_MASK) >= NOTICE
    {
        quiet_write(2, &buf);
    }
}

pub fn strerror(errno: i32) -> String {
    let p = unsafe { libc::strerror(errno) };
    if p.is_null() {
        return format!("Unknown error {}", errno);
    }
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

pub fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// zlog(flags, fmt, ...)
#[macro_export]
macro_rules! zlog {
    ($flags:expr, $($arg:tt)*) => {{
        let __e = $crate::fpm::zlog::errno();
        $crate::fpm::zlog::vzlog(
            $crate::fpm::zlog::fn_name(module_path!()),
            line!(),
            $flags,
            &format!($($arg)*),
            __e,
        )
    }};
}

/// The C function name shown in debug log lines: the module name here.
pub fn fn_name(module: &'static str) -> &'static str {
    match module.rfind(':') {
        Some(p) => &module[p + 1..],
        None => module,
    }
}

/// zlog_msg_ex()
pub fn msg_ex(function: &'static str, line: u32, flags: i32, prefix: &[u8], msg: &[u8]) {
    let mut stream = ZlogStream::init_internal(flags, msg.len() + prefix.len(), 0);
    stream.prefix_ex(function, line);
    stream.str(prefix);
    stream.str(msg);
    stream.finish();
}

/// struct zlog_stream
pub struct ZlogStream {
    flags: i32,
    use_syslog: bool,
    use_fd: bool,
    use_buffer: bool,
    use_stderr: bool,
    prefix_buffer: bool,
    finished: bool,
    full: bool,
    wrap: bool,
    msg_quote: bool,
    decorate: bool,
    pub is_stdout: bool,
    over_limit: bool,
    use_external_logger: bool,
    fd: i32,
    line: u32,
    pub child_pid: i32,
    function: Option<&'static str>,
    /// buf.data (its length is buf.size)
    data: Vec<u8>,
    len: usize,
    buf_init_size: usize,
    prefix_len: usize,
    msg_prefix: Option<Vec<u8>>,
    msg_suffix: Option<Vec<u8>>,
    msg_final_suffix: Option<Vec<u8>>,
}

impl ZlogStream {
    /// zlog_stream_init_internal()
    fn init_internal(flags: i32, capacity: usize, fd: i32) -> ZlogStream {
        let fd = if fd == 0 { self::fd() } else { fd };
        let mut s = ZlogStream {
            flags,
            use_syslog: fd == SYSLOG_FD,
            use_fd: fd > 0,
            use_buffer: false,
            use_stderr: false,
            prefix_buffer: false,
            finished: false,
            full: false,
            wrap: false,
            msg_quote: false,
            decorate: false,
            is_stdout: false,
            over_limit: false,
            use_external_logger: false,
            fd: if fd > -1 { fd } else { 2 },
            line: 0,
            child_pid: 0,
            function: None,
            data: Vec::new(),
            len: 0,
            buf_init_size: capacity,
            prefix_len: 0,
            msg_prefix: None,
            msg_suffix: None,
            msg_final_suffix: None,
        };
        if flags & ACCESS_LOG != 0 {
            s.use_external_logger = false;
            s.use_buffer = true;
            s.use_stderr = fd < 0;
        } else {
            let ext = EXTERNAL.with(|c| c.get()).is_some();
            s.use_external_logger = ext;
            s.use_buffer = BUFFERING.with(|c| c.get()) || ext || s.use_syslog;
            s.use_stderr =
                fd < 0 || (fd != 2 && fd != 1 && !launched() && (flags & LEVEL_MASK) >= NOTICE);
            s.prefix_buffer =
                (flags & LEVEL_MASK) >= level() && (s.use_fd || s.use_stderr || s.use_syslog);
        }
        s
    }

    /// zlog_stream_init_ex(): a wrapping stream to `fd`.
    pub fn init_ex(flags: i32, fd: i32) -> ZlogStream {
        let mut s = Self::init_internal(flags, 1024, fd);
        s.wrap = true;
        s
    }

    pub fn set_decorating(&mut self, decorate: bool) {
        if decorate {
            self.decorate = true;
        } else {
            self.decorate = false;
            self.msg_quote = false;
            self.prefix_buffer = false;
        }
    }

    pub fn set_wrapping(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    pub fn set_msg_quoting(&mut self, quote: bool) {
        self.msg_quote = quote && self.decorate;
    }

    pub fn set_msg_prefix(&mut self, prefix: &str) {
        if !self.decorate {
            return;
        }
        let b = prefix.as_bytes();
        let n = b.len().min(MAX_WRAPPING_PREFIX_LENGTH - 2);
        self.msg_prefix = Some(b[..n].to_vec());
    }

    pub fn set_msg_suffix(&mut self, suffix: Option<&str>, final_suffix: Option<&str>) {
        if !self.wrap || !self.decorate {
            return;
        }
        if let Some(s) = suffix {
            self.msg_suffix = Some(s.as_bytes().to_vec());
        }
        if let Some(f) = final_suffix {
            self.msg_final_suffix = Some(f.as_bytes().to_vec());
        }
    }

    fn msg_suffix_len(&self) -> usize {
        self.msg_suffix.as_ref().map_or(0, |s| s.len())
    }

    /// zlog_stream_buf_alloc_ex()
    fn buf_alloc_ex(&mut self, needed: usize) -> bool {
        let size = if self.data.is_empty() {
            self.buf_init_size
        } else {
            self.data.len()
        };
        let grown = if self.data.is_empty() {
            size
        } else {
            size << 1
        };
        let size = limit().min(grown.max(needed));
        self.data.resize(size, 0);
        true
    }

    /// zlog_stream_buf_alloc()
    fn buf_alloc(&mut self) -> bool {
        if !self.data.is_empty() && self.data.len() >= limit().min(self.buf_init_size) {
            return true;
        }
        self.buf_alloc_ex(0)
    }

    fn direct_write_ex(&self, buf: &[u8], append: &[u8]) -> usize {
        if self.use_fd {
            quiet_write(self.fd, buf);
            if !append.is_empty() {
                quiet_write(self.fd, append);
            }
        }
        if self.use_stderr {
            quiet_write(2, buf);
            if !append.is_empty() {
                quiet_write(2, append);
            }
        }
        buf.len()
    }

    fn direct_write(&self, buf: &[u8]) -> usize {
        self.direct_write_ex(buf, &[])
    }

    /// zlog_stream_unbuffered_write()
    fn unbuffered_write(&mut self, buf: &[u8]) -> isize {
        let mut len = buf.len();
        let mut append: &[u8] = &[];
        if self.len == 0 {
            let f = self.function.unwrap_or("zlog_stream_unbuffered_write");
            self.len = self.prefix_ex(f, self.line).max(0) as usize;
        }
        let lim = limit();
        let reserved_len = self.len + self.msg_suffix_len() + self.msg_quote as usize;
        let required_len = reserved_len + len;
        if required_len >= lim {
            if self.wrap {
                let append: &[u8] = if required_len == lim { &[] } else { b"\n" };
                let available_len = lim.saturating_sub(reserved_len + 1).min(len);
                self.direct_write(&buf[..available_len]);
                if !append.is_empty() {
                    if self.msg_quote {
                        self.direct_write(b"\"");
                    }
                    if let Some(s) = self.msg_suffix.clone() {
                        self.direct_write(&s);
                    }
                    self.direct_write(append);
                }
                self.len = 0;
                let written = self.unbuffered_write(&buf[available_len..]);
                if written > 0 {
                    return available_len as isize + written;
                }
                return written;
            }
            self.full = true;
            if required_len != lim {
                append = b"...";
                len = lim.saturating_sub(self.len + 3).min(len);
            }
        }
        let written = self.direct_write_ex(&buf[..len], append);
        self.len += written;
        written as isize
    }

    /// zlog_stream_start()
    pub fn start(&mut self) {
        self.finished = false;
        self.len = 0;
        self.full = false;
        self.over_limit = false;
    }

    /// zlog_stream_buf_copy_cstr()
    fn buf_copy_cstr(&mut self, s: &[u8]) -> isize {
        let mut str_len = s.len();
        if self.data.len().saturating_sub(self.len) <= str_len
            && !self.buf_alloc_ex(str_len + self.len)
        {
            return -1;
        }
        if self.data.len().saturating_sub(self.len) <= str_len {
            self.over_limit = true;
            str_len = self.data.len().saturating_sub(self.len);
        }
        self.data[self.len..self.len + str_len].copy_from_slice(&s[..str_len]);
        self.len += str_len;
        str_len as isize
    }

    /// zlog_stream_buf_copy_char()
    fn buf_copy_char(&mut self, c: u8) -> isize {
        if self.data.len() == self.len && !self.buf_alloc_ex(1) {
            return -1;
        }
        if self.data.len() == self.len {
            self.over_limit = true;
            return 0;
        }
        self.data[self.len] = c;
        self.len += 1;
        1
    }

    /// zlog_stream_buf_flush()
    fn buf_flush(&mut self) -> usize {
        if self.use_syslog {
            let d = self.data[..self.len].to_vec();
            syslog_write(&d);
        }
        if self.use_external_logger {
            if let Some(ext) = EXTERNAL.with(|c| c.get()) {
                let start = self.prefix_len.min(self.len);
                let d = self.data[start..self.len].to_vec();
                ext(self.flags & LEVEL_MASK, &d);
            }
        }
        self.buf_copy_char(b'\n');
        let d = self.data[..self.len].to_vec();
        let written = self.direct_write(&d);
        self.len = 0;
        written
    }

    /// zlog_stream_buf_append()
    fn buf_append(&mut self, s: &[u8]) -> isize {
        if self.len == 0 {
            let f = self.function.unwrap_or("zlog_stream_buf_append");
            self.len = self.prefix_ex(f, self.line).max(0) as usize;
        }
        let lim = limit();
        let reserved_len = self.len + self.msg_suffix_len() + self.msg_quote as usize;
        let required_len = reserved_len + s.len();
        let (over_limit, available_len) = if required_len >= lim {
            self.over_limit = true;
            (true, lim.saturating_sub(reserved_len + 1).min(s.len()))
        } else {
            (false, s.len())
        };
        if self.buf_copy_cstr(&s[..available_len]) < 0 {
            return -1;
        }
        if !over_limit {
            return available_len as isize;
        }
        if self.wrap {
            if self.msg_quote {
                self.buf_copy_char(b'"');
            }
            if let Some(suf) = self.msg_suffix.clone() {
                self.buf_copy_cstr(&suf);
            }
            self.buf_flush();
            let f = self.function.unwrap_or("zlog_stream_buf_append");
            self.prefix_ex(f, self.line);
            return available_len as isize + self.buf_append(&s[available_len..]);
        }
        // zlog_truncate_buf(buf, len, 0)
        if self.len >= 3 {
            let l = self.len;
            self.data[l - 3..l].copy_from_slice(b"...");
        }
        self.full = true;
        available_len as isize
    }

    /// zlog_stream_prefix_ex()
    pub fn prefix_ex(&mut self, function: &'static str, line: u32) -> isize {
        if !self.prefix_buffer {
            return 0;
        }
        if self.wrap && self.function.is_none() {
            self.function = Some(function);
            self.line = line;
        }
        let prefix = buf_prefix(function, line, self.flags, self.use_syslog);
        if self.use_buffer {
            if !self.buf_alloc() {
                return -1;
            }
            let mut len = prefix.len();
            if len + 1 > self.data.len() {
                len = (len + 1).min(limit());
                if !self.buf_alloc_ex(len) {
                    return -1;
                }
            }
            let n = prefix.len().min(self.data.len().saturating_sub(1));
            self.data[..n].copy_from_slice(&prefix[..n]);
            self.len = len.min(self.data.len());
            self.prefix_len = self.len;
            if let Some(p) = self.msg_prefix.clone() {
                self.buf_copy_cstr(&p);
            }
            if self.msg_quote {
                self.buf_copy_char(b'"');
            }
            self.len as isize
        } else {
            let n = prefix.len().min(1023);
            let mut written = self.direct_write(&prefix[..n]);
            if let Some(p) = self.msg_prefix.clone() {
                written += self.direct_write(&p);
            }
            if self.msg_quote {
                written += self.direct_write(b"\"");
            }
            written as isize
        }
    }

    /// zlog_stream_str()
    pub fn str(&mut self, s: &[u8]) -> isize {
        if s.is_empty() || self.full {
            return 0;
        }
        if self.finished {
            self.start();
        }
        if self.use_buffer {
            return self.buf_append(s);
        }
        self.unbuffered_write(s)
    }

    /// zlog_stream_char()
    pub fn char(&mut self, c: u8) -> isize {
        if self.full {
            return 0;
        }
        if self.finished {
            self.start();
        }
        if self.use_buffer {
            return self.buf_copy_char(c);
        }
        self.direct_write(&[c]) as isize
    }

    /// zlog_stream_finish_buffer_suffix()
    fn finish_buffer_suffix(&mut self) {
        if self.msg_quote {
            self.buf_copy_char(b'"');
        }
        if let Some(s) = self.msg_suffix.clone() {
            self.buf_copy_cstr(&s);
        }
        if let Some(fs) = self.msg_final_suffix.clone() {
            let lim = limit();
            if self.len + fs.len() >= lim {
                let quoting = self.msg_quote;
                let wrap = (self.len + fs.len() + 1 - lim).min(fs.len());
                self.buf_copy_cstr(&fs[..fs.len() - wrap]);
                self.buf_copy_char(b'\n');
                self.buf_flush();
                self.msg_quote = false;
                let f = self.function.unwrap_or("zlog_stream_finish");
                self.prefix_ex(f, self.line);
                self.msg_quote = quoting;
                self.buf_copy_cstr(&fs[fs.len() - wrap..]);
                self.buf_copy_char(b'\n');
            } else {
                self.buf_copy_cstr(&fs);
            }
        }
    }

    /// zlog_stream_finish_direct_suffix()
    fn finish_direct_suffix(&mut self) {
        if self.msg_quote {
            self.direct_write(b"\"");
            self.len += 1;
        }
        if let Some(s) = self.msg_suffix.clone() {
            self.direct_write(&s);
            self.len += s.len();
        }
        if let Some(fs) = self.msg_final_suffix.clone() {
            let lim = limit();
            if self.len + fs.len() >= lim {
                let quoting = self.msg_quote;
                let wrap = (self.len + fs.len() + 1 - lim).min(fs.len());
                self.direct_write_ex(&fs[..fs.len() - wrap], b"\n");
                self.msg_quote = false;
                let f = self.function.unwrap_or("zlog_stream_finish");
                self.prefix_ex(f, self.line);
                self.msg_quote = quoting;
                self.direct_write_ex(&fs[fs.len() - wrap..], b"\n");
            } else {
                self.direct_write_ex(&fs, b"\n");
            }
        } else {
            self.direct_write(b"\n");
        }
    }

    /// zlog_stream_finish()
    pub fn finish(&mut self) -> bool {
        if self.finished || self.len == 0 {
            return true;
        }
        if self.use_buffer {
            if self.decorate {
                self.finish_buffer_suffix();
            }
            self.buf_flush();
        } else if self.decorate {
            self.finish_direct_suffix();
        } else {
            self.direct_write(b"\n");
        }
        self.finished = true;
        true
    }

    pub fn is_over_limit(&self) -> bool {
        self.over_limit
    }
}
