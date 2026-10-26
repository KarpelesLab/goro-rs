//! The worker side of sapi/fpm/fpm/fpm_main.c: fpm_child_init(), the
//! FastCGI request loop, the SAPI callbacks (output, headers, logging) and
//! the cgi-fcgi module functions (fastcgi_finish_request(),
//! fpm_get_status(), apache_request_headers()).

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::rc::Rc;

use goro::engine::{self, IniSettings};
use goro::websapi;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use super::fastcgi::{self, FCGI_STDERR, FCGI_STDOUT, FcgiRequest};
use super::scoreboard::{self, Timeval};
use super::zlog::{self, DEBUG, ERROR, NOTICE, WARNING, ZlogStream};
use super::{Domain, Fpm};
use crate::zlog;

pub const SAPI_NAME: &str = "fpm-fcgi";

/// FPM_STDIO_CMD_FLUSH (with its terminating NUL)
const STDIO_CMD_FLUSH: &[u8; 6] = b"\0fscf\0";

/// The worker's request state (CGIG(), SG(request_info), the pool
/// settings the child keeps).
pub struct Worker {
    pub req: FcgiRequest,
    pub in_request: bool,
    // CGIG()
    rfc2616_headers: bool,
    nph: bool,
    fix_pathinfo: bool,
    discard_path: bool,
    fcgi_script_path_encoded: bool,
    fcgi_logging: bool,
    logging_request_started: bool,
    error_header: Option<Vec<u8>>,
    user_config_cache: HashMap<Vec<u8>, (i64, Vec<(String, String)>)>,
    // SG(request_info)
    path_translated: Option<Vec<u8>>,
    request_method: Option<Vec<u8>>,
    query_string: Option<Vec<u8>>,
    request_uri: Option<Vec<u8>>,
    content_type: Vec<u8>,
    content_length: i64,
    auth_vars: Vec<(Vec<u8>, Vec<u8>)>,
    auth_user: Option<Vec<u8>>,
    response_code: i64,
    no_headers: bool,
    request_body_fd: i32,
    // fpm_status
    status_uri: Option<String>,
    ping_uri: Option<String>,
    ping_response: Option<String>,
    // fpm_log
    log_format: Option<String>,
    log_fd: i32,
    log_stream: Option<ZlogStream>,
    suppress_paths: Vec<String>,
    limit_extensions: Option<Vec<String>>,
    /// The INI settings of the child (php.ini, -d, php_value[] and the
    /// PHP_VALUE / PHP_ADMIN_VALUE ones, which persist like in php-fpm).
    ini: IniSettings,
    /// php_admin_value entries: PHP_INI_SYSTEM from now on.
    admin_locked: Vec<String>,
    /// The INI directives the engine knows.
    known_ini: std::collections::HashSet<Vec<u8>>,
    slowlog: Option<String>,
    pool_name: String,
    trace_depth: i32,
}

thread_local! {
    static W: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

fn with_w<R>(f: impl FnOnce(&mut Worker) -> R) -> R {
    W.with(|w| f(w.borrow_mut().as_mut().expect("fpm worker")))
}

fn try_with_w<R>(f: impl FnOnce(&mut Worker) -> R) -> Option<R> {
    W.with(|w| match w.try_borrow_mut() {
        Ok(mut g) => g.as_mut().map(f),
        Err(_) => None,
    })
}

fn bstr(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// INI entries that only php.ini / php_admin_value can set (PHP_INI_SYSTEM):
/// .user.ini files cannot change them.
const INI_SYSTEM_ONLY: &[&str] = &[
    "allow_url_fopen",
    "allow_url_include",
    "disable_functions",
    "disable_classes",
    "doc_root",
    "enable_dl",
    "expose_php",
    "extension_dir",
    "file_uploads",
    "hard_timeout",
    "max_file_uploads",
    "realpath_cache_size",
    "realpath_cache_ttl",
    "sys_temp_dir",
    "upload_tmp_dir",
    "user_dir",
    "user_ini.cache_ttl",
    "user_ini.filename",
    "cgi.fix_pathinfo",
    "cgi.discard_path",
    "fastcgi.logging",
    "fastcgi.error_header",
    "fastcgi.script_path_encoded",
    "fpm.config",
    "zend.max_allowed_stack_size",
    "zend.reserved_stack_size",
    "opcache.enable",
];

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

/// The FastCGI logger (fcgi_log(), through zlog).
fn fcgi_logger(level: i32, msg: &str) {
    zlog::vzlog("fcgi_log", 0, level, msg, 0);
}

// ---- the request hooks (fpm_request.c, the worker side) ----

/// fpm_request_accepting()
fn request_accepting() {
    let now = scoreboard::clock_get();
    scoreboard::update_begin(std::ptr::null_mut());
    let Some(p) = scoreboard::proc_acquire(std::ptr::null_mut(), -1, false) else {
        zlog!(WARNING, "failed to acquire proc scoreboard");
        return;
    };
    unsafe {
        (*p).request_stage = scoreboard::REQUEST_ACCEPTING;
        (*p).tv = now;
    }
    scoreboard::proc_release(p);
    scoreboard::update_commit(
        1,
        -1,
        0,
        0,
        0,
        0,
        0,
        0,
        scoreboard::ACTION_INC,
        std::ptr::null_mut(),
    );
}

/// fpm_request_reading_headers()
fn request_reading_headers() {
    let now = scoreboard::clock_get();
    let now_epoch = scoreboard::epoch();
    let cpu = scoreboard::times();
    scoreboard::update_begin(std::ptr::null_mut());
    let Some(p) = scoreboard::proc_acquire(std::ptr::null_mut(), -1, false) else {
        zlog!(WARNING, "failed to acquire proc scoreboard");
        return;
    };
    unsafe {
        let p = &mut *p;
        p.request_stage = scoreboard::REQUEST_READING_HEADERS;
        p.tv = now;
        p.accepted = now;
        p.accepted_epoch = now_epoch;
        p.cpu_accepted = cpu;
        p.requests += 1;
        p.request_uri[0] = 0;
        p.request_method[0] = 0;
        p.script_filename[0] = 0;
        p.query_string[0] = 0;
        p.auth_user[0] = 0;
        p.content_length = 0;
    }
    scoreboard::proc_release(p);
    scoreboard::update_commit(
        -1,
        1,
        0,
        0,
        1,
        0,
        0,
        0,
        scoreboard::ACTION_INC,
        std::ptr::null_mut(),
    );
}

/// fpm_request_finished()
fn request_finished() {
    let now = scoreboard::clock_get();
    let Some(p) = scoreboard::proc_acquire(std::ptr::null_mut(), -1, false) else {
        zlog!(WARNING, "failed to acquire proc scoreboard");
        return;
    };
    unsafe {
        (*p).request_stage = scoreboard::REQUEST_FINISHED;
        (*p).tv = now;
    }
    scoreboard::proc_release(p);
}

/// fpm_request_executing()
fn request_executing() {
    let now = scoreboard::clock_get();
    let Some(p) = scoreboard::proc_acquire(std::ptr::null_mut(), -1, false) else {
        zlog!(WARNING, "failed to acquire proc scoreboard");
        return;
    };
    unsafe {
        (*p).request_stage = scoreboard::REQUEST_EXECUTING;
        (*p).tv = now;
    }
    scoreboard::proc_release(p);
}

/// fpm_request_end()
fn request_end() {
    let now = scoreboard::clock_get();
    let cpu = scoreboard::times();
    let memory = goro_core::value::memory_get_peak();
    let Some(p) = scoreboard::proc_acquire(std::ptr::null_mut(), -1, false) else {
        zlog!(WARNING, "failed to acquire proc scoreboard");
        return;
    };
    let mem;
    unsafe {
        let p = &mut *p;
        p.request_stage = scoreboard::REQUEST_FINISHED;
        p.tv = now;
        p.duration = now.sub(&p.accepted);
        p.cpu_duration = p.tv.sub(&p.accepted);
        p.last_request_cpu = scoreboard::Tms {
            utime: cpu.utime - p.cpu_accepted.utime,
            stime: cpu.stime - p.cpu_accepted.stime,
            cutime: cpu.cutime - p.cpu_accepted.cutime,
            cstime: cpu.cstime - p.cpu_accepted.cstime,
        };
        p.memory = memory;
        mem = p.memory;
    }
    scoreboard::proc_release(p);
    scoreboard::update(
        -1,
        -1,
        -1,
        -1,
        -1,
        -1,
        -1,
        mem,
        scoreboard::ACTION_SET,
        std::ptr::null_mut(),
    );
}

/// fpm_request_info()
fn request_info(w: &Worker) {
    let now = scoreboard::clock_get();
    let Some(p) = scoreboard::proc_acquire(std::ptr::null_mut(), -1, false) else {
        zlog!(WARNING, "failed to acquire proc scoreboard");
        return;
    };
    unsafe {
        let p = &mut *p;
        p.request_stage = scoreboard::REQUEST_INFO;
        p.tv = now;
        if let Some(u) = &w.request_uri {
            scoreboard::strlcpy(&mut p.request_uri, u);
        }
        if let Some(m) = &w.request_method {
            scoreboard::strlcpy(&mut p.request_method, m);
        }
        if let Some(q) = &w.query_string {
            scoreboard::strlcpy(&mut p.query_string, q);
        }
        if let Some(a) = &w.auth_user {
            scoreboard::strlcpy(&mut p.auth_user, a);
        }
        p.content_length = w.content_length.max(0) as usize;
        if let Some(s) = &w.path_translated {
            scoreboard::strlcpy(&mut p.script_filename, s);
        }
    }
    scoreboard::proc_release(p);
}

// ---- the SAPI callbacks ----

/// sapi_cgibin_ub_write()
fn ub_write(data: &[u8]) {
    let _ = try_with_w(|w| {
        if w.req.write(FCGI_STDOUT, data) <= 0 {
            // php_handle_aborted_connection()
        }
    });
}

/// sapi_cgibin_flush()
fn sapi_flush(vm: &mut Vm) {
    if !vm.headers_sent {
        vm.mark_output_started();
    }
    let _ = try_with_w(|w| {
        if !w.req.is_closed() {
            w.req.flush(false);
        }
    });
}

/// sapi_cgi_log_message()
fn log_message(msg: &str) {
    zlog::msg_ex(
        "sapi_cgi_log_message",
        0,
        NOTICE,
        b"PHP message: ",
        msg.as_bytes(),
    );
}

/// sapi_cgi_log_fastcgi(): the external logger of the child.
fn log_fastcgi(_level: i32, msg: &[u8]) {
    let _ = try_with_w(|w| {
        if w.fcgi_logging && w.in_request && !msg.is_empty() {
            if w.logging_request_started {
                w.req.write(FCGI_STDERR, b"; ");
            } else {
                w.logging_request_started = true;
            }
            w.req.write(FCGI_STDERR, msg);
        }
    });
}

fn atoi(b: &[u8]) -> i64 {
    super::conf::atoi(&String::from_utf8_lossy(b))
}

/// sapi_cgi_send_headers()
fn send_headers(vm: &mut Vm) {
    let (no_headers, nph, rfc2616) = with_w(|w| (w.no_headers, w.nph, w.rfc2616_headers));
    if no_headers {
        return;
    }
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
    if nph || code != 200 {
        let mut buf: Option<Vec<u8>> = None;
        let mut has_status = false;
        if rfc2616 && status_line.is_some() {
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
                    has_status = headers.iter().any(|(k, v)| is_status(k) && !v.is_empty());
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
                out.extend_from_slice(&b[..b.len().min(1023)]);
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
    ub_write(&out);
}

fn arr_value(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// sapi_add_request_header(): the request header name of a parameter.
fn request_header_name(var: &[u8]) -> Option<Vec<u8>> {
    if var.len() > 5 && var.starts_with(b"HTTP_") {
        let p = &var[5..];
        let mut out = vec![p[0]];
        let mut i = 1;
        while i < p.len() {
            let c = p[i];
            if c == b'_' {
                out.push(b'-');
                i += 1;
                if i < p.len() {
                    out.push(p[i]);
                    i += 1;
                }
            } else {
                out.push(c.to_ascii_lowercase());
                i += 1;
            }
        }
        Some(out)
    } else if var == b"CONTENT_TYPE" {
        Some(b"Content-Type".to_vec())
    } else if var == b"CONTENT_LENGTH" {
        Some(b"Content-Length".to_vec())
    } else {
        None
    }
}

/// apache_request_headers() / getallheaders()
fn fpm_request_headers(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    with_w(|w| {
        for (k, v) in w.req.env.iter() {
            if let Some(name) = request_header_name(k) {
                arr.set(
                    ArrayKey::String(PhpString::from_vec(name)),
                    Value::String(PhpString::from_bytes(v)),
                );
            }
        }
    });
    Ok(arr_value(arr))
}

/// fastcgi_finish_request()
fn fastcgi_finish_request(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    if with_w(|w| w.req.is_closed()) {
        return Ok(Value::False);
    }
    // php_output_end_all(), php_header()
    vm.ob_end_all();
    if !vm.headers_sent {
        vm.mark_output_started();
    }
    vm.flush_cli_output();
    with_w(|w| {
        w.req.end();
        w.req.close(false, false);
    });
    Ok(Value::True)
}

/// fpm_get_status()
fn fpm_get_status(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(super::status::export_to_value().unwrap_or(Value::False))
}

/// getenv(): sapi_getenv() reads the request parameters first; getenv()
/// without a name lists the environment and the parameters.
fn fpm_getenv(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args
        .first()
        .filter(|v| !matches!(v, Value::Null))
        .map(|v| v.to_php_string());
    let local_only = args.get(1).is_some_and(|v| v.to_bool());
    match name {
        None => {
            let mut arr = PhpArray::new();
            for (k, v) in std::env::vars_os() {
                arr.set(
                    ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
                    Value::String(PhpString::from_bytes(v.as_bytes())),
                );
            }
            with_w(|w| {
                for (k, v) in w.req.env.iter() {
                    arr.set(
                        ArrayKey::String(PhpString::from_bytes(k)),
                        Value::String(PhpString::from_bytes(v)),
                    );
                }
            });
            Ok(arr_value(arr))
        }
        Some(n) => {
            let nb = n.as_bytes();
            if !local_only
                && !(nb.len() <= 10 && b"HTTP_PROXY"[..nb.len()].eq_ignore_ascii_case(nb))
            {
                if let Some(v) = with_w(|w| w.req.getenv(nb)) {
                    return Ok(Value::String(PhpString::from_vec(v)));
                }
            }
            match std::env::var_os(std::ffi::OsStr::from_bytes(nb)) {
                Some(v) => Ok(Value::String(PhpString::from_bytes(v.as_bytes()))),
                None => {
                    let _ = vm;
                    Ok(Value::False)
                }
            }
        }
    }
}

fn set_response_code(vm: &mut Vm, code: i64) {
    let f = Value::String(PhpString::from_bytes(b"http_response_code"));
    let _ = vm.call_callback(&f, &[Value::Long(code)]);
}

/// is_valid_path()
fn is_valid_path(p: &[u8]) -> bool {
    !p.split(|&c| c == b'/').any(|seg| seg == b"..")
}

/// php_raw_url_decode()
fn raw_url_decode(s: &[u8]) -> Vec<u8> {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' && i + 2 < s.len() {
            if let (Some(a), Some(b)) = (hex(s[i + 1]), hex(s[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// tsrm_realpath() succeeded (the path exists).
fn realpath_exists(p: &[u8]) -> bool {
    std::fs::canonicalize(std::ffi::OsStr::from_bytes(p)).is_ok()
}

fn is_regular_file(p: &[u8]) -> bool {
    std::fs::metadata(std::ffi::OsStr::from_bytes(p))
        .map(|m| m.is_file())
        .unwrap_or(false)
}

impl Worker {
    fn getenv(&self, k: &str) -> Option<Vec<u8>> {
        self.req.getenv(k.as_bytes())
    }

    fn putenv(&mut self, k: &str, v: Option<&[u8]>) -> Option<Vec<u8>> {
        self.req.putenv(k.as_bytes(), v)
    }

    /// init_request_info()
    fn init_request_info(&mut self) {
        let mut env_script_filename = self.getenv("SCRIPT_FILENAME");
        let mut env_path_translated = self.getenv("PATH_TRANSLATED");
        let mut script_path_translated = env_script_filename.clone();
        let mut apache_was_here = false;
        if script_path_translated.is_none() && env_path_translated.is_some() {
            script_path_translated = env_path_translated.clone();
        }
        self.path_translated = None;
        self.request_method = self.getenv("REQUEST_METHOD");
        self.query_string = None;
        self.request_uri = None;
        self.content_type = Vec::new();
        self.content_length = 0;
        self.auth_vars.clear();
        self.auth_user = None;
        self.response_code = 200;

        if script_path_translated.is_some() {
            let content_length = self.getenv("CONTENT_LENGTH");
            let content_type = self.getenv("CONTENT_TYPE");
            let mut env_path_info = self.getenv("PATH_INFO");
            let mut env_script_name = self.getenv("SCRIPT_NAME");
            let mut script_name_changed = false;
            // Hack for buggy IIS that sets incorrect PATH_INFO
            let env_server_software = self.getenv("SERVER_SOFTWARE");
            if let (Some(ss), Some(sn), Some(pi)) =
                (&env_server_software, &env_script_name, &env_path_info)
            {
                if ss.starts_with(b"Microsoft-IIS") && pi.starts_with(sn) {
                    let pi = pi.clone();
                    self.putenv("ORIG_PATH_INFO", Some(&pi));
                    let rest = pi[sn.len()..].to_vec();
                    env_path_info = if rest.is_empty() {
                        self.putenv("PATH_INFO", None)
                    } else {
                        self.putenv("PATH_INFO", Some(&rest))
                    };
                }
            }
            // Fix proxy URLs in SCRIPT_FILENAME generated by Apache
            // mod_proxy_fcgi and mod_proxy_balancer
            for prefix in [&b"proxy:fcgi://"[..], b"proxy:balancer://"] {
                if let Some(sf) = env_script_filename.clone() {
                    if sf.len() >= prefix.len() && sf[..prefix.len()].eq_ignore_ascii_case(prefix) {
                        let mut p = prefix.len();
                        while p < sf.len() && sf[p] != b'/' {
                            p += 1;
                        }
                        let mut nsf = sf.clone();
                        if p < sf.len() {
                            nsf = sf[p..].to_vec();
                            apache_was_here = true;
                        }
                        if let Some(q) = nsf.iter().position(|&c| c == b'?') {
                            nsf.truncate(q);
                        }
                        // (modified in place: the parameter changes too)
                        self.putenv("SCRIPT_FILENAME", Some(&nsf));
                        env_script_filename = Some(nsf.clone());
                        script_path_translated = Some(nsf);
                    }
                }
            }
            if apache_was_here && !self.fcgi_script_path_encoded {
                if let Some(sf) = env_script_filename.clone() {
                    let d = raw_url_decode(&sf);
                    self.putenv("SCRIPT_FILENAME", Some(&d));
                    env_script_filename = Some(d.clone());
                    script_path_translated = Some(d);
                }
            }

            if self.fix_pathinfo {
                let env_redirect_url = self.getenv("REDIRECT_URL");
                let mut env_document_root = self.getenv("DOCUMENT_ROOT");
                let orig_path_translated = env_path_translated.clone();
                let orig_path_info = env_path_info.clone();
                let orig_script_name = env_script_name.clone();
                let orig_script_filename = env_script_filename.clone();
                if env_document_root.is_none() {
                    let dr = ini_get(&self.ini, "doc_root").map(|d| d.to_string());
                    if let Some(dr) = dr.filter(|d| !d.is_empty()) {
                        env_document_root = self.putenv("DOCUMENT_ROOT", Some(dr.as_bytes()));
                    }
                }
                if !apache_was_here
                    && env_path_translated.is_some()
                    && env_redirect_url.is_some()
                    && env_path_translated != script_path_translated
                {
                    script_path_translated = env_path_translated.clone();
                    env_script_name = env_redirect_url.clone();
                    script_name_changed = true;
                }
                let spt = script_path_translated.clone().unwrap_or_default();
                if !spt.is_empty() && (spt.ends_with(b"/") || !realpath_exists(&spt)) {
                    let len = spt.len();
                    let mut pt = spt.clone();
                    let mut found = false;
                    while let Some(ptr) = pt.iter().rposition(|&c| c == b'/' || c == b'\\') {
                        pt.truncate(ptr);
                        if !is_regular_file(&pt) {
                            continue;
                        }
                        found = true;
                        let ptlen = pt.len();
                        let slen = len - ptlen;
                        let pilen = env_path_info.as_ref().map_or(0, |p| p.len());
                        let path_info: Option<Vec<u8>>;
                        let tflag;
                        if apache_was_here {
                            let pi = spt[ptlen..].to_vec();
                            tflag = slen != 0 && orig_path_info.as_deref() != Some(&pi[..]);
                            path_info = Some(pi);
                        } else {
                            path_info = match &env_path_info {
                                Some(epi) if pilen > slen => Some(epi[pilen - slen..].to_vec()),
                                _ => None,
                            };
                            tflag = path_info.is_some();
                        }
                        if tflag {
                            let path_info = path_info.clone().unwrap_or_default();
                            let mut decoded: Option<Vec<u8>> = None;
                            if let Some(opi) = &orig_path_info {
                                self.putenv("ORIG_PATH_INFO", Some(opi));
                                let epi = env_path_info.clone().unwrap_or_default();
                                let cut: Vec<u8> = if apache_was_here {
                                    epi.clone()
                                } else {
                                    epi[..pilen - slen].to_vec()
                                };
                                if orig_script_name.as_deref() != Some(&cut[..]) {
                                    if let Some(osn) = &orig_script_name {
                                        self.putenv("ORIG_SCRIPT_NAME", Some(osn));
                                    }
                                    self.request_uri = self.putenv("SCRIPT_NAME", Some(&cut));
                                } else {
                                    self.request_uri = orig_script_name.clone();
                                }
                            } else if apache_was_here && env_script_name.is_some() {
                                if self.fcgi_script_path_encoded && path_info.contains(&b'%') {
                                    decoded = Some(raw_url_decode(&path_info));
                                }
                                let sn = env_script_name.clone().unwrap();
                                let snlen = sn.len();
                                let mut start = None;
                                if snlen > slen && sn[snlen - slen..] == path_info[..] {
                                    start = Some(snlen - slen);
                                } else if let Some(d) = &decoded {
                                    if snlen > d.len() && sn[snlen - d.len()..] == d[..] {
                                        start = Some(snlen - d.len());
                                    }
                                }
                                if let Some(st) = start {
                                    if let Some(osn) = &orig_script_name {
                                        self.putenv("ORIG_SCRIPT_NAME", Some(osn));
                                    }
                                    let nsn = sn[..st].to_vec();
                                    env_script_name = Some(nsn.clone());
                                    self.request_uri = self.putenv("SCRIPT_NAME", Some(&nsn));
                                }
                            }
                            env_path_info = match decoded {
                                Some(d) => self.putenv("PATH_INFO", Some(&d)),
                                None => self.putenv("PATH_INFO", Some(&path_info)),
                            };
                        }
                        if orig_script_filename.as_deref() != Some(&pt[..]) {
                            if let Some(osf) = &orig_script_filename {
                                self.putenv("ORIG_SCRIPT_FILENAME", Some(osf));
                            }
                            script_path_translated = self.putenv("SCRIPT_FILENAME", Some(&pt));
                        }
                        // figure out docroot: SCRIPT_FILENAME minus SCRIPT_NAME
                        if let Some(dr) = &env_document_root {
                            let mut d = dr.clone();
                            if d.ends_with(b"/") {
                                d.pop();
                            }
                            if let Some(pi) = &env_path_info {
                                d.extend_from_slice(pi);
                            }
                            if let Some(opt) = &orig_path_translated {
                                self.putenv("ORIG_PATH_TRANSLATED", Some(opt));
                            }
                            env_path_translated = self.putenv("PATH_TRANSLATED", Some(&d));
                        } else if let Some(sn) =
                            env_script_name.clone().filter(|sn| find(&pt, sn).is_some())
                        {
                            let ptlen = pt.len() - sn.len().min(pt.len());
                            let mut d = pt[..ptlen].to_vec();
                            if let Some(pi) = &env_path_info {
                                d.extend_from_slice(pi);
                            }
                            if let Some(opt) = &orig_path_translated {
                                self.putenv("ORIG_PATH_TRANSLATED", Some(opt));
                            }
                            env_path_translated = self.putenv("PATH_TRANSLATED", Some(&d));
                        }
                        break;
                    }
                    if !found {
                        if let Some(osf) = &orig_script_filename {
                            self.putenv("ORIG_SCRIPT_FILENAME", Some(osf));
                        }
                        script_path_translated = self.putenv("SCRIPT_FILENAME", None);
                        self.response_code = 404;
                    }
                    if self.request_uri.is_none() {
                        if orig_script_name.is_none() || orig_script_name != env_script_name {
                            if let Some(osn) = &orig_script_name {
                                self.putenv("ORIG_SCRIPT_NAME", Some(osn));
                            }
                            self.request_uri = match &env_script_name {
                                Some(sn) => {
                                    let sn = sn.clone();
                                    self.putenv("SCRIPT_NAME", Some(&sn))
                                }
                                None => self.putenv("SCRIPT_NAME", None),
                            };
                        } else {
                            self.request_uri = orig_script_name.clone();
                        }
                    }
                } else {
                    if orig_script_filename.is_none()
                        || orig_script_filename != script_path_translated
                    {
                        if let Some(osf) = &orig_script_filename {
                            self.putenv("ORIG_SCRIPT_FILENAME", Some(osf));
                        }
                        let s = script_path_translated.clone().unwrap_or_default();
                        script_path_translated = self.putenv("SCRIPT_FILENAME", Some(&s));
                    }
                    if !apache_was_here && env_redirect_url.is_some() {
                        if let Some(opi) = &orig_path_info {
                            self.putenv("ORIG_PATH_INFO", Some(opi));
                            self.putenv("PATH_INFO", None);
                        }
                        if let Some(opt) = &orig_path_translated {
                            self.putenv("ORIG_PATH_TRANSLATED", Some(opt));
                            self.putenv("PATH_TRANSLATED", None);
                        }
                    }
                    if script_name_changed {
                        if let Some(osn) = &orig_script_name {
                            self.putenv("ORIG_SCRIPT_NAME", Some(osn));
                        }
                        let sn = env_script_name.clone();
                        self.request_uri = self.putenv("SCRIPT_NAME", sn.as_deref());
                    } else {
                        self.request_uri = env_script_name.clone();
                    }
                }
                let _ = env_path_translated;
            } else {
                // pre 4.3 behaviour, shouldn't be used but provides BC
                self.request_uri = env_path_info.clone().or(env_script_name.clone());
                if !self.discard_path && env_path_translated.is_some() {
                    script_path_translated = env_path_translated.clone();
                }
            }
            if let Some(spt) = script_path_translated.filter(|p| is_valid_path(p)) {
                self.path_translated = Some(spt);
            }
            self.query_string = self.getenv("QUERY_STRING");
            self.content_type = content_type.unwrap_or_default();
            self.content_length = content_length.map(|c| atoi(&c)).unwrap_or(0);
            // php_handle_auth_data()
            if let Some(auth) = self.getenv("HTTP_AUTHORIZATION") {
                let mut v = Vec::new();
                websapi::auth_vars(&auth, &mut v);
                self.auth_user = v
                    .iter()
                    .find(|(k, _)| k == b"PHP_AUTH_USER")
                    .map(|(_, v)| v.clone());
                self.auth_vars = v;
            }
        }
    }
}

/// fastcgi_ini_parser() over PHP_VALUE / PHP_ADMIN_VALUE.
fn fastcgi_ini_values(w: &mut Worker, ini: &[u8], admin: bool) {
    let mut data = ini.to_vec();
    data.push(b'\n');
    let mut vm = super::scratch_vm();
    let events = match goro_ext_standard::ini_parser::parse_ini_events(
        &mut vm,
        &data,
        goro_ext_standard::ini_parser::SCANNER_NORMAL,
        "Unknown",
    ) {
        Ok(e) | Err(e) => e,
    };
    let _ = vm.take_output();
    use goro_ext_standard::ini_parser::IniEvent;
    for ev in events {
        match ev {
            IniEvent::Entry(k, v) => {
                let key = bstr(&k);
                if key.is_empty() {
                    zlog!(ERROR, "Passing INI directive through FastCGI: empty key");
                    continue;
                }
                if apply_define(w, &key, &bstr(&v), admin) < 0 {
                    zlog!(
                        ERROR,
                        "Passing INI directive through FastCGI: unable to set '{}'",
                        key
                    );
                }
            }
            _ => {
                zlog!(
                    ERROR,
                    "Passing INI directive through FastCGI: only classic entries are allowed"
                );
            }
        }
    }
}

/// on_modify of an INI entry rejecting the value (fpm_php_zend_ini_alter_master()
/// keeps the old value then).
fn ini_value_rejected(key: &str, value: &str) -> bool {
    match key {
        "precision" | "serialize_precision" => value.trim().parse::<i64>().is_ok_and(|n| n < -1),
        _ => false,
    }
}

/// zend_ini_parse_bool()
fn truthy(v: &str) -> bool {
    let v = v.trim().to_ascii_lowercase();
    matches!(v.as_str(), "1" | "on" | "yes" | "true") || v.parse::<i64>().is_ok_and(|n| n != 0)
}

/// fpm_php_apply_defines_ex(): -1 = unknown entry, 1 = applied,
/// 2 = extension loaded, 0 = extension failed.
fn apply_define(w: &mut Worker, key: &str, value: &str, admin: bool) -> i32 {
    if key == "extension" && !value.is_empty() {
        w.ini.push((key.to_string(), value.to_string()));
        return 2;
    }
    if !w.known_ini.contains(key.as_bytes()) {
        return -1;
    }
    if w.admin_locked.iter().any(|k| k == key) && !admin {
        // modifiable is PHP_INI_SYSTEM: a user value cannot change it
        return 1;
    }
    if ini_value_rejected(key, value) {
        return 1;
    }
    // OnEnable: OPcache may only be disabled once started
    if key == "opcache.enable"
        && truthy(value)
        && ini_get(&w.ini, key).is_some_and(|cur| !truthy(cur))
    {
        goro_core::script_cache::log(
            goro_core::script_cache::LOG_WARNING,
            "Zend OPcache can't be temporarily enabled. Are you using php_admin_value[opcache.enable]=1 in an individual pool's configuration?",
        );
        return 1;
    }
    w.ini.push((key.to_string(), value.to_string()));
    if admin && !w.admin_locked.iter().any(|k| k == key) {
        w.admin_locked.push(key.to_string());
    }
    1
}

/// php_parse_user_ini_file()
fn parse_user_ini_file(dir: &[u8], name: &str, out: &mut Vec<(String, String)>) {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name.as_bytes());
    let Ok(md) = std::fs::metadata(std::ffi::OsStr::from_bytes(&p)) else {
        return;
    };
    if !md.is_file() {
        return;
    }
    let Ok(data) = std::fs::read(std::ffi::OsStr::from_bytes(&p)) else {
        return;
    };
    let mut vm = super::scratch_vm();
    let events = match goro_ext_standard::ini_parser::parse_ini_events(
        &mut vm,
        &data,
        goro_ext_standard::ini_parser::SCANNER_NORMAL,
        &bstr(&p),
    ) {
        Ok(e) | Err(e) => e,
    };
    let _ = vm.take_output();
    for ev in events {
        if let goro_ext_standard::ini_parser::IniEvent::Entry(k, v) = ev {
            let k = bstr(&k);
            out.retain(|(ek, _)| *ek != k);
            out.push((k, bstr(&v)));
        }
    }
}

/// php_cgi_ini_activate_user_config(): the .user.ini entries in effect.
fn user_config(w: &mut Worker, path: &[u8], doc_root: &[u8]) -> Vec<(String, String)> {
    let request_time = scoreboard::epoch();
    let filename = ini_get(&w.ini, "user_ini.filename")
        .unwrap_or(".user.ini")
        .to_string();
    let ttl = ini_get(&w.ini, "user_ini.cache_ttl")
        .map(super::conf::atoi)
        .unwrap_or(300);
    let key = path.to_vec();
    if let Some((expires, cfg)) = w.user_config_cache.get(&key) {
        if request_time <= *expires {
            return cfg.clone();
        }
    }
    w.user_config_cache.insert(key.clone(), (0, Vec::new()));
    let mut path = path.to_vec();
    if !path.starts_with(b"/") {
        match std::fs::canonicalize(std::ffi::OsStr::from_bytes(&path)) {
            Ok(p) => path = p.as_os_str().as_bytes().to_vec(),
            Err(_) => return Vec::new(),
        }
    }
    let (s1, s2, s_len) = if path.len() > doc_root.len() {
        (doc_root.to_vec(), path.clone(), doc_root.len())
    } else {
        (path.clone(), doc_root.to_vec(), path.len())
    };
    let mut cfg = Vec::new();
    if s1.get(..s_len) == s2.get(..s_len) {
        // the tree from DOCUMENT_ROOT down to the script's directory
        let mut pos = doc_root.len();
        while let Some(off) = s2
            .get(pos..)
            .and_then(|r| r.iter().position(|&c| c == b'/'))
        {
            let cut = pos + off;
            parse_user_ini_file(&s2[..cut], &filename, &mut cfg);
            pos = cut + 1;
        }
    } else {
        parse_user_ini_file(&path, &filename, &mut cfg);
    }
    w.user_config_cache
        .insert(key, (request_time + ttl, cfg.clone()));
    cfg
}

/// sapi_cgi_pre_request_init(): the per-directory configuration (.user.ini).
fn pre_request_init(w: &mut Worker) -> Vec<(String, String)> {
    let Some(pt) = w.path_translated.clone() else {
        return Vec::new();
    };
    let fname = ini_get(&w.ini, "user_ini.filename")
        .unwrap_or(".user.ini")
        .to_string();
    if fname.is_empty() {
        return Vec::new();
    }
    // the directory, with a trailing slash
    let mut path = pt.clone();
    if !path.ends_with(b"/") {
        match path.iter().rposition(|&c| c == b'/') {
            Some(p) => path.truncate(p),
            None => path = b".".to_vec(),
        }
        path.push(b'/');
    } else {
        while path.len() > 1 && path.ends_with(b"/") {
            path.pop();
        }
        match path.iter().rposition(|&c| c == b'/') {
            Some(p) => path.truncate(p),
            None => path = b".".to_vec(),
        }
        path.push(b'/');
    }
    let Some(mut doc_root) = w.getenv("DOCUMENT_ROOT") else {
        return Vec::new();
    };
    if !doc_root.is_empty() && doc_root.ends_with(b"/") {
        doc_root.pop();
    }
    user_config(w, &path, &doc_root)
}

/// fpm_php_limit_extensions()
fn limit_extensions(w: &Worker, path: &[u8]) -> bool {
    let Some(exts) = &w.limit_extensions else {
        return false;
    };
    for e in exts {
        if path.len() > e.len() && path.ends_with(e.as_bytes()) {
            return false;
        }
    }
    zlog!(
        NOTICE,
        "Access to the script '{}' has been denied (see security.limit_extensions)",
        bstr(path)
    );
    true
}

/// The child's SIGURG handler: the master asks for a slowlog backtrace.
extern "C" fn sig_slowlog(_signo: libc::c_int) {
    SLOWLOG_PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
    goro_core::value::TRANSPARENT_INTERRUPT.store(true, std::sync::atomic::Ordering::Relaxed);
    goro_core::value::VM_INTERRUPT.store(true, std::sync::atomic::Ordering::Relaxed);
}

static SLOWLOG_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// fpm_php_trace_dump(), from inside the engine: the slowlog entry of the
/// running request.
fn slowlog_hook(vm: &mut Vm) {
    if !SLOWLOG_PENDING.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    goro_core::value::TRANSPARENT_INTERRUPT.store(false, std::sync::atomic::Ordering::Relaxed);
    let Some((path, pool, depth, script)) = try_with_w(|w| {
        (
            w.slowlog.clone(),
            w.pool_name.clone(),
            w.trace_depth,
            w.path_translated.clone().unwrap_or_default(),
        )
    }) else {
        return;
    };
    let Some(path) = path else {
        return;
    };
    let mut out = format!(
        "\n{} [pool {}] pid {}\nscript_filename = {}\n",
        zlog::print_time(zlog::now_tv()),
        pool,
        std::process::id(),
        bstr(&script)
    );
    let trace = vm.build_exception_trace();
    let mut n = 0;
    if let Value::Array(frames) = trace {
        for (_, f) in frames.borrow().iter() {
            if let Value::Array(fa) = f {
                let fa = fa.borrow();
                let get = |k: &[u8]| {
                    fa.get(&ArrayKey::String(PhpString::from_bytes(k)))
                        .map(|v| v.to_php_string().to_string_lossy())
                };
                let func = get(b"function").unwrap_or_else(|| "???".into());
                let func = match get(b"class") {
                    Some(c) => format!(
                        "{}{}{}",
                        c,
                        get(b"type").unwrap_or_else(|| "::".into()),
                        func
                    ),
                    None => func,
                };
                let file = get(b"file").unwrap_or_else(|| "unknown".into());
                let line = get(b"line").unwrap_or_else(|| "0".into());
                out.push_str(&format!(
                    "[0x{:016x}] {}() {}:{}\n",
                    0x7f00_0000_0000u64 + n as u64 * 0x60,
                    func,
                    file,
                    line
                ));
                n += 1;
                if n == depth {
                    break;
                }
            }
        }
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
    {
        let _ = f.write_all(out.as_bytes());
    }
}

/// The engine's INI defaults of the FPM SAPI (cgi_sapi_module and the
/// cgi-fcgi module's own entries).
pub fn sapi_ini_defaults() -> IniSettings {
    vec![
        ("html_errors".to_string(), "1".to_string()),
        ("display_errors".to_string(), "1".to_string()),
        ("cgi.rfc2616_headers".to_string(), "0".to_string()),
        ("cgi.nph".to_string(), "0".to_string()),
        ("cgi.fix_pathinfo".to_string(), "1".to_string()),
        ("cgi.discard_path".to_string(), "0".to_string()),
        ("fastcgi.script_path_encoded".to_string(), "1".to_string()),
        ("fastcgi.logging".to_string(), "1".to_string()),
        ("fastcgi.error_header".to_string(), String::new()),
        ("fpm.config".to_string(), String::new()),
    ]
}

/// fpm_child_init() + the worker part of main(): never returns.
pub fn child_main(fpm: &mut Fpm, wp: usize) -> ! {
    let pool = fpm.pools[wp].config.clone();
    let name = pool.name().to_string();
    let listening = unsafe { libc::dup(fpm.pools[wp].listening_socket) };
    fastcgi::LISTEN_FD.store(listening, std::sync::atomic::Ordering::Relaxed);
    let max_requests = pool.pm_max_requests;

    // fpm_stdio_init_child()
    if fpm.error_log_fd == zlog::SYSLOG_FD {
        unsafe { libc::closelog() };
    } else if fpm.error_log_fd > 0 {
        unsafe { libc::close(fpm.error_log_fd) };
    }
    fpm.error_log_fd = -1;
    zlog::set_fd(-1, false);

    // fpm_log_init_child()
    let mut log_format = None;
    if pool.access_log.as_deref().is_some_and(|s| !s.is_empty()) {
        log_format = pool.access_format.clone();
    }
    let log_fd = fpm.pools[wp].log_fd;
    let mut log_stream = ZlogStream::init_ex(zlog::ACCESS_LOG, log_fd);
    log_stream.set_wrapping(false);
    for (i, p) in fpm.pools.iter_mut().enumerate() {
        if i != wp && p.log_fd > -1 && p.log_fd != log_fd {
            unsafe { libc::close(p.log_fd) };
            p.log_fd = -1;
        }
    }

    // fpm_status_init_child()
    let status_uri = pool.pm_status_path.clone();
    let ping_uri = pool.ping_path.clone();
    let ping_response = pool.ping_response.clone();
    if ping_uri.is_some() && ping_response.is_none() {
        zlog!(
            ERROR,
            "[pool {}] ping is set ({}) but ping.response is not set.",
            name,
            ping_uri.as_deref().unwrap_or("")
        );
        zlog!(ERROR, "[pool {}] child failed to initialize", name);
        std::process::exit(70);
    }

    let mut ok = fpm.unix_init_child(wp) >= 0;
    ok = ok && fpm.signals_init_child() >= 0;
    ok = ok && fpm.env_init_child(wp) >= 0;
    if !ok {
        zlog!(ERROR, "[pool {}] child failed to initialize", name);
        std::process::exit(70);
    }
    // the slowlog backtrace request
    unsafe {
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = sig_slowlog as extern "C" fn(libc::c_int) as libc::sighandler_t;
        act.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut act.sa_mask);
        libc::sigaction(libc::SIGURG, &act, std::ptr::null_mut());
    }

    // fpm_php_init_child(): php_value[] / php_admin_value[]
    let mut ini = sapi_ini_defaults();
    ini.extend(fpm.php_ini.iter().cloned());
    let probe = engine::new_vm(&ini, SAPI_NAME);
    let mut known: std::collections::HashSet<Vec<u8>> = probe
        .constants
        .ini()
        .keys()
        .chain(probe.constants.consts().keys())
        .cloned()
        .collect();
    for k in [
        "cgi.rfc2616_headers",
        "cgi.nph",
        "cgi.fix_pathinfo",
        "cgi.discard_path",
        "fastcgi.script_path_encoded",
        "fastcgi.logging",
        "fastcgi.error_header",
        "fpm.config",
    ] {
        known.insert(k.as_bytes().to_vec());
    }
    drop(probe);
    let mut w = Worker {
        req: FcgiRequest::new(
            listening,
            Some(request_accepting),
            Some(request_reading_headers),
            Some(request_finished),
        ),
        in_request: false,
        rfc2616_headers: false,
        nph: false,
        fix_pathinfo: true,
        discard_path: false,
        fcgi_script_path_encoded: true,
        fcgi_logging: true,
        logging_request_started: false,
        error_header: None,
        user_config_cache: HashMap::new(),
        path_translated: None,
        request_method: None,
        query_string: None,
        request_uri: None,
        content_type: Vec::new(),
        content_length: 0,
        auth_vars: Vec::new(),
        auth_user: None,
        response_code: 200,
        no_headers: false,
        request_body_fd: -1,
        status_uri,
        ping_uri,
        ping_response,
        log_format,
        log_fd,
        log_stream: Some(log_stream),
        suppress_paths: pool.access_suppress_paths.clone(),
        limit_extensions: fpm.pools[wp].limit_extensions.clone(),
        ini,
        admin_locked: Vec::new(),
        known_ini: known,
        slowlog: pool.slowlog.clone(),
        pool_name: name.clone(),
        trace_depth: pool.request_slowlog_trace_depth,
    };
    for (k, v) in &pool.php_values {
        let r = apply_define(&mut w, k, v, false);
        if r < 0 {
            zlog!(ERROR, "Unable to set php_value '{}'", k);
        }
    }
    for (k, v) in &pool.php_admin_values {
        let r = apply_define(&mut w, k, v, true);
        if r < 0 {
            zlog!(ERROR, "Unable to set php_admin_value '{}'", k);
        }
    }
    if fpm.pools[wp].listen_address_domain == Domain::Inet {
        fastcgi::set_logger(fcgi_logger);
        fastcgi::set_allowed_clients(pool.listen_allowed_clients.as_deref());
    }
    fastcgi::set_logger(fcgi_logger);
    // fpm_child_cloexec()
    unsafe {
        let attrs = libc::fcntl(listening, libc::F_GETFD);
        if attrs < 0 {
            zlog!(
                WARNING,
                "failed to get attributes of listening socket, errno: {}",
                zlog::errno()
            );
        } else if libc::fcntl(listening, libc::F_SETFD, attrs | libc::FD_CLOEXEC) < 0 {
            zlog!(WARNING, "failed to change attribute of listening socket");
        }
    }
    // fpm_cleanups_run(FPM_CLEANUP_CHILD): the master's descriptors
    fpm.child_cleanup();

    zlog::set_external_logger(Some(log_fastcgi));
    W.with(|c| *c.borrow_mut() = Some(w));
    let mut requests = 0;
    loop {
        if with_w(|w| w.req.accept_request()) < 0 {
            break;
        }
        handle_request();
        requests += 1;
        if max_requests != 0 && requests == max_requests {
            with_w(|w| {
                w.req.keep = false;
                w.req.finish_request(false);
            });
            break;
        }
    }
    std::process::exit(0);
}

fn new_request_vm(ini: &IniSettings, admin_locked: &[String]) -> Vm {
    let mut vm = engine::new_vm(ini, SAPI_NAME);
    // the cgi-fcgi module of the SAPI
    vm.register_extension(b"cgi-fcgi");
    vm.register_function(b"apache_request_headers", fpm_request_headers);
    vm.register_function(b"getallheaders", fpm_request_headers);
    vm.register_function(b"fastcgi_finish_request", fastcgi_finish_request);
    vm.register_function(b"fpm_get_status", fpm_get_status);
    vm.register_function(b"getenv", fpm_getenv);
    vm.cli_direct_output = true;
    vm.cli_write_through = true;
    vm.sapi_send_headers = Some(send_headers);
    vm.sapi_ub_write = Some(ub_write);
    vm.sapi_flush = Some(sapi_flush);
    vm.sapi_log_message = Some(log_message);
    vm.interrupt_hook = Some(slowlog_hook);
    vm.sapi_script_done = Some(script_done);
    fn locked(_vm: &mut Vm, _k: &[u8], _v: &Value) -> bool {
        false
    }
    for k in admin_locked {
        vm.ini_set_hooks.insert(k.as_bytes().to_vec(), locked);
    }
    vm
}

/// Read the request body (sapi_cgi_read_post()).
fn read_body(w: &mut Worker) -> Vec<u8> {
    let mut body = Vec::new();
    let want = w.content_length.max(0) as usize;
    if want == 0 {
        return body;
    }
    if let Some(f) = w.getenv("REQUEST_BODY_FILE").filter(|f| !f.is_empty()) {
        match std::fs::read(std::ffi::OsStr::from_bytes(&f)) {
            Ok(mut d) => {
                d.truncate(want);
                return d;
            }
            Err(_) => return body,
        }
    }
    let mut buf = vec![0u8; 16384];
    while body.len() < want {
        let n = (want - body.len()).min(buf.len());
        let r = w.req.read(&mut buf[..n]);
        if r <= 0 {
            break;
        }
        body.extend_from_slice(&buf[..r as usize]);
    }
    body
}

/// One FastCGI request: fpm_main()'s loop body.
fn handle_request() {
    with_w(|w| {
        w.request_body_fd = -1;
        w.in_request = true;
        w.logging_request_started = false;
        // INI entries of the cgi-fcgi module
        w.rfc2616_headers = ini_bool(&w.ini, "cgi.rfc2616_headers", false);
        w.nph = ini_bool(&w.ini, "cgi.nph", false);
        w.fix_pathinfo = ini_bool(&w.ini, "cgi.fix_pathinfo", true);
        w.discard_path = ini_bool(&w.ini, "cgi.discard_path", false);
        w.fcgi_script_path_encoded = ini_bool(&w.ini, "fastcgi.script_path_encoded", true);
        w.fcgi_logging = ini_bool(&w.ini, "fastcgi.logging", true);
        w.error_header = ini_get(&w.ini, "fastcgi.error_header")
            .filter(|s| !s.is_empty())
            .map(|s| s.as_bytes().to_vec());
        w.init_request_info();
        // PHP_VALUE / PHP_ADMIN_VALUE
        if let Some(v) = w.getenv("PHP_VALUE") {
            fastcgi_ini_values(w, &v, false);
        }
        if let Some(v) = w.getenv("PHP_ADMIN_VALUE") {
            fastcgi_ini_values(w, &v, true);
        }
        request_info(w);
    });

    // php_request_startup()
    let (ini, locked) = with_w(|w| {
        let user = pre_request_init(w);
        let mut ini = w.ini.clone();
        for (k, v) in user {
            if w.admin_locked.contains(&k) || INI_SYSTEM_ONLY.contains(&k.as_str()) {
                continue;
            }
            if !w.known_ini.contains(k.as_bytes()) {
                continue;
            }
            ini.push((k, v));
        }
        (ini, w.admin_locked.clone())
    });
    let mut vm = new_request_vm(&ini, &locked);
    let _ = goro_ext_standard::misc::take_sent_headers();
    let _ = goro_ext_standard::misc::take_status_line();
    let _ = goro_ext_standard::misc::take_response_code();
    goro_core::sapi_headers::reset();

    let (info, response_code, path_translated, has_method) = with_w(|w| {
        let body = read_body(w);
        let mut info = goro_ext_standard::request::RequestInfo {
            cgi: true,
            ..Default::default()
        };
        info.request_method = w.request_method.clone();
        info.query_string = w.query_string.clone().unwrap_or_default();
        info.cookie_data = w.getenv("HTTP_COOKIE");
        info.content_type = Some(w.content_type.clone()).filter(|c| !c.is_empty());
        info.content_length = w.content_length;
        info.body = body;
        // php_import_environment_variables() + fcgi_loadenv()
        let mut env: Vec<(Vec<u8>, Vec<u8>)> = std::env::vars_os()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect();
        for (k, v) in w.req.env.iter() {
            env.push((k.to_vec(), v.to_vec()));
        }
        let mut server = env.clone();
        // sapi_cgi_register_variables(): PHP_SELF
        let mut php_self = w.request_uri.clone().unwrap_or_default();
        if w.fix_pathinfo {
            if let Some(pi) = w.getenv("PATH_INFO") {
                php_self.extend_from_slice(&pi);
            }
        }
        server.push((b"PHP_SELF".to_vec(), php_self));
        server.extend(w.auth_vars.iter().cloned());
        // check_http_proxy()
        let local_proxy = std::env::var_os("HTTP_PROXY").map(|v| v.as_bytes().to_vec());
        for list in [&mut server, &mut env] {
            if list.iter().any(|(k, _)| k == b"HTTP_PROXY") {
                match &local_proxy {
                    None => list.retain(|(k, _)| k != b"HTTP_PROXY"),
                    Some(p) => {
                        for e in list.iter_mut().filter(|(k, _)| k == b"HTTP_PROXY") {
                            e.1 = p.clone();
                        }
                    }
                }
            }
        }
        info.env = env;
        info.server_vars = Some(server);
        info.script_filename = w.path_translated.clone().unwrap_or_default();
        (
            info,
            w.response_code,
            w.path_translated.clone(),
            w.request_method.is_some(),
        )
    });
    goro_ext_standard::misc::set_translated_path(path_translated.as_ref().map(|p| bstr(p)));
    engine::request_startup(&mut vm, &info, &ini);
    if response_code != 200 {
        set_response_code(&mut vm, response_code);
    }

    let mut result: Result<Value, VmError> = Ok(Value::Null);
    let mut executed = false;
    'done: {
        if !has_method {
            break 'done;
        }
        if status_handle_request(&mut vm) {
            break 'done;
        }
        let Some(pt) = path_translated.clone() else {
            zlog!(DEBUG, "Primary script unknown");
            set_response_code(&mut vm, 404);
            vm.write_output(b"File not found.\n");
            break 'done;
        };
        if with_w(|w| limit_extensions(w, &pt)) {
            set_response_code(&mut vm, 403);
            vm.write_output(b"Access denied.\n");
            break 'done;
        }
        // php_fopen_primary_script()
        let os = std::ffi::OsStr::from_bytes(&pt);
        let read = if std::path::Path::new(os).is_dir() {
            Err(std::io::Error::from_raw_os_error(libc::EISDIR))
        } else {
            std::fs::read(os)
        };
        match read {
            Err(e) => {
                let errno = e.raw_os_error().unwrap_or(libc::ENOENT);
                zlog!(
                    ERROR,
                    "Unable to open primary script: {} ({})",
                    bstr(&pt),
                    zlog::strerror(errno)
                );
                if errno == libc::EACCES {
                    set_response_code(&mut vm, 403);
                    vm.write_output(b"Access denied.\n");
                } else {
                    set_response_code(&mut vm, 404);
                    vm.write_output(b"No input file specified.\n");
                }
            }
            Ok(src) => {
                request_executing();
                executed = true;
                let filename = std::fs::canonicalize(os)
                    .map(|c| c.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| bstr(&pt));
                vm.current_file = filename.as_str().into();
                // php_execute_script(): from the script's directory
                if let Some(dir) = std::path::Path::new(&filename).parent() {
                    let _ = std::env::set_current_dir(dir);
                }
                // (opcache: the script's compilation from an earlier
                // request of this worker)
                result = engine::run_main_script(&mut vm, &src, &filename);
            }
        }
    }
    let _ = executed;
    // php_request_shutdown()
    let r = vm.finish_request(result);
    if let Err(e) = r {
        if vm.current_exception.is_some() || !vm.fatal_emitted {
            vm.report_uncaught(&e);
        }
        vm.run_shutdown_destructors();
    }
    let out = vm.take_output();
    if !vm.headers_sent {
        vm.mark_output_started();
    }
    ub_write(&out);
    vm.module_shutdown();
    goro_ext_standard::request::shutdown();
    // (the engine's class and function tables are released once the
    // request ended)
    let tables = vm.retire_tables();
    drop(vm);
    // sapi_cgi_deactivate()
    with_w(|w| {
        if !w.req.finish_request(false) {
            // php_handle_aborted_connection()
        }
    });
    drop(tables);
    // fpm_stdio_flush_child()
    unsafe {
        libc::write(
            2,
            STDIO_CMD_FLUSH.as_ptr() as *const libc::c_void,
            STDIO_CMD_FLUSH.len(),
        );
    }
}

/// What fpm_main() does after php_execute_script(): fastcgi.error_header,
/// fpm_request_end() and the access log (run by finish_request() before the
/// shutdown functions).
fn script_done(vm: &mut Vm, failed: bool) {
    let exit_status = if vm.exit_requested {
        vm.exit_code
    } else if failed || vm.fatal_emitted {
        255
    } else {
        0
    };
    if exit_status == 255 && !vm.headers_sent {
        if let Some(eh) = with_w(|w| w.error_header.clone()) {
            goro_ext_standard::misc::set_status_line(&eh);
        }
    }
    request_end();
    // fpm_log_write(NULL)
    let code = goro_ext_standard::misc::peek_response_code();
    let headers = goro_ext_standard::misc::peek_sent_headers();
    with_w(|w| {
        if let (Some(fmt), true) = (w.log_format.clone(), w.log_fd != -1) {
            let mut stream = w.log_stream.take().unwrap();
            let env = w.req.env.clone();
            let getenv = move |k: &[u8]| env.get(k).map(|v| v.to_vec());
            let lr = super::log::LogRequest {
                response_code: code,
                content_length: w.content_length,
                headers: &headers,
                getenv: &getenv,
            };
            let _ = super::log::write_request(&fmt, &mut stream, &lr, &w.suppress_paths);
            w.log_stream = Some(stream);
        }
        w.path_translated = None;
    });
}

/// fpm_status_handle_request()
fn status_handle_request(vm: &mut Vm) -> bool {
    let (uri, status_uri, ping_uri, ping_response, method) = with_w(|w| {
        (
            w.request_uri.clone(),
            w.status_uri.clone(),
            w.ping_uri.clone(),
            w.ping_response.clone(),
            w.request_method.clone(),
        )
    });
    let Some(uri) = uri else {
        return false;
    };
    let headers_only = method.as_deref() == Some(b"HEAD");
    let common = |ct: Option<&str>| {
        if let Some(ct) = ct {
            goro_core::sapi_headers::add_header(format!("Content-Type: {}", ct).as_bytes(), true);
        }
        goro_ext_standard::misc::sapi_add_header(b"Expires: Thu, 01 Jan 1970 00:00:00 GMT", true);
        goro_ext_standard::misc::sapi_add_header(
            b"Cache-Control: no-cache, no-store, must-revalidate, max-age=0",
            true,
        );
    };
    if let (Some(p), Some(resp)) = (&ping_uri, &ping_response) {
        if p.as_bytes() == &uri[..] {
            request_executing();
            common(Some("text/plain"));
            set_response_code(vm, 200);
            if !headers_only {
                vm.write_output(resp.as_bytes());
            }
            return true;
        }
    }
    if let Some(s) = &status_uri {
        if s.as_bytes() == &uri[..] {
            request_executing();
            let get_has = |vm: &Vm, key: &[u8]| -> bool {
                match vm.get_global(b"_GET") {
                    Some(Value::Array(a)) => a
                        .borrow()
                        .iter()
                        .any(|(k, _)| matches!(k, ArrayKey::String(s) if s.as_bytes() == key)),
                    _ => false,
                }
            };
            let full = get_has(vm, b"full");
            let sb = scoreboard::get();
            let copy = if sb.is_null() {
                None
            } else {
                let shared = unsafe { (*sb).shared };
                scoreboard::copy(if shared.is_null() { sb } else { shared }, full)
            };
            let Some(copy) = copy else {
                zlog!(
                    ERROR,
                    "status: unable to find or access status shared memory"
                );
                set_response_code(vm, 500);
                common(Some("text/plain"));
                vm.write_output(b"Internal error. Please review log file for errors.");
                return true;
            };
            if copy.sb.idle < 0 || copy.sb.active < 0 {
                zlog!(
                    ERROR,
                    "[pool {}] invalid status values",
                    bstr(scoreboard::cstr(&copy.sb.pool))
                );
                set_response_code(vm, 500);
                common(Some("text/plain"));
                vm.write_output(b"Internal error. Please review log file for errors.");
                return true;
            }
            common(None);
            set_response_code(vm, 200);
            if headers_only {
                return true;
            }
            use super::status::Format;
            let fmt = if get_has(vm, b"html") {
                Format::Html
            } else if get_has(vm, b"xml") {
                Format::Xml
            } else if get_has(vm, b"json") {
                Format::Json
            } else if get_has(vm, b"openmetrics") {
                Format::OpenMetrics
            } else {
                Format::Plain
            };
            let page = super::status::render(&copy, full, fmt);
            goro_core::sapi_headers::add_header(
                format!("Content-Type: {}", page.content_type).as_bytes(),
                true,
            );
            vm.write_output(page.body.as_bytes());
            return true;
        }
    }
    false
}

/// The slow request's start: whether a backtrace was already logged.
#[allow(dead_code)]
fn timeval_zero() -> Timeval {
    Timeval::default()
}
