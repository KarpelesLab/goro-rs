//! The ftp:// and ftps:// stream wrappers (PHP manual: "FTP and FTPS"):
//! reading and writing files over a passive data connection, directory
//! listings, stat, mkdir, unlink, rename and rmdir. Messages follow
//! ext/standard/tests/streams (opendir-00x, bug72771, bug73457, bug77680,
//! bug77765) and PHP 8.5's observed output.

use std::time::Duration;

use goro_runtime::{Key, PhpArray, Value};

use super::conn::{Conn, Open, Ty};
use crate::builtins::file::{Backing, Stream};
use crate::builtins::sockets::Sock;
use crate::builtins::streams::{NativeStream, NativeWrapper, register_wrapper};
use crate::func::Args;
use crate::vm::{R, Vm};

static WRAPPER: NativeWrapper = NativeWrapper {
    open: wrapper_open,
    url_stat: Some(url_stat),
    unlink: Some(unlink),
    rename: Some(rename),
    mkdir: Some(mkdir),
    rmdir: Some(rmdir),
    opendir: Some(opendir),
    metadata: None,
    include: None,
};

pub fn init(vm: &mut Vm) {
    register_wrapper(vm, "ftp", &WRAPPER);
    register_wrapper(vm, "ftps", &WRAPPER);
}

/// A parsed ftp:// URL.
struct Url {
    ssl: bool,
    host: Vec<u8>,
    port: u16,
    user: Vec<u8>,
    pass: Vec<u8>,
    path: Vec<u8>,
}

fn pct_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%'
            && i + 2 < s.len()
            && let (Some(h), Some(l)) = (
                (s[i + 1] as char).to_digit(16),
                (s[i + 2] as char).to_digit(16),
            )
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn parse_url(url: &[u8]) -> Option<Url> {
    let p = url.windows(3).position(|w| w == b"://")?;
    let ssl = url[..p].eq_ignore_ascii_case(b"ftps");
    let rest = &url[p + 3..];
    let slash = rest.iter().position(|&b| b == b'/').unwrap_or(rest.len());
    let (auth, path) = (&rest[..slash], &rest[slash..]);
    let (cred, hostport) = match auth.iter().rposition(|&b| b == b'@') {
        Some(at) => (Some(&auth[..at]), &auth[at + 1..]),
        None => (None, auth),
    };
    let (host, port) = if hostport.first() == Some(&b'[') {
        let e = hostport.iter().position(|&b| b == b']')?;
        let h = &hostport[1..e];
        let port = match hostport.get(e + 1) {
            Some(b':') => std::str::from_utf8(&hostport[e + 2..]).ok()?.parse().ok()?,
            _ => 21,
        };
        (h.to_vec(), port)
    } else {
        match hostport.iter().rposition(|&b| b == b':') {
            Some(c) => (
                hostport[..c].to_vec(),
                std::str::from_utf8(&hostport[c + 1..]).ok()?.parse().ok()?,
            ),
            None => (hostport.to_vec(), 21),
        }
    };
    if host.is_empty() {
        return None;
    }
    let (user, pass) = match cred {
        Some(c) => match c.iter().position(|&b| b == b':') {
            Some(i) => (pct_decode(&c[..i]), pct_decode(&c[i + 1..])),
            None => (pct_decode(c), Vec::new()),
        },
        None => (b"anonymous".to_vec(), b"anonymous".to_vec()),
    };
    let mut path = pct_decode(path);
    if path.is_empty() {
        path.push(b'/');
    }
    Some(Url {
        ssl,
        host,
        port,
        user,
        pass,
        path,
    })
}

fn default_timeout(vm: &Vm) -> Duration {
    let t = vm
        .get_ini(b"default_socket_timeout")
        .map(|v| goro_runtime::convert::str_to_float(&v))
        .unwrap_or(60.0);
    if t <= 0.0 || !t.is_finite() {
        Duration::from_secs(60)
    } else {
        Duration::from_secs_f64(t.min(1e9))
    }
}

/// Connects and logs in; the failure message otherwise.
fn connect(vm: &mut Vm, a: &Args, u: &Url) -> R<Result<Box<Conn>, String>> {
    vm.out.flush();
    let t = default_timeout(vm);
    let mut c = match Conn::open(vm, &u.host, u.port, t) {
        Open::Ok(c) => c,
        Open::Lookup(m) => return Ok(Err(m)),
        Open::Failed => return Ok(Err("Connection refused".into())),
    };
    if u.ssl {
        let ok = c.cmd(vm, "AUTH", Some(b"TLS")) && (c.code == 234 || c.code == 334);
        let ok = ok || (c.cmd(vm, "AUTH", Some(b"SSL")) && (c.code == 234 || c.code == 334));
        if !ok {
            // No silent downgrade to plain FTP (test bug72771.phpt).
            return Ok(Err("Server doesn't support FTPS.".into()));
        }
        c.ctl.ctx = Some(super::conn::no_verify_context());
        let r = crate::builtins::openssl::tls::enable(vm, a, &mut c.ctl, None, false)?;
        if r != crate::builtins::openssl::tls::Enabled::Yes {
            return Ok(Err("Unable to activate SSL mode".into()));
        }
        if c.cmd(vm, "PBSZ", Some(b"0")) && c.cmd(vm, "PROT", Some(b"P")) {
            c.ssl_data = c.code == 200;
        }
    }
    if !c.cmd(vm, "USER", Some(&u.user)) {
        return Ok(Err(reply_text(&c)));
    }
    if c.code == 331 && !(c.cmd(vm, "PASS", Some(&u.pass)) && c.code == 230) {
        return Ok(Err(reply_text(&c)));
    }
    if c.code != 230 && c.code != 331 {
        return Ok(Err(reply_text(&c)));
    }
    c.pasv = true;
    Ok(Ok(c))
}

/// "FTP server reports <reply line>" (the line keeps its end; test
/// opendir-001.phpt).
fn reports(c: &Conn) -> String {
    let mut s = String::from("FTP server reports ");
    if let Some(l) = c.lines.last() {
        s.push_str(&String::from_utf8_lossy(l));
        s.push('\n');
    } else {
        s.push_str(&String::from_utf8_lossy(&c.msg));
    }
    s
}

fn reply_text(c: &Conn) -> String {
    String::from_utf8_lossy(&c.msg).into_owned()
}

/// A file transfer as a stream: the data connection, and the control
/// connection that gets the final reply and QUIT at close.
struct FtpStream {
    data: Option<Sock>,
    ctl: Box<Conn>,
    eof: bool,
}

impl NativeStream for FtpStream {
    fn read(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(d) = &mut self.data else {
            return Ok(0);
        };
        let n = d.read(vm, buf)?;
        if n == 0 && !buf.is_empty() {
            self.eof = true;
        }
        Ok(n)
    }

    fn write(&mut self, vm: &mut Vm, data: &[u8]) -> std::io::Result<usize> {
        match &mut self.data {
            Some(d) => d.write(vm, data),
            None => Err(std::io::Error::other("closed")),
        }
    }

    fn eof(&self) -> bool {
        self.eof
    }

    fn close(&mut self, vm: &mut Vm) -> R<()> {
        if let Some(d) = self.data.take() {
            Conn::close_data(vm, d);
            self.ctl.getresp(vm);
        }
        self.ctl.quit(vm);
        Ok(())
    }

    fn is_socket(&self) -> bool {
        true
    }

    fn raw_fd(&self) -> Option<i32> {
        use std::os::fd::AsRawFd;
        self.data.as_ref().map(|d| d.fd.as_raw_fd())
    }
}

fn wrapper_open(vm: &mut Vm, a: &Args, url: &[u8], mode: &[u8], quiet: bool) -> R<Option<Stream>> {
    let fname = a.fname();
    let fail = |vm: &mut Vm, msg: &str| -> R<Option<Stream>> {
        if !quiet {
            vm.warn(&format!(
                "{}({}): Failed to open stream: {}",
                fname,
                String::from_utf8_lossy(url),
                msg
            ))?;
        }
        Ok(None)
    };
    if !vm.ini_bool(b"allow_url_fopen", true) {
        if !quiet {
            vm.warn(&format!(
                "{fname}(): ftp:// wrapper is disabled in the server configuration by allow_url_fopen=0"
            ))?;
        }
        return fail(vm, "no suitable wrapper could be found");
    }
    let Some(u) = parse_url(url) else {
        return fail(vm, "invalid URL");
    };
    let write = mode.iter().any(|c| matches!(c, b'w' | b'a' | b'x' | b'c'));
    if mode.contains(&b'+') {
        return fail(
            vm,
            "FTP does not support simultaneous read/write connections",
        );
    }
    let mut c = match connect(vm, a, &u)? {
        Ok(c) => c,
        Err(m) => return fail(vm, &m),
    };
    if !c.set_type(vm, Ty::Image) {
        let m = reply_text(&c);
        return fail(vm, &m);
    }
    let cmd = if write {
        if mode.first() == Some(&b'a') {
            "APPE"
        } else {
            if mode.first() == Some(&b'x') && c.cmd(vm, "SIZE", Some(&u.path)) && c.code == 213 {
                return fail(
                    vm,
                    "Remote file already exists and overwrite context option not specified",
                );
            }
            "STOR"
        }
    } else {
        // The size first (an existence check; test bug73457.phpt).
        if !c.cmd(vm, "SIZE", Some(&u.path)) {
            let m = reply_text(&c);
            return fail(vm, &m);
        }
        "RETR"
    };
    let data = match data_conn(vm, a, &mut c, cmd, &u.path)? {
        Ok(d) => d,
        Err(m) => return fail(vm, &m),
    };
    let st = FtpStream {
        data: Some(data),
        ctl: c,
        eof: false,
    };
    Ok(Some(Stream::new(
        Backing::Native(Box::new(st)),
        mode,
        url,
        "ftp",
        "tcp_socket/ssl",
    )))
}

/// A passive data connection for `cmd path`; the failure message
/// otherwise ("Failed to set up data channel: ...").
fn data_conn(
    vm: &mut Vm,
    a: &Args,
    c: &mut Conn,
    cmd: &str,
    path: &[u8],
) -> R<Result<Sock, String>> {
    match c.data_cmd(vm, a, cmd, Some(path))? {
        Some(d) => Ok(Ok(d)),
        None => {
            if c.code == 227 || c.code == 0 {
                let why = if c.code == 0 {
                    reply_text(c)
                } else {
                    "Connection refused".into()
                };
                Ok(Err(format!("Failed to set up data channel: {why}")))
            } else {
                Ok(Err(reports(c)))
            }
        }
    }
}

fn opendir(vm: &mut Vm, a: &Args, url: &[u8]) -> R<Option<Vec<Vec<u8>>>> {
    let fail = |vm: &mut Vm, m: &str| -> R<Option<Vec<Vec<u8>>>> {
        vm.warn(&format!(
            "{}({}): Failed to open directory: {}",
            a.fname(),
            String::from_utf8_lossy(url),
            m
        ))?;
        Ok(None)
    };
    let Some(u) = parse_url(url) else {
        return fail(vm, "invalid URL");
    };
    let mut c = match connect(vm, a, &u)? {
        Ok(c) => c,
        Err(m) => return fail(vm, &m),
    };
    let mut data = match data_conn(vm, a, &mut c, "NLST", &u.path)? {
        Ok(d) => d,
        Err(m) => {
            c.quit(vm);
            return fail(vm, &m);
        }
    };
    let body = Conn::read_all(vm, &mut data);
    Conn::close_data(vm, data);
    c.getresp(vm);
    c.quit(vm);
    // Each entry loses its last two bytes, the CRLF a listing line ends
    // with (observed on PHP 8.5: "file\n" lists as "fil").
    let mut out = Vec::new();
    for l in body.split_inclusive(|&b| b == b'\n') {
        if !l.ends_with(b"\n") {
            out.push(l.to_vec());
            continue;
        }
        out.push(l[..l.len().saturating_sub(2)].to_vec());
    }
    Ok(Some(out))
}

fn stat_arr(dir: bool, size: i64, mtime: i64) -> PhpArray {
    let mut a = PhpArray::new();
    let mode: i64 = if dir { 0o40755 } else { 0o100644 };
    for (k, v) in [
        ("dev", 0i64),
        ("ino", 0),
        ("mode", mode),
        ("nlink", 1),
        ("uid", 0),
        ("gid", 0),
        ("rdev", -1),
        ("size", size),
        ("atime", mtime),
        ("mtime", mtime),
        ("ctime", mtime),
        ("blksize", -1),
        ("blocks", -1),
    ] {
        a.insert(Key::from(k), Value::Int(v));
    }
    a
}

/// A throwaway argument list for the operations without one.
fn quiet_args<T>(vm: &mut Vm, f: impl FnOnce(&mut Vm, &Args) -> R<T>) -> R<T> {
    let func = std::rc::Rc::new(crate::func::Func::native(
        "stat",
        "function stat(string $filename): array|false",
        |_, _| Ok(Value::Null),
        0,
    ));
    let mut vals = Vec::new();
    let a = Args {
        this: None,
        vals: &mut vals,
        func: &func,
        static_class: std::ptr::null(),
        named: None,
    };
    f(vm, &a)
}

fn url_stat(vm: &mut Vm, url: &[u8], _flags: i64) -> R<Option<PhpArray>> {
    let Some(u) = parse_url(url) else {
        return Ok(None);
    };
    quiet_args(vm, |vm, a| {
        let mut c = match connect(vm, a, &u)? {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };
        // A directory when CWD into it works (test bug77765.phpt).
        let dir = c.cmd(vm, "CWD", Some(&u.path)) && c.code == 250;
        let mut size = 0;
        let mut found = dir;
        if c.set_type(vm, Ty::Image) && c.cmd(vm, "SIZE", Some(&u.path)) && c.code == 213 {
            size = String::from_utf8_lossy(&c.msg).trim().parse().unwrap_or(0);
            found = true;
        }
        if !found {
            c.quit(vm);
            return Ok(None);
        }
        let mut mtime = 0;
        if c.cmd(vm, "MDTM", Some(&u.path)) && c.code == 213 {
            let s = String::from_utf8_lossy(&c.msg).trim().to_string();
            if s.len() >= 14 && s.as_bytes()[..14].iter().all(u8::is_ascii_digit) {
                let n = |r: std::ops::Range<usize>| s[r].parse::<i64>().unwrap_or(0);
                mtime = super::days_from_civil(n(0..4), n(4..6), n(6..8)) * 86400
                    + n(8..10) * 3600
                    + n(10..12) * 60
                    + n(12..14);
            }
        }
        c.quit(vm);
        Ok(Some(stat_arr(dir, size, mtime)))
    })
}

/// Runs one command on a fresh connection: true on one of `ok`, else a
/// warning with the reply.
fn one_cmd(vm: &mut Vm, a: &Args, url: &[u8], cmd: &str, ok: &[i32]) -> R<bool> {
    let Some(u) = parse_url(url) else {
        return Ok(false);
    };
    let mut c = match connect(vm, a, &u)? {
        Ok(c) => c,
        Err(m) => {
            vm.fwarn(a, &m)?;
            return Ok(false);
        }
    };
    let good = c.cmd(vm, cmd, Some(&u.path)) && ok.contains(&c.code);
    if !good {
        let m = reports(&c);
        vm.fwarn(a, m.trim_end())?;
    }
    c.quit(vm);
    Ok(good)
}

fn unlink(vm: &mut Vm, a: &Args, url: &[u8]) -> R<bool> {
    one_cmd(vm, a, url, "DELE", &[250])
}

fn rmdir(vm: &mut Vm, a: &Args, url: &[u8]) -> R<bool> {
    one_cmd(vm, a, url, "RMD", &[250])
}

fn rename(vm: &mut Vm, a: &Args, from: &[u8], to: &[u8]) -> R<bool> {
    let (Some(f), Some(t)) = (parse_url(from), parse_url(to)) else {
        return Ok(false);
    };
    let mut c = match connect(vm, a, &f)? {
        Ok(c) => c,
        Err(m) => {
            vm.fwarn(a, &m)?;
            return Ok(false);
        }
    };
    let good = c.cmd(vm, "RNFR", Some(&f.path))
        && c.code == 350
        && c.cmd(vm, "RNTO", Some(&t.path))
        && c.code == 250;
    if !good {
        let m = reports(&c);
        vm.fwarn(a, m.trim_end())?;
    }
    c.quit(vm);
    Ok(good)
}

fn mkdir(vm: &mut Vm, a: &Args, url: &[u8], _mode: i64, options: i64) -> R<bool> {
    let Some(u) = parse_url(url) else {
        return Ok(false);
    };
    let mut c = match connect(vm, a, &u)? {
        Ok(c) => c,
        Err(m) => {
            vm.fwarn(a, &m)?;
            return Ok(false);
        }
    };
    let mut path = u.path.clone();
    while path.len() > 1 && path.ends_with(b"/") {
        path.pop();
    }
    let ok = if options & 1 == 0 {
        c.cmd(vm, "MKD", Some(&path)) && c.code == 257
    } else {
        // Recursive: the deepest existing ancestor (CWD), then MKD each
        // level below it (test bug77680.phpt).
        let mut chain = vec![path.clone()];
        let mut p = path.clone();
        while p.len() > 1 {
            match p.iter().rposition(|&b| b == b'/') {
                Some(0) | None => p = b"/".to_vec(),
                Some(i) => p.truncate(i),
            }
            chain.push(p.clone());
        }
        let mut exists = chain.len();
        for (i, d) in chain.iter().enumerate() {
            if c.cmd(vm, "CWD", Some(d)) && c.code == 250 {
                exists = i;
                break;
            }
        }
        let mut ok = true;
        for d in chain[..exists].iter().rev() {
            if !(c.cmd(vm, "MKD", Some(d)) && c.code == 257) {
                ok = false;
                break;
            }
        }
        ok
    };
    if !ok {
        let m = reports(&c);
        vm.fwarn(a, m.trim_end())?;
    }
    c.quit(vm);
    Ok(ok)
}
