//! One FastCGI request run by the engine, in a process forked from its
//! worker for it: the script found from the FastCGI parameters, the
//! superglobals, the response as FCGI_STDOUT records (CGI header lines,
//! an empty line, the body; RFC 3875 §6) and PHP's log messages as
//! FCGI_STDERR records. The functions only FPM has live here too
//! (PHP manual: fastcgi_finish_request(), fpm_get_status(),
//! getallheaders(), apache_request_headers()).

use std::cell::RefCell;
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::fcgi;
use super::scoreboard::{self, Board};
use crate::registry::Module;
use crate::vm::sapi::{Conn, Transport};
use crate::{Args, R, Vm};

/// The request being answered by this process.
pub struct Active {
    pub fd: RawFd,
    pub id: u16,
    pub keep_conn: bool,
    /// FCGI_END_REQUEST was sent.
    pub finished: bool,
    /// FCGI_STDERR records were sent.
    pub stderr_sent: usize,
    /// Header lines not sent yet: they go out in one record with the
    /// first body bytes (test sapi/fpm/tests/log-suppress-output-request-body.phpt:
    /// the client in fcgi.inc takes a one-byte record holding "0" for an
    /// empty one, so a short body cannot have a record of its own).
    pub pending_head: Vec<u8>,
    pub log_limit: usize,
    /// Worker log lines are wanted at NOTICE level.
    pub log_notices: bool,
    pub board: *const Board,
    pub slot: usize,
    pub pool: String,
    pub pm: &'static str,
    pub listen_backlog: i64,
    pub listen_fd: RawFd,
}

thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

pub fn set_active(a: Active) {
    ACTIVE.with(|c| *c.borrow_mut() = Some(a));
}

fn with_active<T>(f: impl FnOnce(&mut Active) -> T) -> Option<T> {
    ACTIVE.with(|c| c.borrow_mut().as_mut().map(f))
}

fn board<'a>(a: &Active) -> &'a Board {
    // SAFETY: the board's mapping outlives the request process.
    unsafe { &*a.board }
}

fn send_stdout(b: &[u8]) {
    with_active(|a| {
        if a.finished || (b.is_empty() && a.pending_head.is_empty()) {
            return;
        }
        let mut data = std::mem::take(&mut a.pending_head);
        data.extend_from_slice(b);
        let mut out = Vec::with_capacity(data.len() + 16);
        fcgi::stream_records(&mut out, fcgi::STDOUT, a.id, &data);
        let _ = fcgi::write_all(a.fd, &out);
    });
}

/// flush(): what is held back goes to the web server now.
pub fn flush_stdout() {
    send_stdout(b"");
}

fn send_stderr(b: &[u8]) {
    with_active(|a| {
        if a.finished {
            return;
        }
        let mut data = Vec::new();
        if a.stderr_sent > 0 {
            // Messages are separated by "; " (test
            // sapi/fpm/tests/bug77106-fcgi-missing-nl.phpt).
            data.extend_from_slice(b"; ");
        }
        data.extend_from_slice(b);
        a.stderr_sent += 1;
        let mut out = Vec::new();
        fcgi::stream_records(&mut out, fcgi::STDERR, a.id, &data);
        let _ = fcgi::write_all(a.fd, &out);
    });
}

/// Ends the response: the closing stream records and FCGI_END_REQUEST.
pub fn finish_response(app_status: u32) -> bool {
    with_active(|a| {
        if a.finished {
            return false;
        }
        a.finished = true;
        let mut out = Vec::new();
        fcgi::stream_records(
            &mut out,
            fcgi::STDOUT,
            a.id,
            &std::mem::take(&mut a.pending_head),
        );
        fcgi::end_stream(&mut out, fcgi::STDOUT, a.id);
        if a.stderr_sent > 0 {
            fcgi::end_stream(&mut out, fcgi::STDERR, a.id);
        }
        fcgi::end_request(&mut out, a.id, app_status, fcgi::REQUEST_COMPLETE);
        let _ = fcgi::write_all(a.fd, &out);
        let s = board(a).slot(a.slot);
        s.done.store(1, std::sync::atomic::Ordering::Relaxed);
        if !a.keep_conn {
            // SAFETY: shutting down our connection socket.
            unsafe { libc::shutdown(a.fd, libc::SHUT_RDWR) };
        }
        true
    })
    .unwrap_or(false)
}

/// A PHP log message (log_errors, error_log()) without an error_log
/// file: to the web server as "PHP message: ..." on FCGI_STDERR, and to
/// FPM's log as "NOTICE: PHP message: ..." on the worker's stderr, cut
/// to log_limit (tests sapi/fpm/tests/log-bm-*.phpt).
pub fn sapi_log(msg: &[u8]) {
    let Some((limit, notices)) = with_active(|a| (a.log_limit, a.log_notices)) else {
        let mut v = msg.to_vec();
        v.push(b'\n');
        super::log::write_fd(2, &v);
        return;
    };
    const HEAD: &[u8] = b"PHP message: ";
    const LEVEL: &[u8] = b"NOTICE: ";
    let max = limit.saturating_sub(1 + LEVEL.len() + HEAD.len()).max(16);
    let mut m = HEAD.to_vec();
    if msg.len() > max {
        m.extend_from_slice(&msg[..max - 3]);
        m.extend_from_slice(b"...");
    } else {
        m.extend_from_slice(msg);
    }
    send_stderr(&m);
    if notices {
        let mut line = LEVEL.to_vec();
        // log_level = debug names the origin (the optional part of the
        // lines sapi/fpm/tests/logtool.inc reads; test gh16628.phpt).
        if super::log::level() == super::log::Level::Debug {
            line.extend_from_slice(
                format!("pid {}, sapi_log(), line {}: ", std::process::id(), line!()).as_bytes(),
            );
        }
        line.extend_from_slice(&m);
        line.push(b'\n');
        super::log::write_fd(2, &line);
    }
}

/// The response transport: CGI-style head and body on FCGI_STDOUT.
struct FcgiTransport;

impl Transport for FcgiTransport {
    fn write_head(&mut self, _status_line: &[u8], headers: &[Vec<u8>]) {
        let mut out = Vec::new();
        for h in headers {
            out.extend_from_slice(h);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        with_active(|a| a.pending_head.extend_from_slice(&out));
    }

    fn write_body(&mut self, b: &[u8]) {
        send_stdout(b);
    }
}

/// Functions of the FPM SAPI.
pub static FPM_MODULE: Module = Module {
    name: "fpm",
    functions: &[
        (
            "function fastcgi_finish_request(): bool",
            fastcgi_finish_request,
        ),
        ("function fpm_get_status(): array|false", fpm_get_status),
        ("function getallheaders(): array", getallheaders),
        ("function apache_request_headers(): array", getallheaders),
    ],
    listed: false,
    ..Module::EMPTY
};

fn fastcgi_finish_request(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if with_active(|a| a.finished).unwrap_or(true) {
        return Ok(Value::Bool(false));
    }
    // PHP manual: "This function flushes all response data to the
    // client and finishes the request."
    vm.ob_end_all();
    crate::builtins::headers::commit(vm);
    vm.out.flush();
    vm.out.sapi.send_head();
    Ok(Value::Bool(finish_response(0)))
}

fn getallheaders(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut a = PhpArray::new();
    for (n, v) in &vm.out.sapi.request_headers {
        a.insert(Key::from_bytes(n), Value::Str(PhpStr::from(v.clone())));
    }
    Ok(Value::Array(a))
}

fn fpm_get_status(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let Some(st) = with_active(|a| super::status::Status::read(a)) else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Array(st.php_array()))
}

/// What the pool settings give the request.
pub struct Settings<'a> {
    pub limit_extensions: &'a [String],
    /// php_admin_value keys: .user.ini and PHP_VALUE cannot change them.
    pub admin_keys: &'a [String],
}

/// The script and the $_SERVER entries the path settings make (PHP
/// manual: cgi.fix_pathinfo, cgi.discard_path; behaviour as the
/// sapi/fpm/tests/fcgi-env-*.phpt tests expect).
#[derive(Default, Debug)]
pub struct Paths {
    pub script: Option<Vec<u8>>,
    pub script_filename: Option<Vec<u8>>,
    pub script_name: Option<Vec<u8>>,
    pub path_info: Option<Vec<u8>>,
    pub php_self: Vec<u8>,
    pub orig_script_name: Option<Vec<u8>>,
}

fn is_file(p: &[u8]) -> bool {
    !p.is_empty() && Path::new(std::ffi::OsStr::from_bytes(p)).is_file()
}

/// SCRIPT_FILENAME of Apache's mod_proxy_fcgi ("proxy:fcgi://host:port/
/// path?query", "proxy:balancer://cluster/path"): the path.
fn strip_proxy(sf: &[u8]) -> Option<Vec<u8>> {
    let rest = sf.strip_prefix(b"proxy:")?;
    let p = rest.windows(3).position(|w| w == b"://")?;
    let after = &rest[p + 3..];
    let path = match after.iter().position(|&c| c == b'/') {
        Some(s) => &after[s..],
        None => b"",
    };
    let path = match path.iter().position(|&c| c == b'?') {
        Some(q) => &path[..q],
        None => path,
    };
    Some(path.to_vec())
}

pub fn resolve_paths(
    params: &fcgi::Request,
    fix_pathinfo: bool,
    discard_path: bool,
    decode_whole: bool,
) -> Paths {
    let get = |n: &[u8]| params.param(n).map(|v| v.to_vec());
    let mut sf = get(b"SCRIPT_FILENAME");
    let sn = get(b"SCRIPT_NAME");
    let pi = get(b"PATH_INFO").filter(|_| true);
    let pt = get(b"PATH_TRANSLATED");
    let mut proxy = false;
    if let Some(s) = &sf
        && let Some(p) = strip_proxy(s)
    {
        proxy = true;
        sf = Some(if decode_whole {
            goro_ext_standard::url::rawurldecode(&p)
        } else {
            p
        });
    }
    let mut out = Paths {
        script_filename: sf.clone(),
        script_name: sn.clone(),
        path_info: pi.clone(),
        ..Default::default()
    };
    if !fix_pathinfo {
        out.script = if discard_path {
            sf.clone().or(pt.clone())
        } else {
            pt.clone()
                .filter(|p| is_file(p))
                .or(sf.clone())
                .or(pt.clone())
        };
        out.php_self = match &pi {
            Some(p) if !p.is_empty() => p.clone(),
            _ => sn.clone().unwrap_or_default(),
        };
        return out;
    }
    let Some(sf) = sf else {
        out.script = pt;
        out.php_self = sn.clone().unwrap_or_default();
        if let Some(p) = &pi {
            out.php_self.extend_from_slice(p);
        }
        return out;
    };
    if is_file(&sf) {
        out.script = Some(sf);
    } else {
        // The longest leading part that is a file; the rest is the path
        // info.
        let mut found = None;
        let mut end = sf.len();
        while let Some(p) = sf[..end].iter().rposition(|&c| c == b'/') {
            if p == 0 {
                break;
            }
            if is_file(&sf[..p]) {
                found = Some(p);
                break;
            }
            end = p;
        }
        match found {
            Some(p) => {
                let real = sf[..p].to_vec();
                let tail_raw = &sf[p..];
                let tail = if proxy && !decode_whole {
                    goro_ext_standard::url::rawurldecode(tail_raw)
                } else {
                    tail_raw.to_vec()
                };
                out.script = Some(real.clone());
                out.script_filename = Some(real);
                let env_pi_has_tail = pi.as_ref().is_some_and(|v| v.ends_with(&tail));
                out.path_info = match &pi {
                    None => Some(tail.clone()),
                    Some(_) if env_pi_has_tail => Some(tail.clone()),
                    Some(v) => Some(v.clone()),
                };
                match &sn {
                    Some(n) if n.ends_with(&tail) && n.len() > tail.len() => {
                        out.orig_script_name = Some(n.clone());
                        out.script_name = Some(n[..n.len() - tail.len()].to_vec());
                    }
                    _ if env_pi_has_tail => {
                        let v = pi.clone().unwrap_or_default();
                        if v.len() > tail.len() {
                            out.script_name = Some(v[..v.len() - tail.len()].to_vec());
                        }
                    }
                    _ => {}
                }
            }
            None => out.script = Some(sf),
        }
    }
    out.php_self = out.script_name.clone().unwrap_or_default();
    if let Some(p) = &out.path_info {
        out.php_self.extend_from_slice(p);
    }
    out
}

fn ini_on(vm: &Vm, k: &[u8], default: bool) -> bool {
    match vm.get_ini(k) {
        None => default,
        Some(v) => !matches!(
            &v.trim_ascii().to_ascii_lowercase()[..],
            b"0" | b"off" | b"" | b"false" | b"no" | b"none"
        ),
    }
}

/// Settings .user.ini files and PHP_VALUE cannot change (php.ini-only
/// ones; PHP manual: List of php.ini directives, "Changeable").
const SYSTEM_ONLY: &[&str] = &[
    "extension",
    "zend_extension",
    "disable_functions",
    "disable_classes",
    "expose_php",
    "allow_url_fopen",
    "allow_url_include",
    "upload_tmp_dir",
    "max_file_uploads",
    "opcache.enable",
    "user_ini.filename",
    "user_ini.cache_ttl",
];

/// Values a setting refuses (as ini_set() would; test
/// sapi/fpm/tests/php_admin_value-failure.phpt: precision -2).
pub fn ini_value_ok(k: &str, v: &str) -> bool {
    match k {
        "precision" | "serialize_precision" => {
            let n = goro_runtime::convert::str_to_int(v.trim().as_bytes());
            n >= -1
        }
        _ => true,
    }
}

/// `name=value` lines of PHP_VALUE / PHP_ADMIN_VALUE and .user.ini
/// files.
fn ini_lines(vm: &mut Vm, text: &[u8]) -> Vec<(String, String)> {
    struct E<'a>(&'a Vm);
    impl goro_ext_standard::ini::Env for E<'_> {
        fn constant(&mut self, name: &[u8]) -> Option<Vec<u8>> {
            let v = self.0.constants.get(&PhpStr::new(name))?;
            match v {
                Value::Int(i) => Some(i.to_string().into_bytes()),
                Value::Str(s) => Some(s.to_vec()),
                _ => None,
            }
        }
        fn var(&mut self, name: &[u8]) -> Option<Vec<u8>> {
            let n = std::str::from_utf8(name).ok()?;
            std::env::var_os(n).map(|v| v.as_bytes().to_vec())
        }
    }
    let mut out = Vec::new();
    let Ok(ev) =
        goro_ext_standard::ini::parse(text, goro_ext_standard::ini::Mode::Normal, &mut E(vm))
    else {
        return out;
    };
    for e in ev {
        if let goro_ext_standard::ini::Event::Entry {
            key,
            offset: None,
            value,
        } = e
        {
            let v = match value {
                goro_ext_standard::ini::IniValue::Str(s) => {
                    String::from_utf8_lossy(&s).into_owned()
                }
                goro_ext_standard::ini::IniValue::Int(i) => i.to_string(),
                goro_ext_standard::ini::IniValue::Float(f) => f.to_string(),
                goro_ext_standard::ini::IniValue::Bool(true) => "1".into(),
                _ => String::new(),
            };
            out.push((String::from_utf8_lossy(&key).into_owned(), v));
        }
    }
    out
}

fn apply_user_settings(vm: &mut Vm, list: Vec<(String, String)>, locked: &[String]) {
    for (k, v) in list {
        if locked.iter().any(|l| l == &k) || SYSTEM_ONLY.contains(&k.as_str()) {
            continue;
        }
        if !ini_value_ok(&k, &v) {
            continue;
        }
        vm.set_ini(k.as_bytes(), v.as_bytes());
    }
}

/// The .user.ini files from the document root down to the script's
/// directory (PHP manual: .user.ini files).
fn user_ini_files(vm: &Vm, script: &Path, docroot: Option<&[u8]>) -> Vec<PathBuf> {
    let name = vm
        .get_ini(b"user_ini.filename")
        .map(|v| v.to_vec())
        .unwrap_or_else(|| b".user.ini".to_vec());
    if name.is_empty() {
        return Vec::new();
    }
    let name = std::ffi::OsStr::from_bytes(&name).to_os_string();
    let Some(dir) = script.parent() else {
        return Vec::new();
    };
    let root = docroot
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(std::ffi::OsStr::from_bytes(d)));
    let mut dirs = vec![dir.to_path_buf()];
    if let Some(r) = root {
        let r = std::fs::canonicalize(&r).unwrap_or(r);
        let d = std::fs::canonicalize(dir).unwrap_or(dir.to_path_buf());
        if d.starts_with(&r) {
            dirs.clear();
            let mut cur = d.as_path();
            loop {
                dirs.push(cur.to_path_buf());
                if cur == r {
                    break;
                }
                match cur.parent() {
                    Some(p) => cur = p,
                    None => break,
                }
            }
            dirs.reverse();
        }
    }
    dirs.into_iter()
        .map(|d| d.join(&name))
        .filter(|f| f.is_file())
        .collect()
}

/// An HTTP_* parameter's header name ("HTTP_X_FOO" is "X-Foo").
fn header_name(var: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(var.len());
    let mut upper = true;
    for &c in var {
        if c == b'_' {
            out.push(b'-');
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

const POWERED_BY: &[u8] = b"X-Powered-By: PHP/8.5.4";

/// The status and body of a request FPM answers itself (no script).
fn simple_response(vm: &mut Vm, status: &str, body: &[u8], err: &str) {
    send_stderr(err.as_bytes());
    let mut head = format!("Status: {}\r\n", status).into_bytes();
    if ini_on(vm, b"expose_php", true) {
        head.extend_from_slice(POWERED_BY);
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(b"Content-type: text/html; charset=UTF-8\r\n\r\n");
    send_stdout(&head);
    send_stdout(body);
}

/// Runs the request; returns the response status.
pub fn run(vm: &mut Vm, req: &fcgi::Request, set: &Settings) -> i64 {
    let s = |v: &[u8]| Value::Str(PhpStr::from(v.to_vec()));
    // PHP_VALUE / PHP_ADMIN_VALUE parameters (PHP manual: FPM
    // configuration, "passing settings via the web server").
    let mut locked: Vec<String> = set.admin_keys.to_vec();
    if let Some(v) = req.param(b"PHP_ADMIN_VALUE") {
        let l = ini_lines(vm, v);
        for (k, _) in &l {
            locked.push(k.clone());
        }
        apply_user_settings(vm, l, &[]);
    }
    if let Some(v) = req.param(b"PHP_VALUE") {
        let l = ini_lines(vm, v);
        apply_user_settings(vm, l, &locked);
    }
    let fix = ini_on(vm, b"cgi.fix_pathinfo", true);
    let discard = ini_on(vm, b"cgi.discard_path", false);
    let decode_whole = vm.get_ini(b"fastcgi.script_path_encoded").is_some()
        && !ini_on(vm, b"fastcgi.script_path_encoded", true);
    let paths = resolve_paths(req, fix, discard, decode_whole);
    let script = paths.script.clone().filter(|p| is_file(p));
    let Some(script) = script else {
        // (observed behaviour of php-fpm as test
        // sapi/fpm/tests/bug69625-no-script-filename.phpt expects)
        simple_response(
            vm,
            "404 Not Found",
            b"File not found.\n",
            "Primary script unknown",
        );
        return 404;
    };
    if !set.limit_extensions.is_empty()
        && !set
            .limit_extensions
            .iter()
            .any(|e| script.ends_with(e.as_bytes()))
    {
        simple_response(
            vm,
            "403 Forbidden",
            b"Access denied.\n",
            &format!(
                "Access to the script '{}' has been denied (see security.limit_extensions)",
                String::from_utf8_lossy(&script)
            ),
        );
        return 403;
    }
    let script_path = PathBuf::from(std::ffi::OsStr::from_bytes(&script));
    let docroot = req.param(b"DOCUMENT_ROOT").map(|d| d.to_vec());
    for f in user_ini_files(vm, &script_path, docroot.as_deref()) {
        if let Ok(text) = std::fs::read(&f) {
            let l = ini_lines(vm, &text);
            apply_user_settings(vm, l, &locked);
        }
    }

    // The environment: the parameters join it (getenv() sees them), but
    // not HTTP_PROXY (CVE-2016-5385; test
    // sapi/fpm/tests/bug72573-http-proxy.phpt).
    for (k, v) in &req.params {
        if k == b"HTTP_PROXY" || k.is_empty() || k.contains(&b'=') || k.contains(&0) {
            continue;
        }
        if v.contains(&0) {
            continue;
        }
        // SAFETY: this process runs one thread.
        unsafe {
            std::env::set_var(
                std::ffi::OsStr::from_bytes(k),
                std::ffi::OsStr::from_bytes(v),
            );
        }
    }

    let method = req
        .param(b"REQUEST_METHOD")
        .map(|v| v.to_vec())
        .unwrap_or_else(|| b"GET".to_vec());
    let mut server = PhpArray::new();
    for (k, v) in std::env::vars_os() {
        server.insert(Key::from_bytes(k.as_bytes()), s(v.as_bytes()));
    }
    for (k, v) in &req.params {
        if k == b"HTTP_PROXY" {
            continue;
        }
        server.insert(Key::from_bytes(k), s(v));
    }
    let set_or_remove = |a: &mut PhpArray, k: &str, v: &Option<Vec<u8>>| match v {
        Some(v) => {
            a.insert(Key::from(k), s(v));
        }
        None => {
            a.remove(Key::from(k).as_ref());
        }
    };
    set_or_remove(&mut server, "SCRIPT_FILENAME", &paths.script_filename);
    set_or_remove(&mut server, "SCRIPT_NAME", &paths.script_name);
    set_or_remove(&mut server, "PATH_INFO", &paths.path_info);
    if let Some(o) = &paths.orig_script_name {
        server.insert(Key::from("ORIG_SCRIPT_NAME"), s(o));
    }
    server.insert(Key::from("PHP_SELF"), s(&paths.php_self));
    if let Some(auth) = req.param(b"HTTP_AUTHORIZATION")
        && auth.len() > 6
        && auth[..6].eq_ignore_ascii_case(b"basic ")
    {
        let dec = goro_ext_standard::url::base64_decode(auth[6..].trim_ascii(), false)
            .unwrap_or_default();
        if let Some(p) = dec.iter().position(|&b| b == b':') {
            server.insert(Key::from("PHP_AUTH_USER"), s(&dec[..p]));
            server.insert(Key::from("PHP_AUTH_PW"), s(&dec[p + 1..]));
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    server.insert(
        Key::from("REQUEST_TIME_FLOAT"),
        Value::Float(now.as_secs_f64()),
    );
    server.insert(Key::from("REQUEST_TIME"), Value::Int(now.as_secs() as i64));

    let query = req.param(b"QUERY_STRING").map(|v| v.to_vec());
    let register_argv = ini_on(vm, b"register_argc_argv", true);
    let path = std::fs::canonicalize(&script_path)
        .map(|p| p.as_os_str().as_bytes().to_vec())
        .unwrap_or_else(|_| script.clone());
    let argv: Vec<Vec<u8>> = match &query {
        Some(q) if !q.is_empty() => q.split(|&c| c == b'+').map(|w| w.to_vec()).collect(),
        _ => Vec::new(),
    };
    vm.script_path = PhpStr::new(&path);
    vm.argv = argv.iter().map(|a| PhpStr::new(a)).collect();
    if register_argv {
        let args = PhpArray::from_values(argv.iter().map(|a| s(a)));
        server.insert(Key::from("argv"), Value::Array(args.clone()));
        server.insert(Key::from("argc"), Value::Int(argv.len() as i64));
        let g = &mut vm.globals.dyn_vars;
        g.insert(Key::from("argv"), Value::Array(args));
        g.insert(Key::from("argc"), Value::Int(argv.len() as i64));
    }

    // The response.
    let default_type = {
        let mime = vm
            .get_ini(b"default_mimetype")
            .map(|v| v.to_vec())
            .unwrap_or_else(|| b"text/html".to_vec());
        let charset = vm.get_ini(b"default_charset").unwrap_or_default();
        let mut t = mime.clone();
        if !charset.is_empty() && mime.to_ascii_lowercase().starts_with(b"text/") {
            t.extend_from_slice(b"; charset=");
            t.extend_from_slice(&charset);
        }
        t
    };
    let expose = ini_on(vm, b"expose_php", true);
    {
        let sp = &mut vm.out.sapi;
        sp.cgi = true;
        sp.fpm = true;
        sp.status = 200;
        if expose {
            sp.headers.push(POWERED_BY.to_vec());
        }
        sp.conn = Some(Conn {
            transport: Box::new(FcgiTransport),
            protocol: req
                .param(b"SERVER_PROTOCOL")
                .map(|v| v.to_vec())
                .unwrap_or_else(|| b"HTTP/1.1".to_vec()),
            host: None,
            head_only: method == b"HEAD",
            default_type,
            body_bytes: 0,
        });
    }
    vm.constants.insert(
        PhpStr::intern(b"PHP_SAPI"),
        Value::Str(PhpStr::from("fpm-fcgi")),
    );

    let content_type = req.param(b"CONTENT_TYPE").map(|v| v.to_vec());
    let content_length = req
        .param(b"CONTENT_LENGTH")
        .filter(|v| !v.trim_ascii().is_empty())
        .map(|v| goro_runtime::convert::str_to_int(v.trim_ascii()));
    let mut body = req.stdin.clone();
    if let Some(n) = content_length
        && n >= 0
        && (n as usize) < body.len()
    {
        body.truncate(n as usize);
    }
    let cookie = req.param(b"HTTP_COOKIE").map(|v| v.to_vec());
    let data = crate::request_vars::RequestData {
        method: &method,
        query: query.as_deref(),
        content_type: content_type.as_deref(),
        content_length,
        body: &body,
        cookie: cookie.as_deref(),
    };
    crate::request_vars::set_globals(vm, &data, server);
    vm.out.sapi.input = body.clone();
    // getallheaders(): the HTTP_* parameters and the content headers, in
    // the order test sapi/fpm/tests/getallheaders.phpt shows (the last
    // parameter first).
    let mut headers = Vec::new();
    for (k, v) in req.params.iter().rev() {
        if let Some(h) = k.strip_prefix(b"HTTP_") {
            headers.push((header_name(h), v.clone()));
        } else if k == b"CONTENT_TYPE" || k == b"CONTENT_LENGTH" {
            headers.push((header_name(k), v.clone()));
        }
    }
    vm.out.sapi.request_headers = headers;

    vm.included.push(PhpStr::new(&path));
    vm.included_set.insert(PhpStr::new(&path));
    if let Some(d) = Path::new(std::ffi::OsStr::from_bytes(&path)).parent() {
        vm.cwd = PathBuf::from(d);
        let _ = std::env::set_current_dir(d);
    }
    let src = match std::fs::read(&script_path) {
        Ok(src) => src,
        Err(_) => {
            simple_response(
                vm,
                "404 Not Found",
                b"File not found.\n",
                "Primary script unknown",
            );
            return 404;
        }
    };
    if register_argv && let Some(line) = crate::cgi::first_server_use(&src) {
        let _ = vm.raise_at(
            crate::vm::errors::E_DEPRECATED,
            "Deriving $_SERVER['argv'] from the query string is deprecated. Configure register_argc_argv=0 to turn this message off",
            PhpStr::new(&path),
            line,
        );
    }
    let src = if src.starts_with(b"#!") {
        match src.iter().position(|&c| c == b'\n') {
            Some(p) => {
                let mut v = b"<?php ?>".to_vec();
                v.extend_from_slice(&src[p..]);
                v
            }
            None => Vec::new(),
        }
    } else {
        src
    };
    if ini_on(vm, b"zlib.output_compression", false)
        && vm.functions.contains_key(&PhpStr::from("ob_gzhandler"))
    {
        crate::builtins::zlib::start_output_compression(vm);
    }
    crate::builtins::output::start_ini_handler(vm);
    vm.run_script(&src, &path);
    vm.out.flush();
    vm.out.sapi.send_head();
    for f in std::mem::take(&mut vm.out.sapi.uploads) {
        let _ = std::fs::remove_file(Path::new(std::ffi::OsStr::from_bytes(&f)));
    }
    let status = vm.out.sapi.status;
    if let Some(a) = with_active(|a| (a.board, a.slot)) {
        // SAFETY: the board's mapping outlives the request process.
        let b = unsafe { &*a.0 };
        let sl = b.slot(a.1);
        sl.memory.store(
            vm.memory_peak() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        sl.state
            .store(scoreboard::FINISHING, std::sync::atomic::Ordering::Relaxed);
    }
    if status == 0 { 200 } else { status }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_paths() {
        assert_eq!(
            strip_proxy(b"proxy:fcgi://127.0.0.1:9000/a/b.php?q=1").unwrap(),
            b"/a/b.php"
        );
        assert_eq!(
            strip_proxy(b"proxy:balancer://c/x").unwrap(),
            b"/x".to_vec()
        );
        assert!(strip_proxy(b"/a/b.php").is_none());
    }

    #[test]
    fn header_names() {
        assert_eq!(header_name(b"X_FOO"), b"X-Foo");
        assert_eq!(header_name(b"CONTENT_TYPE"), b"Content-Type");
    }
}
