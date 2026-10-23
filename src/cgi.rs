//! The CGI SAPI (sapi/cgi/cgi_main.c), without its FastCGI mode: what
//! `php-cgi` does when run by a web server (CGI environment variables) or
//! from a shell. goro runs as php-cgi when invoked under a name containing
//! "cgi" (e.g. a `php-cgi` symlink to the goro binary).

use std::cell::Cell;
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use goro::engine::{self, IniSettings};
use goro::websapi;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::getopt::{CGI_OPTIONS, GETOPT_INVALID_ARG, GetOpt};

const SAPI_NAME: &str = "cgi-fcgi";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Behavior {
    Standard,
    Highlight,
    Lint,
    Strip,
}

thread_local! {
    /// SG(request_info).no_headers
    static NO_HEADERS: Cell<bool> = const { Cell::new(false) };
    /// cgi.nph / cgi.rfc2616_headers
    static NPH: Cell<bool> = const { Cell::new(false) };
    static RFC2616: Cell<bool> = const { Cell::new(false) };
}

fn getenv(k: &str) -> Option<Vec<u8>> {
    std::env::var_os(k).map(|v| v.into_vec())
}

/// CGI_PUTENV(): set (or, for None, remove) an environment variable.
fn putenv(k: &str, v: Option<&[u8]>) -> Option<Vec<u8>> {
    // SAFETY: the CGI SAPI is single-threaded.
    unsafe {
        match v {
            Some(v) => std::env::set_var(k, std::ffi::OsStr::from_bytes(v)),
            None => std::env::remove_var(k),
        }
    }
    v.map(|v| v.to_vec())
}

/// The last value an INI setting got.
fn ini_get<'a>(ini: &'a IniSettings, key: &str) -> Option<&'a str> {
    ini.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn ini_bool(ini: &IniSettings, key: &str, default: bool) -> bool {
    match ini_get(ini, key) {
        Some(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "on" | "yes" | "true")
                || v.parse::<i64>().is_ok_and(|n| n != 0)
        }
        None => default,
    }
}

/// php_url_decode()
fn url_decode(s: &[u8]) -> Vec<u8> {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < s.len() => match (hex(s[i + 1]), hex(s[i + 2])) {
                (Some(a), Some(b)) => {
                    out.push(a * 16 + b);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// php_cgi_usage()
fn usage(argv0: &[u8]) -> String {
    let a = String::from_utf8_lossy(argv0);
    let prog = match a.rfind('/') {
        Some(p) => &a[p + 1..],
        None => "php-cgi",
    };
    format!(
        "Usage: {p} [-q] [-h] [-s] [-v] [-i] [-f <file>]\n\
         \x20      {p} <file> [args...]\n\
         \x20 -a               Run interactively\n\
         \x20 -b <address:port>|<port> Bind Path for external FASTCGI Server mode\n\
         \x20 -C               Do not chdir to the script's directory\n\
         \x20 -c <path>|<file> Look for php.ini file in this directory\n\
         \x20 -n               No php.ini file will be used\n\
         \x20 -d foo[=bar]     Define INI entry foo with value 'bar'\n\
         \x20 -e               Generate extended information for debugger/profiler\n\
         \x20 -f <file>        Parse <file>.  Implies `-q'\n\
         \x20 -h               This help\n\
         \x20 -i               PHP information\n\
         \x20 -l               Syntax check only (lint)\n\
         \x20 -m               Show compiled in modules\n\
         \x20 -q               Quiet-mode.  Suppress HTTP Header output.\n\
         \x20 -s               Display colour syntax highlighted source.\n\
         \x20 -v               Version number\n\
         \x20 -w               Display source with stripped comments and whitespace.\n\
         \x20 -T <count>       Measure execution time of script repeated <count> times.\n",
        p = prog
    )
}

fn stdout_write(b: &[u8]) {
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(b);
    let _ = so.flush();
}

/// SG(request_info) as init_request_info() fills it from the environment.
#[derive(Default)]
struct CgiRequest {
    path_translated: Option<Vec<u8>>,
    request_method: Option<Vec<u8>>,
    query_string: Option<Vec<u8>>,
    content_type: Option<Vec<u8>>,
    content_length: i64,
    response_code: i64,
}

/// is_valid_path(): no ".." path segment.
fn is_valid_path(p: &[u8]) -> bool {
    !p.split(|&c| c == b'/').any(|seg| seg == b"..")
}

/// init_request_info() (CGI, cgi.fix_pathinfo handling included).
fn init_request_info(ini: &IniSettings) -> CgiRequest {
    let mut req = CgiRequest {
        response_code: 200,
        ..Default::default()
    };
    let env_script_filename = getenv("SCRIPT_FILENAME");
    let mut env_path_translated = getenv("PATH_TRANSLATED");
    let mut script_path_translated = env_script_filename
        .clone()
        .or_else(|| env_path_translated.clone());
    if script_path_translated.is_none() {
        return req;
    }
    let content_length = getenv("CONTENT_LENGTH");
    let content_type = getenv("CONTENT_TYPE");
    let mut env_path_info = getenv("PATH_INFO");
    let mut env_script_name = getenv("SCRIPT_NAME");
    if ini_bool(ini, "cgi.fix_pathinfo", true) {
        let env_redirect_url = getenv("REDIRECT_URL");
        let mut env_document_root = getenv("DOCUMENT_ROOT");
        let orig_path_translated = env_path_translated.clone();
        let orig_path_info = env_path_info.clone();
        let orig_script_name = env_script_name.clone();
        let orig_script_filename = env_script_filename.clone();
        if env_document_root.is_none() {
            if let Some(dr) = ini_get(ini, "doc_root").filter(|d| !d.is_empty()) {
                env_document_root = putenv("DOCUMENT_ROOT", Some(dr.as_bytes()));
            }
        }
        if let (Some(pt), Some(ru)) = (&env_path_translated, &env_redirect_url) {
            if Some(pt) != script_path_translated.as_ref() {
                script_path_translated = Some(pt.clone());
                env_script_name = Some(ru.clone());
            }
        }
        let spt = script_path_translated.clone().unwrap_or_default();
        let real_path = std::fs::canonicalize(std::ffi::OsStr::from_bytes(&spt)).ok();
        let mut request_uri: Option<Vec<u8>> = None;
        if !spt.is_empty() && (spt.ends_with(b"/") || real_path.is_none()) {
            // Strip path components until a regular file is found: the
            // rest is PATH_INFO (/info.php/test).
            let mut pt = spt.clone();
            let len = spt.len();
            let mut found = false;
            while let Some(p) = pt.iter().rposition(|&c| c == b'/') {
                pt.truncate(p);
                let is_file = std::fs::metadata(std::ffi::OsStr::from_bytes(&pt))
                    .map(|m| m.is_file())
                    .unwrap_or(false);
                if !is_file {
                    continue;
                }
                found = true;
                let slen = len - pt.len();
                let pilen = env_path_info.as_ref().map_or(0, |p| p.len());
                let path_info: Option<Vec<u8>> = env_path_info
                    .as_ref()
                    .map(|p| p[pilen.saturating_sub(slen)..].to_vec());
                if orig_path_info != path_info {
                    if let Some(opi) = &orig_path_info {
                        putenv("ORIG_PATH_INFO", Some(opi));
                        let epi = env_path_info.clone().unwrap_or_default();
                        let cut = &epi[..pilen.saturating_sub(slen)];
                        if orig_script_name.as_deref() != Some(cut) {
                            if let Some(osn) = &orig_script_name {
                                putenv("ORIG_SCRIPT_NAME", Some(osn));
                            }
                            request_uri = putenv("SCRIPT_NAME", Some(cut));
                        } else {
                            request_uri = orig_script_name.clone();
                        }
                    }
                    env_path_info = putenv("PATH_INFO", path_info.as_deref());
                }
                if orig_script_filename.as_deref() != Some(&pt[..]) {
                    if let Some(osf) = &orig_script_filename {
                        putenv("ORIG_SCRIPT_FILENAME", Some(osf));
                    }
                    script_path_translated = putenv("SCRIPT_FILENAME", Some(&pt));
                }
                // PATH_TRANSLATED = DOCUMENT_ROOT + PATH_INFO, or
                // SCRIPT_FILENAME - SCRIPT_NAME + PATH_INFO
                let pi = env_path_info.clone().unwrap_or_default();
                let new_pt = if let Some(dr) = &env_document_root {
                    let mut d = dr.clone();
                    if d.ends_with(b"/") {
                        d.pop();
                    }
                    d.extend_from_slice(&pi);
                    Some(d)
                } else if let Some(sn) = env_script_name
                    .as_ref()
                    .filter(|sn| pt.windows(sn.len().max(1)).any(|w| w == sn.as_slice()))
                {
                    let mut d = pt[..pt.len() - sn.len().min(pt.len())].to_vec();
                    d.extend_from_slice(&pi);
                    Some(d)
                } else {
                    None
                };
                if let Some(np) = new_pt {
                    if let Some(opt) = &orig_path_translated {
                        putenv("ORIG_PATH_TRANSLATED", Some(opt));
                    }
                    env_path_translated = putenv("PATH_TRANSLATED", Some(&np));
                }
                break;
            }
            if !found {
                if let Some(osf) = &orig_script_filename {
                    putenv("ORIG_SCRIPT_FILENAME", Some(osf));
                }
                script_path_translated = putenv("SCRIPT_FILENAME", None);
                req.response_code = 404;
            }
            if request_uri.is_none() && orig_script_name != env_script_name {
                if let Some(osn) = &orig_script_name {
                    putenv("ORIG_SCRIPT_NAME", Some(osn));
                }
                putenv("SCRIPT_NAME", env_script_name.as_deref());
            }
        } else {
            if orig_script_filename.as_ref() != Some(&spt) {
                if let Some(osf) = &orig_script_filename {
                    putenv("ORIG_SCRIPT_FILENAME", Some(osf));
                }
                script_path_translated = putenv("SCRIPT_FILENAME", Some(&spt));
            }
            if env_redirect_url.is_some() {
                if let Some(opi) = &orig_path_info {
                    putenv("ORIG_PATH_INFO", Some(opi));
                    putenv("PATH_INFO", None);
                }
                if let Some(opt) = &orig_path_translated {
                    putenv("ORIG_PATH_TRANSLATED", Some(opt));
                    putenv("PATH_TRANSLATED", None);
                }
            }
            if env_script_name != orig_script_name {
                if let Some(osn) = &orig_script_name {
                    putenv("ORIG_SCRIPT_NAME", Some(osn));
                }
                putenv("SCRIPT_NAME", env_script_name.as_deref());
            }
        }
        let _ = env_path_translated;
    } else if !ini_bool(ini, "cgi.discard_path", false) && env_path_translated.is_some() {
        script_path_translated = env_path_translated;
    }
    if let Some(spt) = script_path_translated.filter(|p| is_valid_path(p)) {
        req.path_translated = Some(spt);
    }
    req.request_method = getenv("REQUEST_METHOD");
    req.query_string = getenv("QUERY_STRING");
    req.content_type = Some(content_type.unwrap_or_default());
    req.content_length = content_length
        .map(|c| {
            let s = String::from_utf8_lossy(&c).trim().to_string();
            let d: String = s
                .chars()
                .enumerate()
                .take_while(|(i, ch)| ch.is_ascii_digit() || (*i == 0 && *ch == '-'))
                .map(|(_, ch)| ch)
                .collect();
            d.parse().unwrap_or(0)
        })
        .unwrap_or(0);
    req
}

/// sapi_cgi_send_headers(), run when the headers are sent.
fn send_headers(vm: &mut Vm) {
    if NO_HEADERS.with(|n| n.get()) {
        return;
    }
    // sapi_send_headers(): the default Content-type joins the list.
    if goro_core::sapi_headers::send_default_content_type() {
        let get = |k: &[u8]| {
            vm.constants
                .get(k)
                .map(|v| v.to_php_string().as_bytes().to_vec())
        };
        let mimetype = get(b"default_mimetype")
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| b"text/html".to_vec());
        let charset = get(b"default_charset").unwrap_or_else(|| b"UTF-8".to_vec());
        let mut line = b"Content-type: ".to_vec();
        line.extend_from_slice(&mimetype);
        if !charset.is_empty()
            && mimetype.len() >= 5
            && mimetype[..5].eq_ignore_ascii_case(b"text/")
        {
            line.extend_from_slice(b"; charset=");
            line.extend_from_slice(&charset);
        }
        goro_ext_standard::misc::sapi_add_header(&line, false);
        goro_core::sapi_headers::content_type_set(&line[14..]);
    }
    let code = goro_ext_standard::misc::peek_response_code();
    let status_line = goro_ext_standard::misc::peek_status_line();
    let headers = goro_ext_standard::misc::peek_sent_headers();
    let is_status = |k: &[u8]| k.eq_ignore_ascii_case(b"Status");
    let mut out: Vec<u8> = Vec::new();
    let mut ignore_status = false;
    let mut response_status = code;
    if NPH.with(|n| n.get()) || code != 200 {
        let mut buf: Option<Vec<u8>> = None;
        let mut has_status = false;
        if RFC2616.with(|r| r.get()) && status_line.is_some() {
            let sl = status_line.clone().unwrap();
            if let Some(sp) = sl.iter().position(|&c| c == b' ') {
                response_status = atoi(&sl[sp + 1..]);
            }
            buf = Some(sl);
        } else {
            let sp = status_line
                .as_ref()
                .and_then(|sl| sl.iter().position(|&c| c == b' ').map(|p| (sl, p)));
            match sp {
                Some((sl, p)) if p >= 5 && sl[..5].eq_ignore_ascii_case(b"HTTP/") => {
                    let mut b = b"Status:".to_vec();
                    b.extend_from_slice(&sl[p..]);
                    response_status = atoi(&sl[p + 1..]);
                    buf = Some(b);
                }
                _ => {
                    has_status = headers.iter().any(|(k, _)| is_status(k));
                    if !has_status {
                        let reason = websapi::status_string(code as i32);
                        buf = Some(if reason == "Unknown Status Code" {
                            format!("Status: {}", code).into_bytes()
                        } else {
                            format!("Status: {} {}", code, reason).into_bytes()
                        });
                    }
                }
            }
        }
        if !has_status {
            if let Some(b) = buf {
                out.extend_from_slice(&b[..b.len().min(1024)]);
                out.extend_from_slice(b"\r\n");
                ignore_status = true;
            }
        }
    }
    for (k, v) in &headers {
        if k.is_empty() && v.is_empty() {
            continue;
        }
        if is_status(k) {
            if !ignore_status {
                ignore_status = true;
            } else {
                continue;
            }
        } else if response_status == 304 && k.eq_ignore_ascii_case(b"Content-Type") {
            continue;
        }
        out.extend_from_slice(k);
        out.extend_from_slice(b": ");
        out.extend_from_slice(v);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    stdout_write(&out);
}

fn atoi(b: &[u8]) -> i64 {
    let d: Vec<u8> = b
        .iter()
        .copied()
        .skip_while(|c| *c == b' ')
        .take_while(|c| c.is_ascii_digit())
        .collect();
    String::from_utf8_lossy(&d).parse().unwrap_or(0)
}

/// apache_request_headers() / getallheaders() of the CGI SAPI: the HTTP_*
/// environment variables (and CONTENT_TYPE / CONTENT_LENGTH).
fn cgi_request_headers(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for (k, v) in std::env::vars_os() {
        let k = k.into_vec();
        let name: Vec<u8> = if k.len() > 5 && k.starts_with(b"HTTP_") {
            let p = &k[5..];
            let mut q = vec![p[0]];
            let mut i = 1;
            while i < p.len() {
                let c = p[i];
                if c == b'_' {
                    q.push(b'-');
                    i += 1;
                    if i < p.len() {
                        q.push(p[i]);
                        i += 1;
                    }
                } else {
                    q.push(c.to_ascii_lowercase());
                    i += 1;
                }
            }
            q
        } else if k == b"CONTENT_TYPE" {
            b"Content-Type".to_vec()
        } else if k == b"CONTENT_LENGTH" {
            b"Content-Length".to_vec()
        } else {
            continue;
        };
        arr.set(
            ArrayKey::String(PhpString::from_vec(name)),
            Value::String(PhpString::from_vec(v.into_vec())),
        );
    }
    Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr))))
}

/// What one request runs.
struct Job<'a> {
    behavior: Behavior,
    cgi: bool,
    no_headers: bool,
    no_chdir: bool,
    script_file: Option<Vec<u8>>,
    /// The script arguments (SG(request_info).argv), from the command line.
    args: &'a [Vec<u8>],
    query_string: Option<Vec<u8>>,
}

/// php_request_startup() .. php_request_shutdown() of one request.
/// Returns the exit status, or Err when the script could not be opened
/// (main() then fails).
fn run_request(ini: &IniSettings, req: &CgiRequest, job: &Job) -> Result<i32, ()> {
    NO_HEADERS.with(|n| n.set(job.no_headers));
    NPH.with(|n| n.set(ini_bool(ini, "cgi.nph", false)));
    RFC2616.with(|n| n.set(ini_bool(ini, "cgi.rfc2616_headers", false)));
    let path_translated = job
        .script_file
        .clone()
        .or_else(|| req.path_translated.clone());

    let mut vm = engine::new_vm(ini, SAPI_NAME);
    vm.register_function(b"apache_request_headers", cgi_request_headers);
    vm.register_function(b"getallheaders", cgi_request_headers);
    vm.cli_direct_output = true;
    vm.sapi_send_headers = Some(send_headers);
    let _ = goro_ext_standard::misc::take_sent_headers();
    let _ = goro_ext_standard::misc::take_status_line();
    goro_core::sapi_headers::reset();

    let mut info = goro_ext_standard::request::RequestInfo {
        cgi: true,
        ..Default::default()
    };
    info.request_method = req.request_method.clone();
    info.query_string = job.query_string.clone().unwrap_or_default();
    info.cookie_data = getenv("HTTP_COOKIE");
    info.content_type = req.content_type.clone().filter(|c| !c.is_empty());
    info.content_length = req.content_length;
    // sapi_cgi_read_post(): the body comes from stdin.
    if req.content_length > 0 {
        let mut body = Vec::new();
        let _ = std::io::stdin()
            .lock()
            .take(req.content_length as u64)
            .read_to_end(&mut body);
        info.body = body;
    }
    info.argv = job.args.to_vec();
    info.env = std::env::vars_os()
        .map(|(k, v)| (k.into_vec(), v.into_vec()))
        .collect();
    info.script_filename = path_translated.clone().unwrap_or_default();
    goro_ext_standard::misc::set_translated_path(
        path_translated
            .as_ref()
            .map(|p| String::from_utf8_lossy(p).into_owned()),
    );
    engine::request_startup(&mut vm, &info, ini);
    // init_request_info()'s response code (404 when no script was found).
    if req.response_code != 200 {
        set_response_code(&mut vm, req.response_code);
    }

    // php_fopen_primary_script() / stdin
    let (src, filename, display_name): (Vec<u8>, String, String) =
        if job.cgi || path_translated.is_some() {
            let p = path_translated.clone().unwrap_or_default();
            let read = if p.is_empty() {
                Err(std::io::Error::from(std::io::ErrorKind::NotFound))
            } else {
                let os = std::ffi::OsStr::from_bytes(&p);
                if Path::new(os).is_dir() {
                    Err(std::io::Error::from(std::io::ErrorKind::NotFound))
                } else {
                    std::fs::read(os)
                }
            };
            match read {
                Ok(s) => {
                    let name = std::fs::canonicalize(std::ffi::OsStr::from_bytes(&p))
                        .map(|c| c.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&p).into_owned());
                    (s, name, String::from_utf8_lossy(&p).into_owned())
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        set_response_code(&mut vm, 403);
                        vm.write_output(b"Access denied.\n");
                    } else {
                        set_response_code(&mut vm, 404);
                        vm.write_output(b"No input file specified.\n");
                    }
                    finish(&mut vm, Ok(Value::Null));
                    return Err(());
                }
            }
        } else {
            let mut v = Vec::new();
            let _ = std::io::stdin().lock().read_to_end(&mut v);
            (
                v,
                "Standard input code".to_string(),
                "Standard input code".to_string(),
            )
        };

    let mut exit_status = 0;
    let result: Result<Value, VmError> = match job.behavior {
        Behavior::Standard => {
            vm.current_file = filename.as_str().into();
            let dir = if job.no_chdir || filename == "Standard input code" {
                None
            } else {
                Path::new(&filename).parent().map(|d| d.to_path_buf())
            };
            websapi::with_cwd(dir, || match engine::compile(&src, &filename, true) {
                Ok(mut c) => {
                    engine::load(&mut vm, &mut c);
                    vm.execute(&c.op_array)
                }
                Err(e) => {
                    engine::report_compile_error(&mut vm, &e, &filename);
                    vm.fatal_emitted = true;
                    Err(VmError {
                        message: e.message,
                        line: e.line,
                    })
                }
            })
        }
        Behavior::Lint => {
            match engine::compile(&src, &filename, true) {
                Ok(_) => vm.write_output(
                    format!("No syntax errors detected in {}\n", display_name).as_bytes(),
                ),
                Err(e) => {
                    engine::report_compile_error(&mut vm, &e, &filename);
                    vm.write_output(format!("Errors parsing {}\n", display_name).as_bytes());
                    exit_status = 255;
                }
            }
            Ok(Value::Null)
        }
        Behavior::Strip => {
            let out = goro_ext_standard::highlight::strip_source(&src);
            vm.write_output(&out);
            Ok(Value::Null)
        }
        Behavior::Highlight => {
            let out = goro_ext_standard::highlight::highlight_source(&vm, &src);
            vm.write_output(&out);
            Ok(Value::Null)
        }
    };
    let failed = finish(&mut vm, result);
    if exit_status == 0 {
        exit_status = crate::request_exit_status(&vm, failed);
    }
    Ok(exit_status)
}

fn set_response_code(vm: &mut Vm, code: i64) {
    let f = Value::String(PhpString::from_bytes(b"http_response_code"));
    let _ = vm.call_callback(&f, &[Value::Long(code)]);
}

/// php_request_shutdown(): shutdown functions, destructors, the headers
/// (sent now if nothing was output) and the remaining output.
fn finish(vm: &mut Vm, result: Result<Value, VmError>) -> bool {
    let r = vm.finish_request(result);
    let failed = r.is_err() || vm.fatal_emitted;
    crate::finish_errors(vm, r);
    let out = vm.take_output();
    if !vm.headers_sent {
        vm.mark_output_started();
    }
    stdout_write(&out);
    vm.module_shutdown();
    goro_ext_standard::request::shutdown();
    failed
}

/// main() of sapi/cgi/cgi_main.c (non-FastCGI).
pub fn cgi_main(argv: Vec<Vec<u8>>) -> i32 {
    let argv0 = argv.first().cloned().unwrap_or_else(|| b"php-cgi".to_vec());
    let argc = argv.len();
    let cgi = [
        "SERVER_SOFTWARE",
        "SERVER_NAME",
        "GATEWAY_INTERFACE",
        "REQUEST_METHOD",
    ]
    .iter()
    .any(|k| std::env::var_os(k).is_some());
    // A query string without '=' whose decoded form starts with '-' is not
    // taken as options (GHSA-3qgc-jrrr-25jv).
    let mut skip_getopt = false;
    if let Some(qs) = getenv("QUERY_STRING") {
        if !qs.contains(&b'=') {
            let d = url_decode(&qs);
            if d.iter().find(|&&c| c > b' ') == Some(&b'-') {
                skip_getopt = true;
            }
        }
    }

    let mut ini_path_override: Option<Vec<u8>> = None;
    let mut ini_ignore = false;
    let mut defines: Vec<u8> = Vec::new();
    let mut behavior = Behavior::Standard;
    if !skip_getopt {
        let mut g = GetOpt::with_options(&argv, false, CGI_OPTIONS);
        let mut optarg = None;
        loop {
            let c = g.next(&mut optarg);
            if c == -1 {
                break;
            }
            let arg = optarg.take();
            match c {
                c if c == 'c' as i32 => ini_path_override = arg,
                c if c == 'n' as i32 => ini_ignore = true,
                c if c == 'd' as i32 => {
                    if let Some(a) = arg {
                        crate::ini_define_line(&a, &mut defines);
                    }
                }
                c if c == 's' as i32 => behavior = Behavior::Highlight,
                _ => {}
            }
        }
    }

    // php_module_startup(): SAPI defaults, php.ini, then the -d entries.
    let mut ini: IniSettings = vec![
        ("html_errors".to_string(), "1".to_string()),
        ("display_errors".to_string(), "1".to_string()),
    ];
    crate::load_php_ini(
        ini_path_override.as_deref(),
        ini_ignore,
        &mut ini,
        "php-cgi-fcgi.ini",
    );
    engine::parse_ini_text(&defines, &mut ini);

    if cgi && ini_bool(&ini, "cgi.force_redirect", true) {
        let env = ini_get(&ini, "cgi.redirect_status_env")
            .filter(|e| !e.is_empty())
            .unwrap_or("REDIRECT_STATUS");
        if std::env::var_os(env).is_none() {
            stdout_write(b"<b>Security Alert!</b> The PHP CGI cannot be accessed directly.\n\n\
<p>This PHP CGI binary was compiled with force-cgi-redirect enabled.  This\n\
means that a page will only be served up if the REDIRECT_STATUS CGI variable is\n\
set, e.g. via an Apache Action directive.</p>\n\
<p>For more information as to <i>why</i> this behaviour exists, see the <a href=\"https://www.php.net/security.cgi-bin\">\
manual page for CGI security</a>.</p>\n\
<p>For more information about changing this behaviour or re-enabling this webserver,\n\
consult the installation file that came with this distribution, or visit \n\
<a href=\"https://www.php.net/install.windows\">the manual page</a>.</p>\n");
            return 255;
        }
    }

    // Second pass: -T, and the help / invalid options.
    if !skip_getopt {
        let mut g = GetOpt::with_options(&argv, true, CGI_OPTIONS);
        let mut optarg = None;
        loop {
            let c = g.next(&mut optarg);
            if c == -1 {
                break;
            }
            let _ = optarg.take();
            if c == 'h' as i32 || c == '?' as i32 || c == GETOPT_INVALID_ARG {
                stdout_write(usage(&argv0).as_bytes());
                return if c == GETOPT_INVALID_ARG { 1 } else { 0 };
            }
        }
    }

    let req = init_request_info(&ini);
    let mut no_headers = false;
    let mut no_chdir = false;
    let mut script_file: Option<Vec<u8>> = None;
    let mut optind = 1;
    if !cgi {
        let mut g = GetOpt::with_options(&argv, false, CGI_OPTIONS);
        let mut optarg = None;
        loop {
            let c = g.next(&mut optarg);
            if c == -1 {
                break;
            }
            let arg = optarg.take();
            let ch = if (0..128).contains(&c) {
                c as u8 as char
            } else {
                '\0'
            };
            match ch {
                'a' => stdout_write(b"Interactive mode enabled\n\n"),
                'C' => no_chdir = true,
                'f' => {
                    script_file = arg;
                    no_headers = true;
                }
                'i' => {
                    let mut vm = engine::new_vm(&ini, SAPI_NAME);
                    vm.cli_direct_output = true;
                    NO_HEADERS.with(|n| n.set(no_headers));
                    vm.sapi_send_headers = Some(send_headers);
                    let info = goro_ext_standard::request::RequestInfo {
                        cgi: true,
                        ..Default::default()
                    };
                    engine::request_startup(&mut vm, &info, &ini);
                    crate::run_snippet(&mut vm, "phpinfo();");
                    finish(&mut vm, Ok(Value::Null));
                    return 0;
                }
                'l' => {
                    no_headers = true;
                    behavior = Behavior::Lint;
                }
                'm' => {
                    let mut vm = engine::new_vm(&ini, SAPI_NAME);
                    let info = goro_ext_standard::request::RequestInfo::default();
                    engine::request_startup(&mut vm, &info, &ini);
                    crate::run_snippet(
                        &mut vm,
                        "$m = get_loaded_extensions(); usort($m, 'strcasecmp'); \
                         echo \"[PHP Modules]\\n\"; foreach ($m as $n) echo $n, \"\\n\"; \
                         echo \"\\n[Zend Modules]\\n\\n\";",
                    );
                    stdout_write(&vm.take_output());
                    return 0;
                }
                'q' => no_headers = true,
                'v' => {
                    let mut vm = engine::new_vm(&ini, SAPI_NAME);
                    stdout_write(&vm.take_output());
                    stdout_write(crate::version_string(SAPI_NAME).as_bytes());
                    return 0;
                }
                'w' => behavior = Behavior::Strip,
                _ => {}
            }
        }
        optind = g.optind;
    }

    let mut exit_status = 0;
    loop {
        // do_repeat:
        let mut args: Vec<Vec<u8>> = Vec::new();
        let mut path_override = script_file.clone();
        if let Some(sf) = &script_file {
            args.push(sf.clone());
            args.extend(argv[optind.min(argc)..].iter().cloned());
        } else if !cgi && argc > optind {
            path_override = Some(argv[optind].clone());
            args.extend(argv[optind..].iter().cloned());
        }
        // The remaining arguments make the query string (a&b&c).
        let mut query_string = req.query_string.clone();
        if !cgi && query_string.is_none() && argc > optind {
            let sep = ini_get(&ini, "arg_separator.input")
                .filter(|s| !s.is_empty())
                .unwrap_or("&")
                .as_bytes()
                .to_vec();
            query_string = Some(argv[optind..].join(&sep[..]));
        }
        let job = Job {
            behavior,
            cgi,
            no_headers,
            no_chdir,
            script_file: path_override,
            args: &args,
            query_string,
        };
        match run_request(&ini, &req, &job) {
            Ok(status) => {
                if exit_status == 0 {
                    exit_status = status;
                }
            }
            Err(()) => return 255,
        }
        if behavior == Behavior::Lint && argc > optind + 1 {
            optind += 1;
            script_file = None;
            continue;
        }
        return exit_status;
    }
}

/// Whether `argv0` names the CGI SAPI.
pub fn is_cgi_name(argv0: &[u8]) -> bool {
    let base = match argv0.iter().rposition(|&c| c == b'/') {
        Some(p) => &argv0[p + 1..],
        None => argv0,
    };
    base.windows(3).any(|w| w == b"cgi")
}
