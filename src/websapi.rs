//! Request handling shared by the web server SAPIs (the built-in `-S`
//! server and goro-httpd): running a script as an HTTP request (the
//! superglobals, the response headers and status collected from the
//! script) and the CGI-style `$_SERVER` helpers.

use std::cell::RefCell;
use std::os::unix::ffi::OsStrExt;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::engine::{self, IniSettings};

thread_local! {
    /// The request headers of the request being executed, in their original
    /// case (apache_request_headers()).
    static REQUEST_HEADERS: RefCell<Vec<(Vec<u8>, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

/// What a SAPI hands over to run a script for a request.
pub struct ScriptRequest<'a> {
    pub ini: &'a IniSettings,
    /// PHP_SAPI
    pub sapi: &'a str,
    pub method: &'a [u8],
    pub query_string: &'a [u8],
    /// The request headers in their original case, in order.
    pub headers: &'a [(Vec<u8>, Vec<u8>)],
    /// The Cookie and Content-Type request headers.
    pub cookie: Option<&'a [u8]>,
    pub content_type: Option<&'a [u8]>,
    pub body: &'a [u8],
    /// SCRIPT_FILENAME as request startup sees it.
    pub script_filename: Vec<u8>,
    /// SG(request_info).path_translated
    pub path_translated: Option<&'a [u8]>,
    /// The `$_SERVER` entries the SAPI registers.
    pub server_vars: Vec<(Vec<u8>, Vec<u8>)>,
}

/// The response a script produced.
pub struct ScriptOutput {
    /// A status line set with header("HTTP/1.1 ...").
    pub status_line: Option<Vec<u8>>,
    /// http_response_code()
    pub code: i32,
    /// The headers the script sent (header(), setcookie(), ...).
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    pub body: Vec<u8>,
}

/// The last error of a request, for the access log.
pub struct LastError {
    pub fatal: bool,
    pub message: String,
    pub file: String,
    pub line: u32,
}

/// A request's engine: request startup, script execution and request
/// shutdown.
pub struct PhpRun {
    pub vm: Vm,
}

impl PhpRun {
    #[allow(clippy::field_reassign_with_default)] // fields filled in step by step
    pub fn startup(req: &ScriptRequest) -> PhpRun {
        let mut vm = engine::new_vm(req.ini, req.sapi);
        vm.register_function(b"apache_request_headers", apache_request_headers);
        vm.register_function(b"getallheaders", apache_request_headers);
        REQUEST_HEADERS.with(|h| *h.borrow_mut() = req.headers.to_vec());
        let _ = goro_ext_standard::misc::take_sent_headers();
        let _ = goro_ext_standard::misc::take_status_line();
        let _ = goro_ext_standard::misc::take_response_code();
        let mut info = goro_ext_standard::request::RequestInfo::default();
        info.cgi = true;
        info.request_method = Some(req.method.to_vec());
        info.query_string = req.query_string.to_vec();
        info.cookie_data = req.cookie.map(|c| c.to_vec());
        info.content_type = req.content_type.map(|c| c.to_vec());
        info.content_length = req.body.len() as i64;
        info.body = req.body.to_vec();
        info.script_filename = req.script_filename.clone();
        info.env = std::env::vars_os()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect();
        info.server_vars = Some(req.server_vars.clone());
        goro_ext_standard::misc::set_translated_path(
            req.path_translated
                .map(|p| String::from_utf8_lossy(cstr(p)).into_owned()),
        );
        engine::request_startup(&mut vm, &info, req.ini);
        PhpRun { vm }
    }

    /// Compile and run `path` as a main script piece.
    pub fn run_file(&mut self, path: &[u8]) -> Result<Value, VmError> {
        let src = match std::fs::read(std::ffi::OsStr::from_bytes(cstr(path))) {
            Ok(s) => s,
            Err(_) => {
                self.vm.display_error(
                    "Warning",
                    "Unknown: Failed to open stream: No such file or directory",
                    "Unknown",
                    0,
                );
                return Ok(Value::Null);
            }
        };
        let name = std::fs::canonicalize(std::ffi::OsStr::from_bytes(cstr(path)))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(path).into_owned());
        self.vm.current_file = name.as_str().into();
        // opcache: the script's compilation from an earlier request.
        if let Some(c) = goro_core::script_cache::lookup(&name) {
            self.vm.replay_cached_diags(&c.diags);
            for class in c.classes.iter().cloned() {
                self.vm.register_class(class);
            }
            return self.vm.execute_chunk(&c.op_array);
        }
        match engine::compile(&src, &name, true) {
            Ok(mut c) => {
                if goro_core::script_cache::enabled()
                    && goro_core::script_cache::store_compiled(
                        &name,
                        std::rc::Rc::new(goro_core::script_cache::Compiled {
                            op_array: c.op_array.clone(),
                            classes: c.classes.clone().into(),
                            diags: Vec::new(),
                        }),
                    )
                {
                    self.vm.run_optimizer_passes();
                }
                engine::load(&mut self.vm, &mut c);
                self.vm.execute_chunk(&c.op_array)
            }
            Err(e) => {
                engine::report_compile_error(&mut self.vm, &e, &name);
                self.vm.fatal_emitted = true;
                self.vm.last_error = Some((4, e.message.clone(), name, e.line));
                Err(VmError {
                    message: e.message,
                    line: e.line,
                })
            }
        }
    }

    /// Run the script `path` the way php_execute_script() does: from the
    /// script's directory.
    pub fn execute_script(&mut self, path: &[u8]) -> Result<Value, VmError> {
        let dir = std::path::Path::new(std::ffi::OsStr::from_bytes(cstr(path)))
            .parent()
            .map(|p| p.to_path_buf());
        with_cwd(dir, || self.run_file(path))
    }

    pub fn last_error(&self) -> Option<LastError> {
        self.vm
            .last_error
            .as_ref()
            .map(|(ty, msg, file, line)| LastError {
                // E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR | E_RECOVERABLE_ERROR | E_PARSE
                fatal: ty & (1 | 16 | 64 | 256 | 4096 | 4) != 0,
                message: msg.clone(),
                file: file.clone(),
                line: *line,
            })
    }

    /// php_request_shutdown(): shutdown functions, destructors, output.
    pub fn shutdown(mut self, result: Result<Value, VmError>) -> ScriptOutput {
        let headers_sent_before_error = self.vm.headers_sent;
        let r = self.vm.finish_request(result);
        let failed = r.is_err() || self.vm.fatal_emitted;
        if let Err(e) = r {
            if self.vm.current_exception.is_some() || !self.vm.fatal_emitted {
                self.vm.report_uncaught(&e);
            }
            self.vm.run_shutdown_destructors();
        }
        // php_error_cb(): a fatal error with display_errors off turns a
        // 200 response that has not started into a 500.
        if failed && !headers_sent_before_error {
            let display = self
                .vm
                .constants
                .get(b"display_errors".as_slice())
                .map(|v| v.to_php_string().to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let off = matches!(display.trim(), "" | "0" | "off" | "no" | "false" | "none");
            if off && goro_ext_standard::misc::peek_response_code() == 200 {
                goro_ext_standard::misc::set_status_line(b"HTTP/1.0 500 Internal Server Error");
            }
        }
        let body = self.vm.take_output();
        let headers = goro_ext_standard::misc::take_sent_headers();
        let status_line = goro_ext_standard::misc::take_status_line();
        let code = goro_ext_standard::misc::take_response_code() as i32;
        goro_ext_standard::request::shutdown();
        ScriptOutput {
            status_line,
            code,
            headers,
            body,
        }
    }
}

/// Run `f` with `dir` as the current directory (php_execute_script()
/// changes to the script's directory).
pub fn with_cwd<R>(dir: Option<std::path::PathBuf>, f: impl FnOnce() -> R) -> R {
    let old = std::env::current_dir().ok();
    if let Some(d) = &dir {
        let _ = std::env::set_current_dir(d);
    }
    let r = f();
    if dir.is_some() {
        if let Some(o) = old {
            let _ = std::env::set_current_dir(o);
        }
    }
    r
}

/// apache_request_headers() / getallheaders()
fn apache_request_headers(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    REQUEST_HEADERS.with(|h| {
        for (k, v) in h.borrow().iter() {
            arr.set(
                ArrayKey::String(PhpString::from_bytes(k)),
                Value::String(PhpString::from_bytes(v)),
            );
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// sapi_apply_default_charset()
pub fn apply_default_charset(mimetype: &[u8]) -> Vec<u8> {
    let mut m = mimetype.to_vec();
    if m.starts_with(b"text/") && !m.windows(8).any(|w| w == b"charset=") {
        m.extend_from_slice(b";charset=UTF-8");
    }
    m
}

/// The bytes up to the first NUL (C string view).
pub fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(z) => &b[..z],
        None => b,
    }
}

/// php_handle_auth_data(): PHP_AUTH_USER / PHP_AUTH_PW / PHP_AUTH_DIGEST.
pub fn auth_vars(auth: &[u8], out: &mut Vec<(Vec<u8>, Vec<u8>)>) {
    if auth.len() >= 6 && auth[..6].eq_ignore_ascii_case(b"Basic ") {
        if let Some(user) = base64_decode(&auth[6..]) {
            let user = match user.iter().position(|&c| c == 0) {
                Some(z) => user[..z].to_vec(),
                None => user,
            };
            if let Some(colon) = user.iter().position(|&c| c == b':') {
                out.push((b"PHP_AUTH_USER".to_vec(), user[..colon].to_vec()));
                let pass = &user[colon + 1..];
                if !pass.is_empty() {
                    out.push((b"PHP_AUTH_PW".to_vec(), pass.to_vec()));
                }
                return;
            }
        }
    }
    if auth.len() >= 7 && auth[..7].eq_ignore_ascii_case(b"Digest ") {
        out.push((b"PHP_AUTH_DIGEST".to_vec(), auth[7..].to_vec()));
    }
}

/// php_base64_decode() (non-strict: skips invalid characters).
fn base64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in s {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// php_register_variable_safe(): the variable name as PHP registers it
/// ('.' and ' ' become '_'; names with '[' are not plain keys).
pub fn mangle_var_name(name: &[u8]) -> Option<Vec<u8>> {
    let mut start = 0;
    while start < name.len() && name[start] == b' ' {
        start += 1;
    }
    let name = &name[start..];
    if name.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(name.len());
    for &c in name {
        match c {
            b' ' | b'.' => out.push(b'_'),
            b'[' => break,
            _ => out.push(c),
        }
    }
    Some(out)
}

/// The `HTTP_*` name of a request header (CONTENT_TYPE / CONTENT_LENGTH
/// for those two, which are registered under both names).
pub fn header_var_name(name: &[u8]) -> Vec<u8> {
    name.iter()
        .map(|&c| {
            if c == b'-' {
                b'_'
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect()
}

/// main/http_status_codes.h
pub fn status_string(code: i32) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Request Entity Too Large",
        414 => "Request-URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Requested Range Not Satisfiable",
        417 => "Expectation Failed",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        511 => "Network Authentication Required",
        _ => "Unknown Status Code",
    }
}
