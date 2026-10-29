//! The CGI SAPI, `goro --cgi` (or goro run as `php-cgi`): one request
//! taken from the CGI environment and stdin, answered on stdout (RFC 3875,
//! the CGI/1.1 specification; PHP manual: Predefined Variables, $_SERVER,
//! Handling file uploads; behaviour as the tests with --GET--, --POST--,
//! --COOKIE--, --CGI-- and --EXPECTHEADERS-- sections expect).
//!
//! - The request: REQUEST_METHOD, QUERY_STRING, CONTENT_TYPE,
//!   CONTENT_LENGTH (that many body bytes are read from stdin), HTTP_COOKIE
//!   and the other HTTP_* variables, SCRIPT_FILENAME / PATH_TRANSLATED when
//!   no script is named on the command line.
//! - The response (RFC 3875 6): header lines (a Status header for a
//!   status other than 200, X-Powered-By, the script's headers, the default
//!   Content-type), an empty line, then the body; `-q` leaves the header
//!   lines out.

use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::Vm;
use crate::vm::sapi::{Conn, Transport};

/// What `goro --cgi` was given.
#[derive(Default)]
pub struct CgiOptions {
    /// The script named on the command line (`-f FILE` or the first
    /// argument); otherwise SCRIPT_FILENAME / PATH_TRANSLATED, then stdin.
    pub script: Option<Vec<u8>>,
    /// Arguments after the script.
    pub args: Vec<Vec<u8>>,
    /// `-d` settings (and php.ini ones), in order.
    pub ini: Vec<(Vec<u8>, Vec<u8>)>,
    /// `-q`: no header lines.
    pub no_headers: bool,
    /// `-C`: stay in the current directory.
    pub no_chdir: bool,
}

/// The response on stdout.
struct CgiTransport {
    headers: bool,
}

impl Transport for CgiTransport {
    fn write_head(&mut self, _status_line: &[u8], headers: &[Vec<u8>]) {
        if !self.headers {
            return;
        }
        let mut out = Vec::new();
        for h in headers {
            out.extend_from_slice(h);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(&out);
        let _ = so.flush();
    }

    fn write_body(&mut self, b: &[u8]) {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(b);
        let _ = so.flush();
    }
}

fn env(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(|v| v.into_vec())
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

/// An HTTP_* variable's header name, as apache_request_headers() shows it
/// (test sapi/cgi/tests/apache_request_headers.phpt: HTTP_X_TEST is
/// "X-Test", HTTP_X__TEST "X-_test", HTTP_X_ "X-").
fn header_name(var: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(var.len());
    let mut upper = true;
    let mut i = 0;
    while i < var.len() {
        let c = var[i];
        if c == b'_' && !upper {
            out.push(b'-');
            upper = true;
            if let Some(&n) = var.get(i + 1) {
                out.push(n.to_ascii_uppercase());
                upper = false;
                i += 1;
            }
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c.to_ascii_lowercase());
        }
        i += 1;
    }
    out
}

/// The text of a `-d`-style default: display_errors on, HTML errors, no
/// implicit flush and a 30 s time limit (the CGI SAPI's defaults, unlike
/// the CLI's).
const DEFAULT_INI: &[(&str, &str)] = &[
    ("display_errors", "1"),
    ("html_errors", "1"),
    ("implicit_flush", "0"),
    ("max_execution_time", "30"),
];

/// The line of the first `$_SERVER` in a script, where the argv
/// deprecation is reported (PHP fills $_SERVER when it compiles the first
/// use; test tests/basic/011.phpt).
fn first_server_use(src: &[u8]) -> Option<u32> {
    // Only a script reading argv or argc (test
    // sapi/cgi/tests/auto_globals_no_jit.phpt reads other entries).
    if !src.windows(4).any(|w| w == b"argv" || w == b"argc") {
        return None;
    }
    let p = src.windows(8).position(|w| w == b"$_SERVER")?;
    Some(1 + src[..p].iter().filter(|&&c| c == b'\n').count() as u32)
}

/// Runs one CGI request; returns the exit status.
pub fn run(opts: CgiOptions) -> i32 {
    let mut ini: Vec<(Vec<u8>, Vec<u8>)> = DEFAULT_INI
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
        .collect();
    ini.extend(opts.ini);
    let method = env("REQUEST_METHOD").unwrap_or_else(|| b"GET".to_vec());
    // The response exists from the start: startup diagnostics are its
    // first body bytes.
    let mut vm = crate::startup_with(&ini, &[&crate::cli_server::SERVER_MODULE], |vm| {
        let s = &mut vm.out.sapi;
        s.cgi = true;
        s.status = 200;
        s.headers.push(POWERED_BY.to_vec());
        s.conn = Some(Conn {
            transport: Box::new(CgiTransport {
                headers: !opts.no_headers,
            }),
            protocol: env("SERVER_PROTOCOL").unwrap_or_else(|| b"HTTP/1.0".to_vec()),
            host: None,
            head_only: method == b"HEAD",
            default_type: b"text/html; charset=UTF-8".to_vec(),
            body_bytes: 0,
        });
    });
    vm.constants.insert(
        PhpStr::intern(b"PHP_SAPI"),
        Value::Str(PhpStr::from("cgi-fcgi")),
    );
    configure_response(&mut vm);
    let query = env("QUERY_STRING");
    let content_type = env("CONTENT_TYPE");
    let content_length = env("CONTENT_LENGTH")
        .filter(|v| !v.trim_ascii().is_empty())
        .map(|v| goro_runtime::convert::str_to_int(v.trim_ascii()));
    let cookie = env("HTTP_COOKIE");

    // The script.
    let named = opts.script.clone().or_else(|| {
        env("SCRIPT_FILENAME")
            .or_else(|| env("PATH_TRANSLATED"))
            .filter(|s| !s.is_empty())
    });
    let mut body = Vec::new();
    let (src, path, display) = match &named {
        Some(p) => {
            let fp = Path::new(std::ffi::OsStr::from_bytes(p));
            match std::fs::read(fp) {
                Ok(s) => {
                    let abs = std::fs::canonicalize(fp).unwrap_or_else(|_| fp.to_path_buf());
                    (s, abs.as_os_str().as_bytes().to_vec(), p.clone())
                }
                Err(_) => {
                    // (RFC 3875 6.3.3: the script is not there)
                    vm.out.sapi.status = 404;
                    vm.out.write_stdout(b"No input file specified.\n");
                    vm.out.flush();
                    vm.out.sapi.send_head();
                    return 1;
                }
            }
        }
        None => {
            let _ = std::io::stdin().read_to_end(&mut body);
            let s = std::mem::take(&mut body);
            (s, b"Standard input code".to_vec(), b"-".to_vec())
        }
    };
    if named.is_some()
        && let Some(n) = content_length.filter(|&n| n > 0)
    {
        let _ = std::io::stdin().take(n as u64).read_to_end(&mut body);
    }
    if body.len() > POST_MEMORY_LIMIT && !can_spool(&vm) {
        crate::request_vars::startup_warning(
            &mut vm,
            "Unable to create temporary file, Check permissions in temporary files directory.",
        );
        crate::request_vars::startup_warning(
            &mut vm,
            "POST data can't be buffered; all data discarded",
        );
        body.clear();
    }

    // $_SERVER: the CGI environment, then what PHP adds.
    let s = |v: &[u8]| Value::Str(PhpStr::from(v.to_vec()));
    let mut server = PhpArray::new();
    let mut request_headers = Vec::new();
    for (k, v) in std::env::vars_os() {
        let (k, v) = (k.into_vec(), v.into_vec());
        if let Some(h) = k.strip_prefix(b"HTTP_") {
            request_headers.push((header_name(h), v.clone()));
        }
        server.insert(Key::from_bytes(&k), s(&v));
    }
    let script_name = env("SCRIPT_NAME").unwrap_or_else(|| display.clone());
    let mut php_self = script_name.clone();
    if let Some(pi) = env("PATH_INFO") {
        php_self.extend_from_slice(&pi);
    }
    for (k, v) in [
        (
            "SCRIPT_FILENAME",
            env("SCRIPT_FILENAME").unwrap_or(path.clone()),
        ),
        (
            "PATH_TRANSLATED",
            env("PATH_TRANSLATED").unwrap_or(path.clone()),
        ),
        ("SCRIPT_NAME", script_name),
        ("DOCUMENT_ROOT", env("DOCUMENT_ROOT").unwrap_or_default()),
    ] {
        server.insert(Key::from(k), s(&v));
    }
    server.insert(Key::from("PHP_SELF"), s(&php_self));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    server.insert(
        Key::from("REQUEST_TIME_FLOAT"),
        Value::Float(now.as_secs_f64()),
    );
    server.insert(Key::from("REQUEST_TIME"), Value::Int(now.as_secs() as i64));
    // argv/argc: the command line's arguments, or with a query string and
    // none, its `+`-separated words (PHP manual: register_argc_argv; tests
    // tests/basic/011*.phpt).
    let mut argv_from_query = false;
    let argv: Vec<Vec<u8>> = match &query {
        Some(q) if opts.args.is_empty() => {
            argv_from_query = true;
            if q.is_empty() {
                Vec::new()
            } else {
                q.split(|&c| c == b'+').map(|w| w.to_vec()).collect()
            }
        }
        _ => std::iter::once(display.clone())
            .chain(opts.args.iter().cloned())
            .collect(),
    };
    let register_argv = ini_on(&vm, b"register_argc_argv", true);
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
    let data = crate::request_vars::RequestData {
        method: &method,
        query: query.as_deref(),
        content_type: content_type.as_deref(),
        content_length,
        body: &body,
        cookie: cookie.as_deref(),
    };
    crate::request_vars::set_globals(&mut vm, &data, server);
    vm.out.sapi.input = body.clone();
    vm.out.sapi.request_headers = request_headers;

    if named.is_some() {
        vm.included.push(PhpStr::new(&path));
        vm.included_set.insert(PhpStr::new(&path));
        if !opts.no_chdir
            && let Some(d) = Path::new(std::ffi::OsStr::from_bytes(&path)).parent()
        {
            vm.cwd = PathBuf::from(d);
            let _ = std::env::set_current_dir(d);
        }
    }
    if register_argv
        && argv_from_query
        && let Some(line) = first_server_use(&src)
    {
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
    // zlib.output_compression: the output goes through ob_gzhandler, which
    // compresses for clients that accept it (PHP manual: Zlib runtime
    // configuration; tests ext/zlib/tests/ob_001.phpt, bug51269.phpt).
    if ini_on(&vm, b"zlib.output_compression", false)
        && vm.functions.contains_key(&PhpStr::from("ob_gzhandler"))
    {
        crate::builtins::zlib::start_output_compression(&mut vm);
    }
    crate::builtins::output::start_ini_handler(&mut vm);
    let status = vm.run_script(&src, &path);
    vm.out.flush();
    vm.out.sapi.send_head();
    for f in std::mem::take(&mut vm.out.sapi.uploads) {
        let _ = std::fs::remove_file(Path::new(std::ffi::OsStr::from_bytes(&f)));
    }
    status
}

const POWERED_BY: &[u8] = b"X-Powered-By: PHP/8.5.4";

/// A request body larger than this goes to a temporary file in
/// sys_temp_dir; when none can be made, the body is discarded with
/// warnings (test sapi/cgi/tests/bug69487.phpt, a 16387-byte body).
const POST_MEMORY_LIMIT: usize = 16384;

/// Whether a temporary file can be made for the body.
fn can_spool(vm: &Vm) -> bool {
    let dir = vm
        .get_ini(b"sys_temp_dir")
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(std::ffi::OsStr::from_bytes(&d)))
        .unwrap_or_else(std::env::temp_dir);
    let p = dir.join(format!(".goro-post-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&p);
            true
        }
        Err(e) => e.kind() == std::io::ErrorKind::AlreadyExists,
    }
}

/// The response as the settings make it, unless already sent:
/// X-Powered-By (expose_php) and the default Content-type
/// (default_mimetype, default_charset).
fn configure_response(vm: &mut Vm) {
    if vm.out.sapi.sent {
        return;
    }
    let mime = vm
        .get_ini(b"default_mimetype")
        .map(|v| v.to_vec())
        .unwrap_or_else(|| b"text/html".to_vec());
    let charset = vm.get_ini(b"default_charset").unwrap_or_default();
    let mut default_type = mime.clone();
    if !charset.is_empty() && mime.to_ascii_lowercase().starts_with(b"text/") {
        default_type.extend_from_slice(b"; charset=");
        default_type.extend_from_slice(&charset);
    }
    let expose = ini_on(vm, b"expose_php", true);
    let s = &mut vm.out.sapi;
    if !expose {
        s.headers.retain(|h| h != POWERED_BY);
    }
    if let Some(c) = s.conn.as_mut() {
        c.default_type = default_type;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_names() {
        assert_eq!(header_name(b"X_TEST"), b"X-Test");
        assert_eq!(header_name(b"X__TEST"), b"X-_test");
        assert_eq!(header_name(b"X_"), b"X-");
        assert_eq!(header_name(b"USER_AGENT"), b"User-Agent");
    }

    #[test]
    fn server_use_line() {
        assert_eq!(first_server_use(b"<?php\n$a = $_SERVER['argv'];"), Some(2));
        assert_eq!(first_server_use(b"<?php echo 1;"), None);
    }
}
