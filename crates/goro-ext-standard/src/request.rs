//! Request startup: registration of the request superglobals
//! ($_GET, $_POST, $_COOKIE, $_FILES, $_SERVER, $_ENV, $_REQUEST, $argv/$argc),
//! ported from main/php_variables.c, main/SAPI.c, main/php_content_types.c and
//! main/rfc1867.c. Used by SAPIs (CLI / CGI emulation in the phpt runner).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// Description of the incoming request, filled in by the SAPI.
#[derive(Default, Clone, Debug)]
pub struct RequestInfo {
    /// CGI semantics (request vars from the environment) instead of CLI.
    pub cgi: bool,
    pub request_method: Option<Vec<u8>>,
    pub query_string: Vec<u8>,
    pub cookie_data: Option<Vec<u8>>,
    pub content_type: Option<Vec<u8>>,
    pub content_length: i64,
    /// Raw request body (what the SAPI's read_post would return).
    pub body: Vec<u8>,
    /// CLI argv (argv[0] = script path). Empty for CGI.
    pub argv: Vec<Vec<u8>>,
    /// Environment variables, in order.
    pub env: Vec<(Vec<u8>, Vec<u8>)>,
    /// Script path (PHP_SELF / SCRIPT_FILENAME ...).
    pub script_filename: Vec<u8>,
    /// CLI: PHP_SELF / SCRIPT_NAME when they differ from script_filename
    /// ("Standard input code" for code from stdin or -r).
    pub php_self: Option<Vec<u8>>,
    /// The SAPI registers $_SERVER itself (the built-in web server):
    /// entries added after the environment import.
    pub server_vars: Option<Vec<(Vec<u8>, Vec<u8>)>>,
    /// Which of `server_vars` the SAPI passes through the input filter
    /// (PARSE_SERVER): ext/filter keeps them for INPUT_SERVER and applies
    /// filter.default.
    pub server_var_filtered: Option<fn(&[u8]) -> bool>,
}

thread_local! {
    static UPLOADED_FILES: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    /// SG(request_info) of the current request, for request_parse_body().
    static REQUEST: RefCell<Option<RequestInfo>> = const { RefCell::new(None) };
    static INPUT_FILTER: std::cell::Cell<Option<InputFilter>> = const { std::cell::Cell::new(None) };
    /// The raw (unfiltered) request variables the input filter saw, by
    /// PARSE_* source (ext/filter's IF_G(get_array) ...).
    static RAW_INPUT: RefCell<[Option<Rc<RefCell<PhpArray>>>; 6]> = const { RefCell::new([None, None, None, None, None, None]) };
}

/// Sources of request variables (main/php_variables.h PARSE_*).
pub const PARSE_POST: u8 = 0;
pub const PARSE_GET: u8 = 1;
pub const PARSE_COOKIE: u8 = 2;
pub const PARSE_STRING: u8 = 3;
pub const PARSE_ENV: u8 = 4;
pub const PARSE_SERVER: u8 = 5;

/// sapi_module.input_filter (ext/filter's php_sapi_filter()): the value a
/// non-empty request variable from source `arg` is registered with.
pub type InputFilter = fn(&mut Vm, u8, &[u8]) -> Value;

/// sapi_register_input_filter()
pub fn set_input_filter(f: InputFilter) {
    INPUT_FILTER.with(|c| c.set(Some(f)));
}

/// SG(request_info) of the current request (None outside a request).
pub fn request_info() -> Option<RequestInfo> {
    REQUEST.with(|r| r.borrow().clone())
}

/// The raw request variables from source `arg` (PARSE_*), when the input
/// filter received some.
pub fn raw_input(arg: u8) -> Option<Rc<RefCell<PhpArray>>> {
    RAW_INPUT.with(|r| r.borrow().get(arg as usize).cloned().flatten())
}

/// Register a request variable through the input filter: the raw value is
/// kept (except for parse_str()), the filtered one registered in `track`.
fn input_register(
    ctx: &mut Ctx,
    arg: u8,
    name: &[u8],
    val: Vec<u8>,
    track: &Rc<RefCell<PhpArray>>,
    is_cookie: bool,
) {
    let Some(f) = INPUT_FILTER.with(|c| c.get()) else {
        register_variable(
            ctx,
            name,
            Value::String(PhpString::from_vec(val)),
            track,
            is_cookie,
        );
        return;
    };
    let name = cstr(name);
    // A duplicate cookie name keeps the first (more specific) cookie.
    if arg == PARSE_COOKIE && track.borrow().contains_key(&sym_key(name)) {
        return;
    }
    if arg != PARSE_STRING {
        let raw = RAW_INPUT.with(|r| {
            r.borrow_mut()[arg as usize]
                .get_or_insert_with(new_arr)
                .clone()
        });
        register_variable(
            ctx,
            name,
            Value::String(PhpString::from_bytes(&val)),
            &raw,
            false,
        );
    }
    let v = if val.is_empty() {
        Value::String(PhpString::empty())
    } else {
        f(ctx.vm, arg, &val)
    };
    register_variable(ctx, name, v, track, is_cookie);
}

/// request_parse_body() $options (REQUEST_PARSE_BODY_OPTION_*).
#[derive(Default)]
pub struct ParseBodyOptions {
    pub max_file_uploads: Option<i64>,
    pub max_input_vars: Option<i64>,
    pub max_multipart_body_parts: Option<i64>,
    pub post_max_size: Option<i64>,
    pub upload_max_filesize: Option<i64>,
}

/// The parse failure of request_parse_body(): the message of the
/// RequestParseBodyException to throw.
pub type ParseBodyError = String;

/// request_parse_body(): parse the request body like a POST request,
/// returning [$_POST, $_FILES] without touching the superglobals.
pub fn request_parse_body(
    vm: &mut Vm,
    opts: &ParseBodyOptions,
) -> Result<(Rc<RefCell<PhpArray>>, Rc<RefCell<PhpArray>>), ParseBodyError> {
    let req = REQUEST.with(|r| r.borrow().clone()).unwrap_or_default();
    let Some(ct) = req.content_type.clone() else {
        return Err("Request does not provide a content type".to_string());
    };
    let ini = |vm: &Vm, k: &str| -> Option<String> {
        vm.constants
            .get(k.as_bytes())
            .map(|v| v.to_php_string().to_string_lossy())
    };
    let end = ct
        .iter()
        .position(|&c| c == b';' || c == b',' || c == b' ')
        .unwrap_or(ct.len());
    let base = ct[..end].to_ascii_lowercase();
    let max_vars = opts.max_input_vars.unwrap_or_else(|| {
        ini(vm, "max_input_vars")
            .map(|v| parse_quantity(&v))
            .unwrap_or(1000)
    });
    let post_max_size = opts.post_max_size.unwrap_or_else(|| {
        ini(vm, "post_max_size")
            .map(|v| parse_quantity(&v))
            .unwrap_or(8 * 1024 * 1024)
    });
    let max_nesting = ini(vm, "max_input_nesting_level")
        .map(|v| parse_quantity(&v))
        .unwrap_or(64);
    let display_errors = ini_bool(ini(vm, "display_errors"), true);
    let cfg = UploadCfg {
        max_file_uploads: opts.max_file_uploads.unwrap_or_else(|| {
            ini(vm, "max_file_uploads")
                .map(|v| parse_quantity(&v))
                .unwrap_or(20)
        }),
        max_body_parts: opts.max_multipart_body_parts.unwrap_or_else(|| {
            ini(vm, "max_multipart_body_parts")
                .map(|v| parse_quantity(&v))
                .unwrap_or(-1)
        }),
        post_max_size,
        max_input_vars: max_vars,
        upload_max_filesize: opts.upload_max_filesize.unwrap_or_else(|| {
            ini(vm, "upload_max_filesize")
                .map(|v| parse_quantity(&v))
                .unwrap_or(2 * 1024 * 1024)
        }),
        file_uploads: ini_bool(ini(vm, "file_uploads"), true),
        upload_tmp_dir: ini(vm, "upload_tmp_dir"),
        query_string: req.query_string.clone(),
        cookie: req.cookie_data.clone().unwrap_or_default(),
    };
    let post = new_arr();
    let files = new_arr();
    let mut ctx = Ctx {
        max_nesting,
        display_errors,
        vm,
        parse_body: true,
        exception: None,
    };
    let is_post = req.request_method.as_deref() == Some(b"POST".as_slice());
    // sapi_read_standard_form_data(): a new request_body stream with what
    // the SAPI still has (nothing once the body was read); over
    // post_max_size the current one stays.
    let read_form_data = |ctx: &mut Ctx| {
        if post_max_size > 0 && req.content_length > post_max_size {
            ctx_docref_warning(
                ctx,
                &format!(
                    "POST Content-Length of {} bytes exceeds the limit of {} bytes",
                    req.content_length, post_max_size
                ),
            );
        } else {
            crate::misc::set_request_body(crate::misc::read_sapi_body());
        }
    };
    if base == b"application/x-www-form-urlencoded" {
        read_form_data(&mut ctx);
        // php_std_post_handler() parses SG(request_info).request_body.
        let data = crate::misc::buffered_request_body();
        std_post_handler(&mut ctx, &data, max_vars, &post);
    } else if base == b"multipart/form-data" {
        // rfc1867 reads the SAPI body directly.
        let data = crate::misc::peek_sapi_body();
        let used = rfc1867_handler(
            &mut ctx,
            &ct,
            &data,
            req.content_length,
            &cfg,
            &post,
            &files,
        );
        crate::misc::advance_sapi_body(used);
    } else {
        // php_default_post_reader() swallows a POST body of an unknown type.
        if is_post {
            read_form_data(&mut ctx);
        }
        return Err(format!(
            "Content-Type \"{}\" is not supported",
            goro_core::string::msg_str(&ct)
        ));
    }
    match ctx.exception {
        Some(e) => Err(e),
        None => Ok((post, files)),
    }
}

type Ini<'a> = &'a dyn Fn(&str) -> Option<String>;

fn new_arr() -> Rc<RefCell<PhpArray>> {
    Rc::new(RefCell::new(PhpArray::new()))
}

fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

/// ZEND_HANDLE_NUMERIC_STR
fn numeric_key(s: &[u8]) -> Option<i64> {
    if s.is_empty() || s.len() > 20 {
        return None;
    }
    let (neg, digits) = if s[0] == b'-' {
        (true, &s[1..])
    } else {
        (false, s)
    };
    if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if digits[0] == b'0' && (digits.len() > 1 || neg) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse::<i64>().ok()
}

fn sym_key(s: &[u8]) -> ArrayKey {
    match numeric_key(s) {
        Some(n) => ArrayKey::Int(n),
        None => ArrayKey::String(PhpString::from_bytes(s)),
    }
}

fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// php_url_decode / php_raw_url_decode
fn url_decode(s: &[u8], plus: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'+' && plus {
            out.push(b' ');
        } else if c == b'%' && i + 2 < s.len() {
            match (hexval(s[i + 1]), hexval(s[i + 2])) {
                (Some(a), Some(b)) => {
                    out.push(a * 16 + b);
                    i += 3;
                    continue;
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// C-string semantics: stop at the first NUL byte.
fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

fn is_forbidden(mangled: &[u8], orig: &[u8]) -> bool {
    (mangled.starts_with(b"__Host-") && !orig.starts_with(b"__Host-"))
        || (mangled.starts_with(b"__Secure-") && !orig.starts_with(b"__Secure-"))
}

struct Ctx<'a> {
    max_nesting: i64,
    display_errors: bool,
    vm: &'a mut Vm,
    /// Parsing for request_parse_body(): diagnostics are runtime warnings
    /// (or the RequestParseBodyException kept here) instead of startup ones.
    parse_body: bool,
    exception: Option<String>,
}

/// EMIT_WARNING_OR_ERROR() of rfc1867.c.
fn ctx_warn(ctx: &mut Ctx, msg: &str) {
    if ctx.parse_body {
        if ctx.exception.is_none() {
            ctx.exception = Some(msg.to_string());
        }
    } else {
        emit_startup(ctx.vm, "Warning", &format!("PHP Request Startup: {}", msg));
    }
}

/// sapi_module.sapi_error(E_WARNING) while parsing the request body
/// (php_error(): no function prefix, never an exception).
fn ctx_sapi_warning(ctx: &mut Ctx, msg: &str) {
    if ctx.parse_body {
        ctx.vm.emit_warning(msg);
    } else {
        emit_startup(ctx.vm, "Warning", msg);
    }
}

/// php_error_docref(E_NOTICE) while parsing the request body.
fn ctx_notice(ctx: &mut Ctx, msg: &str) {
    if ctx.parse_body {
        let line = ctx.vm.current_line;
        ctx.vm
            .emit_notice_at(&format!("request_parse_body(): {}", msg), line);
    } else {
        emit_startup(ctx.vm, "Notice", &format!("PHP Request Startup: {}", msg));
    }
}

/// php_error_docref(E_WARNING) while parsing the request body.
fn ctx_docref_warning(ctx: &mut Ctx, msg: &str) {
    if ctx.parse_body {
        ctx.vm
            .emit_warning(&format!("request_parse_body(): {}", msg));
    } else {
        emit_startup(ctx.vm, "Warning", &format!("PHP Request Startup: {}", msg));
    }
}

/// php_register_variable_ex
fn register_variable(
    ctx: &mut Ctx,
    var_name: &[u8],
    val: Value,
    track: &Rc<RefCell<PhpArray>>,
    is_cookie: bool,
) {
    let mut var_name = cstr(var_name);
    while var_name.first() == Some(&b' ') {
        var_name = &var_name[1..];
    }
    let orig = var_name;
    let mut var: Vec<u8> = var_name.to_vec();
    let mut is_array = false;
    let mut ip = 0usize;
    let mut var_len = var.len();
    for (i, c) in var.iter_mut().enumerate() {
        if *c == b' ' || *c == b'.' {
            *c = b'_';
        } else if *c == b'[' {
            is_array = true;
            ip = i;
            var_len = i;
            break;
        }
    }
    if var_len == 0 {
        return;
    }
    // GLOBALS hijack attempts are only rejected for the symbol table.
    let mut index: Option<Vec<u8>> = Some(var[..var_len].to_vec());
    let mut cur = track.clone();
    let top_name = var[..var_len].to_vec();

    if is_array {
        // `var` from ip on is the remainder "[...]..."; operate on bytes.
        let rest = var.clone();
        let mut pos = ip; // points at '['
        let mut nest_level = 0;
        loop {
            nest_level += 1;
            if nest_level > ctx.max_nesting {
                track.borrow_mut().remove(&sym_key(&top_name));
                if !ctx.display_errors {
                    ctx.vm.emit_warning(&format!(
                        "Input variable nesting level exceeded {}. To increase the limit change max_input_nesting_level in php.ini.",
                        ctx.max_nesting
                    ));
                }
                return;
            }
            pos += 1;
            let index_s = pos;
            let mut p = pos;
            if p < rest.len() && (rest[p] as char).is_ascii_whitespace()
                || (p < rest.len() && rest[p] == 0x0b)
            {
                p += 1;
            }
            let new_index: Option<Vec<u8>>;
            if p < rest.len() && rest[p] == b']' {
                new_index = None;
                pos = p;
            } else {
                match rest[p.min(rest.len())..].iter().position(|&c| c == b']') {
                    None if nest_level > 1 => {
                        // Inside a nested index the previous index was already
                        // NUL-terminated, so the remainder is dropped.
                        plain_var(&cur, index.clone(), val, orig, is_cookie, track);
                        return;
                    }
                    None => {
                        // not an index; un-terminate the var name
                        let mut name = index.clone().unwrap_or_default();
                        name.push(b'_');
                        for &c in &rest[index_s..] {
                            name.push(if c == b' ' || c == b'.' || c == b'[' {
                                b'_'
                            } else {
                                c
                            });
                        }
                        // plain var with the whole mangled name
                        plain_var(&cur, Some(name), val, orig, is_cookie, track);
                        return;
                    }
                    Some(off) => {
                        let end = p + off;
                        new_index = Some(rest[index_s..end].to_vec());
                        pos = end;
                    }
                }
            }
            // descend
            let child: Rc<RefCell<PhpArray>> = match &index {
                None => {
                    let a = new_arr();
                    if cur.borrow_mut().try_push(Value::Array(a.clone())).is_err() {
                        return;
                    }
                    a
                }
                Some(idx) => {
                    if is_forbidden(idx, orig) {
                        return;
                    }
                    let key = sym_key(idx);
                    let existing = cur.borrow().get(&key).cloned();
                    match existing {
                        Some(Value::Array(a)) => {
                            // separate if shared
                            if Rc::strong_count(&a) > 1 {
                                let copy = Rc::new(RefCell::new(a.borrow().clone()));
                                cur.borrow_mut().set(key, Value::Array(copy.clone()));
                                copy
                            } else {
                                a
                            }
                        }
                        _ => {
                            let a = new_arr();
                            cur.borrow_mut().set(key, Value::Array(a.clone()));
                            a
                        }
                    }
                }
            };
            cur = child;
            index = new_index;
            pos += 1; // past ']'
            if pos < rest.len() && rest[pos] == b'[' {
                continue;
            }
            plain_var(&cur, index, val, orig, is_cookie, track);
            return;
        }
    } else {
        plain_var(&cur, index, val, orig, is_cookie, track);
    }
}

fn plain_var(
    arr: &Rc<RefCell<PhpArray>>,
    index: Option<Vec<u8>>,
    val: Value,
    orig: &[u8],
    is_cookie: bool,
    top: &Rc<RefCell<PhpArray>>,
) {
    match index {
        None => {
            let _ = arr.borrow_mut().try_push(val);
        }
        Some(idx) => {
            if is_forbidden(&idx, orig) {
                return;
            }
            let key = sym_key(&idx);
            if is_cookie && Rc::ptr_eq(arr, top) && arr.borrow().contains_key(&key) {
                return;
            }
            arr.borrow_mut().set(key, val);
        }
    }
}

/// php_strtok_r over a set of separator bytes, skipping empty tokens.
fn strtok<'a>(s: &'a [u8], seps: &[u8]) -> Vec<&'a [u8]> {
    s.split(|c| seps.contains(c))
        .filter(|t| !t.is_empty())
        .collect()
}

fn emit_startup(vm: &mut Vm, level: &str, msg: &str) {
    let bit = match level {
        "Warning" => 2,
        "Deprecated" => 8192,
        _ => 8,
    };
    // php_error_cb() records the error for error_get_last() whether or not
    // it is reported.
    vm.last_error = Some((bit, msg.to_string(), "Unknown".to_string(), 0));
    if vm.error_reporting & bit != 0 {
        let html = vm
            .constants
            .get(b"html_errors".as_slice())
            .is_some_and(|v| ini_bool(Some(v.to_php_string().to_string_lossy()), false));
        if html {
            vm.display_error(level, msg, "Unknown", 0);
        } else {
            vm.write_output(format!("\n{}: {} in Unknown on line 0\n", level, msg).as_bytes());
        }
    }
}

/// php_default_treat_data for GET / COOKIE / STRING
fn treat_data(
    ctx: &mut Ctx,
    arg: u8,
    data: &[u8],
    is_cookie: bool,
    separators: &[u8],
    max_vars: i64,
    arr: &Rc<RefCell<PhpArray>>,
) {
    let data = cstr(data);
    if data.is_empty() {
        return;
    }
    let mut count = 0i64;
    for tok in strtok(data, separators) {
        let mut var = tok;
        if is_cookie {
            let mut skip = 0;
            while skip < var.len() && (var[skip] as char).is_ascii_whitespace() {
                skip += 1;
            }
            var = &var[skip..];
            if var.is_empty() || var[0] == b'=' {
                continue;
            }
        }
        let eq = var.iter().position(|&c| c == b'=');
        count += 1;
        if count > max_vars {
            emit_startup(
                ctx.vm,
                "Warning",
                &format!(
                    "PHP Request Startup: Input variables exceeded {}. To increase the limit change max_input_vars in php.ini.",
                    max_vars
                ),
            );
            break;
        }
        let (name, value) = match eq {
            Some(p) => {
                let v = &var[p + 1..];
                (var[..p].to_vec(), url_decode(v, !is_cookie))
            }
            None => (var.to_vec(), Vec::new()),
        };
        let name = if is_cookie {
            name
        } else {
            url_decode(&name, true)
        };
        input_register(ctx, arg, &name, value, arr, is_cookie);
    }
}

/// php_std_post_handler (application/x-www-form-urlencoded)
fn std_post_handler(ctx: &mut Ctx, body: &[u8], max_vars: i64, arr: &Rc<RefCell<PhpArray>>) {
    let mut cnt: i64 = 0;
    let mut ptr = 0usize;
    while ptr < body.len() {
        let vsep = body[ptr..]
            .iter()
            .position(|&c| c == b'&')
            .map(|p| ptr + p)
            .unwrap_or(body.len());
        let seg = &body[ptr..vsep];
        let (k, v) = match seg.iter().position(|&c| c == b'=') {
            Some(p) => (&seg[..p], &seg[p + 1..]),
            None => (seg, &b""[..]),
        };
        let key = url_decode(k, true);
        let val = if v.is_empty() {
            Vec::new()
        } else {
            url_decode(v, true)
        };
        input_register(ctx, PARSE_POST, &key, val, arr, false);
        ptr = vsep + if vsep != body.len() { 1 } else { 0 };
        cnt += 1;
        if cnt > max_vars {
            ctx_docref_warning(
                ctx,
                &format!(
                    "Input variables exceeded {}. To increase the limit change max_input_vars in php.ini.",
                    max_vars
                ),
            );
            return;
        }
    }
}

fn parse_quantity(s: &str) -> i64 {
    let s = s.trim();
    let digits: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+')
        .collect();
    let n: i64 = digits.parse().unwrap_or(0);
    let mult = match s[digits.len()..].trim().chars().next() {
        Some('k') | Some('K') => 1024,
        Some('m') | Some('M') => 1024 * 1024,
        Some('g') | Some('G') => 1024 * 1024 * 1024,
        _ => 1,
    };
    n.saturating_mul(mult)
}

fn ini_bool(v: Option<String>, default: bool) -> bool {
    match v {
        None => default,
        Some(s) => {
            let l = s.trim().to_ascii_lowercase();
            matches!(l.as_str(), "1" | "on" | "yes" | "true")
                || l.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
    }
}

// ---------------------------------------------------------------------------
// rfc1867 multipart/form-data
// ---------------------------------------------------------------------------

const FILLUNIT: usize = 1024 * 5;

struct MultipartBuffer<'a> {
    src: &'a [u8],
    src_pos: usize,
    buffer: Vec<u8>,
    begin: usize,
    bytes_in_buffer: usize,
    bufsize: usize,
    boundary: Vec<u8>,
    boundary_next: Vec<u8>,
}

impl<'a> MultipartBuffer<'a> {
    fn new(src: &'a [u8], boundary: &[u8]) -> Self {
        let minsize = (boundary.len() + 6).max(FILLUNIT);
        let mut b = b"--".to_vec();
        b.extend_from_slice(boundary);
        let mut bn = b"\n--".to_vec();
        bn.extend_from_slice(boundary);
        MultipartBuffer {
            src,
            src_pos: 0,
            buffer: vec![0; minsize + 1],
            begin: 0,
            bytes_in_buffer: 0,
            bufsize: minsize,
            boundary: b,
            boundary_next: bn,
        }
    }

    fn fill_buffer(&mut self) -> usize {
        if self.bytes_in_buffer > 0 && self.begin != 0 {
            self.buffer
                .copy_within(self.begin..self.begin + self.bytes_in_buffer, 0);
        }
        self.begin = 0;
        let to_read = self.bufsize - self.bytes_in_buffer;
        let avail = self.src.len() - self.src_pos;
        let n = to_read.min(avail);
        if n > 0 {
            let at = self.bytes_in_buffer;
            self.buffer[at..at + n].copy_from_slice(&self.src[self.src_pos..self.src_pos + n]);
            self.src_pos += n;
            self.bytes_in_buffer += n;
        }
        n
    }

    fn eof(&mut self) -> bool {
        self.bytes_in_buffer == 0 && self.fill_buffer() < 1
    }

    fn next_line(&mut self) -> Option<Vec<u8>> {
        let data = &self.buffer[self.begin..self.begin + self.bytes_in_buffer];
        if let Some(p) = data.iter().position(|&c| c == b'\n') {
            let mut line = data[..p].to_vec();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.begin += p + 1;
            self.bytes_in_buffer -= p + 1;
            Some(cstr(&line).to_vec())
        } else {
            if self.bytes_in_buffer < self.bufsize {
                return None;
            }
            let line = data.to_vec();
            self.bytes_in_buffer = 0;
            Some(cstr(&line).to_vec())
        }
    }

    fn get_line(&mut self) -> Option<Vec<u8>> {
        match self.next_line() {
            Some(l) => Some(l),
            None => {
                self.fill_buffer();
                self.next_line()
            }
        }
    }

    fn find_boundary(&mut self) -> bool {
        while let Some(line) = self.get_line() {
            if line == self.boundary {
                return true;
            }
        }
        false
    }

    /// The part's headers; `each_line` sees every header line (the
    /// encoding detection of encoding translation).
    fn headers(
        &mut self,
        headers: &mut Vec<(Vec<u8>, Vec<u8>)>,
        each_line: &mut dyn FnMut(&[u8]),
    ) -> bool {
        if !self.find_boundary() {
            return false;
        }
        let mut key: Option<Vec<u8>> = None;
        let mut buf_value: Option<Vec<u8>> = None;
        while let Some(line) = self.get_line() {
            if line.is_empty() {
                break;
            }
            each_line(&line);
            let colon = if !line.is_empty() && !(line[0] as char).is_ascii_whitespace() {
                line.iter().position(|&c| c == b':')
            } else {
                None
            };
            if let Some(c) = colon {
                if let (Some(k), Some(v)) = (key.take(), buf_value.take()) {
                    headers.push((k, v));
                }
                let mut v = c + 1;
                while v < line.len() && (line[v] as char).is_ascii_whitespace() {
                    v += 1;
                }
                key = Some(line[..c].to_vec());
                buf_value = Some(line[v..].to_vec());
            } else if let Some(bv) = buf_value.as_mut() {
                bv.extend_from_slice(&line);
            }
        }
        if let (Some(k), Some(v)) = (key, buf_value) {
            headers.push((k, v));
        }
        true
    }

    /// php_ap_memstr: returns offset of match within the buffered data.
    fn memstr(&self, partial: bool) -> Option<usize> {
        let hay = &self.buffer[self.begin..self.begin + self.bytes_in_buffer];
        let needle = &self.boundary_next;
        let mut i = 0;
        while i < hay.len() {
            if hay[i] == needle[0] {
                let len = hay.len() - i;
                let cmp = needle.len().min(len);
                if hay[i..i + cmp] == needle[..cmp] && (partial || len >= needle.len()) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn read(&mut self, bytes: usize, end: &mut bool) -> Vec<u8> {
        if bytes > self.bytes_in_buffer {
            self.fill_buffer();
        }
        let bound = self.memstr(true);
        let max = match bound {
            Some(b) => {
                if self.memstr(false).is_some() {
                    *end = true;
                }
                b
            }
            None => self.bytes_in_buffer,
        };
        let len = max.min(bytes - 1);
        let mut out = Vec::new();
        if len > 0 {
            out.extend_from_slice(&self.buffer[self.begin..self.begin + len]);
            let consumed = len;
            if bound.is_some() && out.last() == Some(&b'\r') {
                out.pop();
                // C decrements len before updating the buffer: the '\r' stays
                self.bytes_in_buffer -= consumed - 1;
                self.begin += consumed - 1;
                return out;
            }
            self.bytes_in_buffer -= consumed;
            self.begin += consumed;
        }
        out
    }

    fn read_body(&mut self) -> Option<Vec<u8>> {
        let mut out: Option<Vec<u8>> = None;
        let mut dummy = false;
        loop {
            let chunk = self.read(FILLUNIT, &mut dummy);
            if chunk.is_empty() {
                break;
            }
            out.get_or_insert_with(Vec::new).extend_from_slice(&chunk);
        }
        out
    }
}

fn get_hdr<'b>(headers: &'b [(Vec<u8>, Vec<u8>)], key: &[u8]) -> Option<&'b [u8]> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_slice())
}

/// php_ap_getword, or php_mb_rfc1867_getword when `mb` gives the length of
/// the (multibyte) character a byte starts.
fn getword(line: &[u8], pos: &mut usize, stop: u8, mb: Mblen) -> Vec<u8> {
    let start = *pos;
    let mut p = start;
    while p < line.len() && line[p] != 0 && line[p] != stop {
        let q = line[p];
        if q == b'"' || q == b'\'' {
            p += 1;
            while p < line.len() && line[p] != q {
                if line[p] == b'\\' && p + 1 < line.len() && line[p + 1] == q {
                    p += 2;
                } else {
                    p += 1;
                }
            }
            if p < line.len() {
                p += 1;
            }
        } else {
            p += mb.len(q);
        }
    }
    if p >= line.len() {
        *pos = line.len();
        return line[start..].to_vec();
    }
    let res = line[start..p].to_vec();
    while p < line.len() && line[p] == stop {
        p += mb.len(line[p]);
    }
    *pos = p.min(line.len());
    res
}

/// The length of the character a byte starts, in the multibyte encoding
/// multipart headers are parsed in (1: none).
#[derive(Clone, Copy)]
struct Mblen(Option<(fn(goro_core::rfc1867::EncodingId, u8) -> usize, usize)>);

impl Mblen {
    #[inline]
    fn len(self, b: u8) -> usize {
        match self.0 {
            Some((f, e)) => f(e, b).max(1),
            None => 1,
        }
    }
}

/// php_ap_ / php_mb_rfc1867_substring_conf
fn substring_conf(s: &[u8], len: usize, quote: u8, mb: Mblen) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < len && i < s.len() && s[i] != quote {
        if s[i] == b'\\'
            && i + 1 < s.len()
            && (s[i + 1] == b'\\' || (quote != 0 && s[i + 1] == quote))
        {
            i += 1;
            out.push(s[i]);
            i += 1;
        } else {
            let mut j = mb.len(s[i]);
            while j > 0 && i < len && i < s.len() {
                out.push(s[i]);
                i += 1;
                j -= 1;
            }
        }
    }
    out
}

/// php_ap_getword_conf / php_mb_rfc1867_getword_conf
fn getword_conf(s: &[u8], mb: Mblen) -> Vec<u8> {
    let mut i = 0;
    while i < s.len() && (s[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    let s = &s[i..];
    if s.is_empty() {
        return Vec::new();
    }
    if s[0] == b'"' || s[0] == b'\'' {
        let q = s[0];
        substring_conf(&s[1..], s.len() - 1, q, mb)
    } else {
        let end = s
            .iter()
            .position(|c| (*c as char).is_ascii_whitespace())
            .unwrap_or(s.len());
        substring_conf(s, end, 0, mb)
    }
}

/// php_ap_basename / php_mb_rfc1867_basename (php_mb_safe_strrchr(): only
/// separators that are whole characters count).
fn ap_basename(path: &[u8], mb: Mblen) -> &[u8] {
    let (mut s, mut s2) = (None, None);
    let mut i = 0;
    while i < path.len() {
        match path[i] {
            b'\\' => s = Some(i),
            b'/' => s2 = Some(i),
            _ => {}
        }
        i += mb.len(path[i]);
    }
    match (s, s2) {
        (Some(a), Some(b)) => &path[a.max(b) + 1..],
        (Some(a), None) => &path[a + 1..],
        (None, Some(b)) => &path[b + 1..],
        (None, None) => path,
    }
}

fn normalize_protected_variable(v: &[u8]) -> Vec<u8> {
    let mut s = 0;
    while s < v.len() && v[s] == b' ' {
        s += 1;
    }
    let mut var: Vec<u8> = v[s..].to_vec();
    let mut i = 0;
    while i < var.len() && var[i] != b'[' {
        if var[i] == b' ' || var[i] == b'.' {
            var[i] = b'_';
        }
        i += 1;
    }
    let first = match var.iter().position(|&c| c == b'[') {
        Some(p) => p + 1,
        None => return var,
    };
    // Rebuild: strip leading whitespace inside each [index]
    let mut out: Vec<u8> = var[..first].to_vec();
    let mut index = first;
    loop {
        while index < var.len() && matches!(var[index], b' ' | b'\r' | b'\n' | b'\t') {
            index += 1;
        }
        let indexend = match var[index.min(var.len())..].iter().position(|&c| c == b']') {
            Some(p) => index + p + 1,
            None => var.len(),
        };
        out.extend_from_slice(&var[index.min(var.len())..indexend]);
        if indexend < var.len() && var[indexend] == b'[' {
            out.push(b'[');
            index = indexend + 1;
        } else {
            break;
        }
    }
    out
}

struct UploadCfg {
    max_file_uploads: i64,
    max_body_parts: i64,
    post_max_size: i64,
    max_input_vars: i64,
    upload_max_filesize: i64,
    file_uploads: bool,
    upload_tmp_dir: Option<String>,
    query_string: Vec<u8>,
    cookie: Vec<u8>,
}

/// rfc1867_post_handler. Returns the number of body bytes consumed.
fn rfc1867_handler(
    ctx: &mut Ctx,
    content_type: &[u8],
    body: &[u8],
    content_length: i64,
    cfg: &UploadCfg,
    post: &Rc<RefCell<PhpArray>>,
    files: &Rc<RefCell<PhpArray>>,
) -> usize {
    if cfg.post_max_size > 0 && content_length > cfg.post_max_size {
        ctx_warn(
            ctx,
            &format!(
                "POST Content-Length of {} bytes exceeds the limit of {} bytes",
                content_length, cfg.post_max_size
            ),
        );
        return 0;
    }
    let mut body_parts_cnt = cfg.max_body_parts;
    if body_parts_cnt < 0 {
        body_parts_cnt = cfg.max_input_vars + cfg.max_file_uploads;
    }
    let body_parts_limit = body_parts_cnt;

    let lc: Vec<u8> = content_type.to_ascii_lowercase();
    let bpos = match find(content_type, b"boundary").or_else(|| find(&lc, b"boundary")) {
        Some(p) => p,
        None => {
            ctx_warn(ctx, "Missing boundary in multipart/form-data POST data");
            return 0;
        }
    };
    let eq = match content_type[bpos..].iter().position(|&c| c == b'=') {
        Some(p) => bpos + p,
        None => {
            ctx_warn(ctx, "Missing boundary in multipart/form-data POST data");
            return 0;
        }
    };
    let mut boundary = &content_type[eq + 1..];
    if boundary.first() == Some(&b'"') {
        boundary = &boundary[1..];
        match boundary.iter().position(|&c| c == b'"') {
            Some(p) => boundary = &boundary[..p],
            None => {
                ctx_warn(ctx, "Invalid boundary in multipart/form-data POST data");
                return 0;
            }
        }
    } else if let Some(p) = boundary.iter().position(|&c| c == b',' || c == b';') {
        boundary = &boundary[..p];
    }
    if boundary.len() > FILLUNIT - 4 {
        ctx_sapi_warning(ctx, "Boundary too large in multipart/form-data POST data");
        return 0;
    }

    let mut mb = MultipartBuffer::new(body, boundary);
    let mut protected: Vec<Vec<u8>> = Vec::new();
    let mut upload_cnt = cfg.max_file_uploads;
    let mut anonymous_index = 0u32;
    // Function-wide in rfc1867_post_handler(): once a file part is skipped
    // (a malformed name ...), every later one is too.
    let mut skip_upload = false;
    let mut max_file_size: i64 = 0;
    let mut count: i64 = 0;

    use goro_core::rfc1867::Event;
    let cb = ctx.vm.rfc1867_callback;
    let mut skip_all = false;
    if let Some(f) = cb {
        let ev = Event::Start {
            content_length,
            query_string: &cfg.query_string,
            cookie: &cfg.cookie,
        };
        if !f(ctx.vm, &ev) {
            skip_all = true;
        }
    }

    // php_rfc1867_encoding_translation() with an internal encoding: names,
    // filenames and values are converted from the encoding detected from
    // the part's header lines (mbuff->input_encoding).
    let mbx = ctx.vm.rfc1867_multibyte.filter(|m| (m.start)(ctx.vm));
    let mut input_encoding: Option<goro_core::rfc1867::EncodingId> = None;
    while !skip_all && !mb.eof() {
        let mut headers = Vec::new();
        let vm = &mut *ctx.vm;
        let found = mb.headers(&mut headers, &mut |line| {
            if let Some(m) = mbx {
                input_encoding = (m.detect)(vm, line);
            }
        });
        if !found {
            break;
        }
        let wmb = Mblen(mbx.and_then(|m| input_encoding.map(|e| (m.mbchar_bytes, e))));
        let to_internal = |vm: &mut Vm, b: Vec<u8>| -> Vec<u8> {
            match (mbx, input_encoding) {
                (Some(m), Some(e)) => (m.convert)(vm, cstr(&b), e),
                _ => b,
            }
        };
        let cd = match get_hdr(&headers, b"Content-Disposition") {
            Some(c) => c.to_vec(),
            None => continue,
        };
        body_parts_cnt -= 1;
        if body_parts_cnt < 0 {
            ctx_warn(
                ctx,
                &format!(
                    "Multipart body parts limit exceeded {}. To increase the limit change max_multipart_body_parts in php.ini.",
                    body_parts_limit
                ),
            );
            break;
        }
        let mut param: Option<Vec<u8>> = None;
        let mut filename: Option<Vec<u8>> = None;
        let mut pos = 0;
        while pos < cd.len() && (cd[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        while pos < cd.len() && cd[pos] != 0 {
            let pair = getword(&cd, &mut pos, b';', wmb);
            while pos < cd.len() && (cd[pos] as char).is_ascii_whitespace() {
                pos += 1;
            }
            if pair.contains(&b'=') {
                let mut pp = 0;
                let key = getword(&pair, &mut pp, b'=', wmb);
                let rest = &pair[pp..];
                if key.eq_ignore_ascii_case(b"name") {
                    param = Some(to_internal(ctx.vm, getword_conf(rest, wmb)));
                } else if key.eq_ignore_ascii_case(b"filename") {
                    filename = Some(to_internal(ctx.vm, getword_conf(rest, wmb)));
                }
            }
        }
        // Normal form variable
        if filename.is_none() {
            if let Some(param) = param {
                let mut value = mb.read_body().unwrap_or_default();
                if let (Some(m), Some(e)) = (mbx, input_encoding) {
                    value = (m.convert)(ctx.vm, &value, e);
                }
                count += 1;
                // ext/filter's php_sapi_filter() registers the variable
                // itself (so the upload protection does not apply) and
                // returns 0: the code below treats it as filtered out.
                let filtered_out =
                    count <= cfg.max_input_vars && INPUT_FILTER.with(|c| c.get()).is_some();
                if filtered_out {
                    input_register(ctx, PARSE_POST, &param, value.clone(), post, false);
                }
                if count <= cfg.max_input_vars && !filtered_out {
                    if let Some(f) = cb {
                        let ev = Event::FormData {
                            post_bytes_processed: mb.src_pos as i64,
                            name: cstr(&param),
                            value: &value,
                        };
                        if !f(ctx.vm, &ev) {
                            continue;
                        }
                    }
                    if !protected.contains(&normalize_protected_variable(&param)) {
                        input_register(ctx, PARSE_POST, &param, value.clone(), post, false);
                    }
                } else {
                    if count == cfg.max_input_vars + 1 {
                        ctx_warn(
                            ctx,
                            &format!(
                                "Input variables exceeded {}. To increase the limit change max_input_vars in php.ini.",
                                cfg.max_input_vars
                            ),
                        );
                    }
                    if let Some(f) = cb {
                        let ev = Event::FormData {
                            post_bytes_processed: mb.src_pos as i64,
                            name: cstr(&param),
                            value: &value,
                        };
                        f(ctx.vm, &ev);
                    }
                }
                if param.eq_ignore_ascii_case(b"MAX_FILE_SIZE") {
                    let v = cstr(&value);
                    let s = String::from_utf8_lossy(v);
                    let t = s.trim_start();
                    let num: String = t
                        .chars()
                        .enumerate()
                        .take_while(|(i, c)| {
                            c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))
                        })
                        .map(|(_, c)| c)
                        .collect();
                    max_file_size = num.parse().unwrap_or(0);
                }
                continue;
            }
        }
        if !cfg.file_uploads {
            skip_upload = true;
        } else if upload_cnt <= 0 {
            skip_upload = true;
            if upload_cnt == 0 {
                upload_cnt -= 1;
                ctx_warn(
                    ctx,
                    "Maximum number of allowable file uploads has been exceeded",
                );
            }
        }
        if param.is_none() && filename.is_none() {
            ctx_warn(ctx, "File Upload Mime headers garbled");
            break;
        }
        let param = match param {
            Some(p) => p,
            None => {
                let p = anonymous_index.to_string().into_bytes();
                anonymous_index += 1;
                p
            }
        };
        let filename = filename.unwrap_or_default();
        if !skip_upload {
            let mut c: i64 = 0;
            let p = cstr(&param);
            for (i, &ch) in p.iter().enumerate() {
                if ch == b'[' {
                    c += 1;
                } else if ch == b']' {
                    c -= 1;
                    if i + 1 < p.len() && p[i + 1] != b'[' {
                        skip_upload = true;
                        break;
                    }
                }
                if c < 0 {
                    skip_upload = true;
                    break;
                }
            }
            if c != 0 {
                skip_upload = true;
            }
        }
        if !skip_upload {
            if let Some(f) = cb {
                let ev = Event::FileStart {
                    post_bytes_processed: mb.src_pos as i64,
                    name: cstr(&param),
                    filename: cstr(&filename),
                };
                if !f(ctx.vm, &ev) {
                    continue;
                }
            }
        }
        if skip_upload {
            continue;
        }
        let mut cancel_upload: i64 = 0;
        let mut offset: i64 = 0;
        if cstr(&filename).is_empty() {
            cancel_upload = 4;
        }
        let mut end = false;
        let mut total_bytes: i64 = 0;
        let mut temp: Option<(std::path::PathBuf, std::fs::File)> = None;
        let mut blen_data = Vec::new();
        if cancel_upload == 0 {
            blen_data = mb.read(FILLUNIT, &mut end);
            upload_cnt -= 1;
            // php_open_temporary_fd_ex(): upload_tmp_dir, else (with a
            // notice when that one failed) the system's temporary directory.
            let open =
                |dir: &std::path::Path| tempfile::Builder::new().prefix("php").tempfile_in(dir);
            let explicit = cfg.upload_tmp_dir.clone().filter(|d| !d.is_empty());
            let mut created = explicit.as_ref().map(|d| open(std::path::Path::new(d)));
            if !matches!(created, Some(Ok(_))) {
                if created.is_some() {
                    ctx_notice(ctx, "file created in the system's temporary directory");
                }
                let sys = crate::misc::php_get_temporary_directory(ctx.vm);
                created = Some(open(std::path::Path::new(&sys)));
            }
            match created.expect("set above") {
                Ok(t) => match t.keep() {
                    Ok((f, p)) => temp = Some((p, f)),
                    Err(_) => cancel_upload = 6,
                },
                Err(_) => {
                    ctx_warn(ctx, "File upload error - unable to create a temporary file");
                    cancel_upload = 6;
                }
            }
        }
        while cancel_upload == 0 && !blen_data.is_empty() {
            if let Some(f) = cb {
                let ev = Event::FileData {
                    post_bytes_processed: mb.src_pos as i64,
                    offset,
                    length: blen_data.len() as i64,
                };
                if !f(ctx.vm, &ev) {
                    cancel_upload = 8;
                    continue;
                }
            }
            let blen = blen_data.len() as i64;
            if cfg.upload_max_filesize > 0 && total_bytes + blen > cfg.upload_max_filesize {
                cancel_upload = 1;
            } else if max_file_size != 0 && total_bytes + blen > max_file_size {
                cancel_upload = 2;
            } else {
                use std::io::Write;
                if let Some((_, f)) = temp.as_mut() {
                    if f.write_all(&blen_data).is_err() {
                        cancel_upload = 7;
                    } else {
                        total_bytes += blen;
                    }
                    offset += blen;
                }
            }
            blen_data = mb.read(FILLUNIT, &mut end);
        }
        if cancel_upload == 0 && !end {
            cancel_upload = 3;
        }
        if let Some(f) = cb {
            let tn = temp
                .as_ref()
                .map(|(p, _)| p.to_string_lossy().as_bytes().to_vec());
            let ev = Event::FileEnd {
                post_bytes_processed: mb.src_pos as i64,
                temp_filename: tn.as_deref(),
                cancel_upload,
            };
            if !f(ctx.vm, &ev) {
                cancel_upload = 8;
            }
        }
        let temp_name: Option<Vec<u8>> = if cancel_upload != 0 {
            if let Some((p, _)) = temp.take() {
                if cancel_upload != 6 {
                    let _ = std::fs::remove_file(&p);
                }
            }
            None
        } else {
            temp.take().map(|(p, _)| {
                let b = p.to_string_lossy().as_bytes().to_vec();
                UPLOADED_FILES.with(|u| u.borrow_mut().push(b.clone()));
                b
            })
        };

        let pc = cstr(&param).to_vec();
        let start_arr = pc.iter().position(|&c| c == b'[');
        let is_arr_upload = start_arr.is_some() && pc.last() == Some(&b']');
        let (abuf, array_index) = if is_arr_upload {
            let sa = start_arr.unwrap();
            (pc[..sa].to_vec(), pc[sa + 1..pc.len() - 1].to_vec())
        } else {
            (Vec::new(), Vec::new())
        };
        let name_for = |field: &str| -> Vec<u8> {
            let mut v;
            if is_arr_upload {
                v = abuf.clone();
                v.extend_from_slice(format!("[{}][", field).as_bytes());
                v.extend_from_slice(&array_index);
                v.push(b']');
            } else {
                v = pc.clone();
                v.extend_from_slice(format!("[{}]", field).as_bytes());
            }
            v
        };
        let reg = |ctx: &mut Ctx,
                   protected: &Vec<Vec<u8>>,
                   name: Vec<u8>,
                   val: Value,
                   override_: bool| {
            if override_ || !protected.contains(&normalize_protected_variable(&name)) {
                register_variable(ctx, &name, val, files, false);
            }
        };
        let fname_c = cstr(&filename).to_vec();
        let bmb = Mblen(mbx.map(|m| (m.mbchar_bytes, (m.internal)(ctx.vm))));
        reg(
            ctx,
            &protected,
            name_for("name"),
            sval(ap_basename(&fname_c, bmb)),
            false,
        );
        reg(
            ctx,
            &protected,
            name_for("full_path"),
            sval(&fname_c),
            false,
        );
        let ctype: Vec<u8> = if cancel_upload != 0 {
            Vec::new()
        } else {
            match get_hdr(&headers, b"Content-Type") {
                Some(c) => {
                    let c = cstr(c);
                    match c.iter().position(|&x| x == b';') {
                        Some(p) => c[..p].to_vec(),
                        None => c.to_vec(),
                    }
                }
                None => Vec::new(),
            }
        };
        reg(ctx, &protected, name_for("type"), sval(&ctype), false);
        protected.push(normalize_protected_variable(&pc));
        let tmp_field = name_for("tmp_name");
        protected.push(normalize_protected_variable(&tmp_field));
        reg(
            ctx,
            &protected,
            tmp_field,
            sval(&temp_name.unwrap_or_default()),
            true,
        );
        reg(
            ctx,
            &protected,
            name_for("error"),
            Value::Long(cancel_upload),
            false,
        );
        let size = if cancel_upload != 0 { 0 } else { total_bytes };
        reg(ctx, &protected, name_for("size"), Value::Long(size), false);
    }
    if let Some(f) = cb {
        f(
            ctx.vm,
            &Event::End {
                post_bytes_processed: mb.src_pos as i64,
            },
        );
    }
    mb.src_pos
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// Request startup (php_request_startup → sapi_activate + php_hash_environment).
/// Registers all superglobals into `vm`, and sets the php://input body.
/// sapi_module.treat_data(PARSE_STRING, ...) as used by parse_str().
pub fn parse_str_into(vm: &mut Vm, data: &[u8], arr: &Rc<RefCell<PhpArray>>) {
    let ini = |k: &str| {
        vm.constants
            .get(k.as_bytes())
            .map(|v| v.to_php_string().to_string_lossy())
    };
    let max_vars = ini("max_input_vars")
        .map(|v| parse_quantity(&v))
        .unwrap_or(1000);
    let max_nesting = ini("max_input_nesting_level")
        .map(|v| parse_quantity(&v))
        .unwrap_or(64);
    let display_errors = ini_bool(ini("display_errors"), true);
    let arg_sep: Vec<u8> = ini("arg_separator.input")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "&".into())
        .into_bytes();
    let mut ctx = Ctx {
        max_nesting,
        display_errors,
        vm,
        parse_body: false,
        exception: None,
    };
    if !ctx
        .vm
        .treat_data_hook
        .is_some_and(|h| h(ctx.vm, 3, data, arr))
    {
        treat_data(&mut ctx, PARSE_STRING, data, false, &arg_sep, max_vars, arr);
    }
}

/// SAPI_POST_BLOCK_SIZE
const SAPI_POST_BLOCK_SIZE: usize = 0x4000;

/// Whether php_open_temporary_fd() can create a file in `dir`.
fn temp_file_possible(dir: &str) -> bool {
    let p = std::path::Path::new(dir).join(format!(".goro-post-{}", std::process::id()));
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

pub fn startup(vm: &mut Vm, req: &RequestInfo, ini: Ini) {
    goro_core::sapi_headers::reset();
    REQUEST.with(|r| *r.borrow_mut() = Some(req.clone()));
    RAW_INPUT.with(|r| *r.borrow_mut() = Default::default());
    crate::info::set_web_sapi(req.cgi);
    let cli = matches!(vm.constants.get(b"PHP_SAPI".as_slice()), Some(Value::String(s)) if s.as_bytes() == b"cli");
    if req.cgi && cli {
        // Requests (--POST--, --GET--, ...) are served by the CGI SAPI
        // (the web server SAPIs have a name of their own).
        goro_core::compiler::set_sapi_name(b"cgi-fcgi");
        vm.constants.insert(
            b"PHP_SAPI".to_vec(),
            Value::String(PhpString::from_bytes(b"cgi-fcgi")),
        );
        vm.constants
            .shift_remove(b"PHP_CLI_PROCESS_TITLE".as_slice());
    }
    // php_request_startup(): SAPI_PHP_VERSION_HEADER (the CLI SAPI has
    // no headers).
    if req.cgi && ini_bool(ini("expose_php"), true) {
        let version = vm
            .constants
            .get(b"PHP_VERSION".as_slice())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default();
        crate::misc::sapi_add_header(format!("X-Powered-By: PHP/{}", version).as_bytes(), true);
    }
    // SG(sapi_headers).http_response_code: 200 for a web request, none for
    // the CLI.
    crate::misc::RESPONSE_CODE.with(|c| c.set(if req.cgi { 200 } else { 0 }));
    // OnUpdateStringUnempty: an empty variables_order keeps the default.
    let variables_order = ini("variables_order")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "EGPCS".to_string());
    let has = |c: char| variables_order.chars().any(|x| x.eq_ignore_ascii_case(&c));
    let max_vars = ini("max_input_vars")
        .map(|v| parse_quantity(&v))
        .unwrap_or(1000);
    let max_nesting = ini("max_input_nesting_level")
        .map(|v| parse_quantity(&v))
        .unwrap_or(64);
    let display_errors = ini_bool(ini("display_errors"), true);
    let arg_sep: Vec<u8> = ini("arg_separator.input")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "&".into())
        .into_bytes();
    let post_max_size = ini("post_max_size")
        .map(|v| parse_quantity(&v))
        .unwrap_or(8 * 1024 * 1024);
    let enable_post = ini_bool(ini("enable_post_data_reading"), true);

    let get = new_arr();
    let post = new_arr();
    let cookie = new_arr();
    let files = new_arr();
    let mut input_body: Vec<u8> = Vec::new();
    // SG(read_post_bytes): how much of the body the SAPI delivered.
    let mut sapi_read = 0usize;
    let body_discarded = std::cell::Cell::new(false);

    {
        let mut ctx = Ctx {
            max_nesting,
            display_errors,
            vm,
            parse_body: false,
            exception: None,
        };

        // sapi_activate: read POST data
        let is_post = req.request_method.as_deref().is_some_and(|m| m == b"POST");
        let mut post_entry: Option<&str> = None;
        let mut buffered = false;
        if req.cgi {
            if enable_post && is_post && req.content_type.is_some() {
                let ct = req.content_type.clone().unwrap();
                let end = ct
                    .iter()
                    .position(|&c| c == b';' || c == b',' || c == b' ')
                    .unwrap_or(ct.len());
                let base = ct[..end].to_ascii_lowercase();
                let std_reader = |ctx: &mut Ctx| -> bool {
                    if post_max_size > 0 && req.content_length > post_max_size {
                        emit_startup(
                            ctx.vm,
                            "Warning",
                            &format!(
                                "PHP Request Startup: POST Content-Length of {} bytes exceeds the limit of {} bytes",
                                req.content_length, post_max_size
                            ),
                        );
                        return false;
                    }
                    // sapi_read_standard_form_data(): the body is kept in a
                    // php://temp stream of SAPI_POST_BLOCK_SIZE bytes, which
                    // spills into a temporary file.
                    if req.body.len() > SAPI_POST_BLOCK_SIZE {
                        let dir = ini("upload_tmp_dir")
                            .filter(|d| !d.is_empty())
                            .unwrap_or_else(|| crate::misc::php_get_temporary_directory(ctx.vm));
                        if !temp_file_possible(&dir) {
                            emit_startup(
                                ctx.vm,
                                "Warning",
                                "PHP Request Startup: Unable to create temporary file, Check permissions in temporary files directory.",
                            );
                            emit_startup(
                                ctx.vm,
                                "Warning",
                                "PHP Request Startup: POST data can't be buffered; all data discarded",
                            );
                            body_discarded.set(true);
                            return false;
                        }
                    }
                    true
                };
                if base == b"application/x-www-form-urlencoded" {
                    post_entry = Some("std");
                    buffered = std_reader(&mut ctx);
                } else if base == b"multipart/form-data" {
                    post_entry = Some("rfc1867");
                } else {
                    buffered = std_reader(&mut ctx);
                }
            }
        }

        // $_GET
        if has('G') {
            if !ctx
                .vm
                .treat_data_hook
                .is_some_and(|h| h(ctx.vm, 1, &req.query_string, &get))
            {
                treat_data(
                    &mut ctx,
                    PARSE_GET,
                    &req.query_string,
                    false,
                    &arg_sep,
                    max_vars,
                    &get,
                );
            }
        }
        // $_POST
        // php_auto_globals_create_post(): not once a startup error was
        // displayed (the headers went out with it).
        if has('P') && is_post && !ctx.vm.headers_sent {
            match post_entry {
                Some("std") if buffered => {
                    if !ctx
                        .vm
                        .treat_data_hook
                        .is_some_and(|h| h(ctx.vm, 0, &req.body, &post))
                    {
                        std_post_handler(&mut ctx, &req.body, max_vars, &post)
                    }
                }
                Some("rfc1867") => {
                    let cfg = UploadCfg {
                        max_file_uploads: ini("max_file_uploads")
                            .map(|v| parse_quantity(&v))
                            .unwrap_or(20),
                        max_body_parts: ini("max_multipart_body_parts")
                            .map(|v| parse_quantity(&v))
                            .unwrap_or(-1),
                        post_max_size,
                        max_input_vars: max_vars,
                        upload_max_filesize: ini("upload_max_filesize")
                            .map(|v| parse_quantity(&v))
                            .unwrap_or(2 * 1024 * 1024),
                        file_uploads: ini_bool(ini("file_uploads"), true),
                        upload_tmp_dir: ini("upload_tmp_dir"),
                        query_string: req.query_string.clone(),
                        cookie: req.cookie_data.clone().unwrap_or_default(),
                    };
                    let ct = req.content_type.clone().unwrap_or_default();
                    let used = rfc1867_handler(
                        &mut ctx,
                        &ct,
                        &req.body,
                        req.content_length,
                        &cfg,
                        &post,
                        &files,
                    );
                    // rfc1867 reads the SAPI body itself: php://input only
                    // gets what it left unread.
                    sapi_read = used.min(req.body.len());
                }
                _ => {}
            }
        }
        if buffered {
            // sapi_read_standard_form_data() buffered the whole body.
            input_body = req.body.clone();
            sapi_read = req.body.len();
        } else if body_discarded.get() {
            sapi_read = req.body.len();
        }
        // $_COOKIE
        if has('C') {
            if let Some(c) = &req.cookie_data {
                if !ctx
                    .vm
                    .treat_data_hook
                    .is_some_and(|h| h(ctx.vm, 2, c, &cookie))
                {
                    treat_data(&mut ctx, PARSE_COOKIE, c, true, b";", max_vars, &cookie);
                }
            }
        }
    }

    // $_ENV
    let env = new_arr();
    if has('E') {
        import_env(&req.env, &env);
    }
    // $_SERVER
    let server = new_arr();
    if has('S') {
        if req.server_vars.is_none() {
            import_env(&req.env, &server);
        }
        let mut s = server.borrow_mut();
        if let Some(vars) = &req.server_vars {
            let filter = INPUT_FILTER.with(|c| c.get());
            for (k, v) in vars {
                let fv = match (filter, req.server_var_filtered) {
                    (Some(f), Some(filtered)) if filtered(k) => {
                        RAW_INPUT.with(|r| {
                            r.borrow_mut()[PARSE_SERVER as usize]
                                .get_or_insert_with(new_arr)
                                .borrow_mut()
                                .set(sym_key(k), sval(v))
                        });
                        if v.is_empty() {
                            sval(b"")
                        } else {
                            f(vm, PARSE_SERVER, v)
                        }
                    }
                    _ => sval(v),
                };
                s.set(sym_key(k), fv);
            }
        } else if !req.cgi {
            // sapi_cli_register_variables(): these go through the input
            // filter (PARSE_SERVER).
            let php_self = req.php_self.as_ref().unwrap_or(&req.script_filename);
            let filter = INPUT_FILTER.with(|c| c.get());
            for (k, v) in [
                (&b"PHP_SELF"[..], php_self.as_slice()),
                (b"SCRIPT_NAME", php_self),
                (b"SCRIPT_FILENAME", &req.script_filename),
                (b"PATH_TRANSLATED", &req.script_filename),
                (b"DOCUMENT_ROOT", b""),
            ] {
                let fv = match filter {
                    Some(f) => {
                        RAW_INPUT.with(|r| {
                            r.borrow_mut()[PARSE_SERVER as usize]
                                .get_or_insert_with(new_arr)
                                .borrow_mut()
                                .set(sym_key(k), sval(v))
                        });
                        if v.is_empty() {
                            sval(b"")
                        } else {
                            f(vm, PARSE_SERVER, v)
                        }
                    }
                    None => sval(v),
                };
                s.set(sym_key(k), fv);
            }
        } else {
            // cgi.fix_pathinfo: PHP_SELF from SCRIPT_NAME / the script path
            let mut self_name = req
                .env
                .iter()
                .find(|(k, _)| k == b"SCRIPT_NAME")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| req.script_filename.clone());
            // (sapi_cgi_register_variables(): SCRIPT_NAME + PATH_INFO)
            if ini_bool(ini("cgi.fix_pathinfo"), true) {
                if let Some((_, pi)) = req.env.iter().find(|(k, _)| k == b"PATH_INFO") {
                    self_name.extend_from_slice(pi);
                }
            }
            s.set(sym_key(b"PHP_SELF"), sval(&self_name));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        s.set(sym_key(b"REQUEST_TIME_FLOAT"), Value::Double(now));
        s.set(sym_key(b"REQUEST_TIME"), Value::Long(now as i64));
    }
    // argv / argc (php_build_argv(), when register_argc_argv is on)
    if !req.cgi {
        if !ini_bool(ini("register_argc_argv"), true) {
            // (no $argv / $argc)
        } else {
            let argv = new_arr();
            for a in &req.argv {
                argv.borrow_mut().push(sval(a));
            }
            let argc = Value::Long(req.argv.len() as i64);
            vm.set_global(b"argv".to_vec(), Value::Array(argv.clone()));
            vm.set_global(b"argc".to_vec(), argc.clone());
            if has('S') {
                let copy = Rc::new(RefCell::new(argv.borrow().clone()));
                server
                    .borrow_mut()
                    .set(sym_key(b"argv"), Value::Array(copy));
                server.borrow_mut().set(sym_key(b"argc"), argc);
            }
        }
    } else if has('S') && ini_bool(ini("register_argc_argv"), false) {
        emit_startup(
            vm,
            "Deprecated",
            "Deriving $_SERVER['argv'] from the query string is deprecated. Configure register_argc_argv=0 to turn this message off",
        );
        let argv = new_arr();
        let qs = cstr(&req.query_string);
        let mut count = 0;
        if !qs.is_empty() {
            for part in qs.split(|&c| c == b'+') {
                argv.borrow_mut().push(sval(part));
                count += 1;
            }
        }
        server
            .borrow_mut()
            .set(sym_key(b"argv"), Value::Array(argv));
        server
            .borrow_mut()
            .set(sym_key(b"argc"), Value::Long(count));
    }
    // $_REQUEST
    let request = new_arr();
    let order = match vm.constants.get(b"request_order".as_slice()) {
        Some(v) if !matches!(v, Value::Null) => v.to_php_string().to_string_lossy(),
        _ => variables_order.clone(),
    };
    let mut done = [false; 3];
    for c in order.chars() {
        let (i, src) = match c.to_ascii_uppercase() {
            'G' => (0, &get),
            'P' => (1, &post),
            'C' => (2, &cookie),
            _ => continue,
        };
        if !done[i] {
            autoglobal_merge(&request, &src.borrow());
            done[i] = true;
        }
    }

    vm.set_global(b"_GET".to_vec(), Value::Array(get));
    vm.set_global(b"_POST".to_vec(), Value::Array(post));
    vm.set_global(b"_COOKIE".to_vec(), Value::Array(cookie));
    vm.set_global(b"_FILES".to_vec(), Value::Array(files));
    vm.set_global(b"_SERVER".to_vec(), Value::Array(server));
    vm.set_global(b"_ENV".to_vec(), Value::Array(env));
    vm.set_global(b"_REQUEST".to_vec(), Value::Array(request));
    crate::misc::set_request_body(input_body);
    crate::misc::set_sapi_body(req.body.clone(), sapi_read);
}

fn import_env(env: &[(Vec<u8>, Vec<u8>)], arr: &Rc<RefCell<PhpArray>>) {
    let mut a = arr.borrow_mut();
    for (k, v) in env {
        if k.is_empty() || k.iter().any(|&c| c == b' ' || c == b'.' || c == b'[') {
            continue;
        }
        a.set(sym_key(k), sval(v));
    }
}

fn autoglobal_merge(dest: &Rc<RefCell<PhpArray>>, src: &PhpArray) {
    for (k, v) in src.iter() {
        let existing = dest.borrow().get(k).cloned();
        match (v, existing) {
            (Value::Array(sa), Some(Value::Array(da))) => {
                let copy = Rc::new(RefCell::new(da.borrow().clone()));
                autoglobal_merge(&copy, &sa.borrow());
                dest.borrow_mut().set(k.clone(), Value::Array(copy));
            }
            (Value::Array(sa), _) => {
                let copy = Rc::new(RefCell::new(sa.borrow().clone()));
                dest.borrow_mut().set(k.clone(), Value::Array(copy));
            }
            _ => dest.borrow_mut().set(k.clone(), v.clone()),
        }
    }
}

/// Delete uploaded temp files that were not moved (destroy_uploaded_files_hash).
pub fn shutdown() {
    UPLOADED_FILES.with(|u| {
        for f in u.borrow_mut().drain(..) {
            let _ = std::fs::remove_file(String::from_utf8_lossy(&f).as_ref());
        }
    });
}

fn is_uploaded(path: &[u8]) -> bool {
    UPLOADED_FILES.with(|u| u.borrow().iter().any(|f| f == path))
}

fn is_uploaded_file_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let p = args
        .first()
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b""));
    Ok(if is_uploaded(p.as_bytes()) {
        Value::True
    } else {
        Value::False
    })
}

fn move_uploaded_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let from = args
        .first()
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b""));
    let to = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b""));
    if !is_uploaded(from.as_bytes()) {
        return Ok(Value::False);
    }
    let f = from.to_string_lossy();
    let t = to.to_string_lossy();
    let ok = std::fs::rename(&f, &t).is_ok()
        || (std::fs::copy(&f, &t).is_ok() && std::fs::remove_file(&f).is_ok());
    if ok {
        UPLOADED_FILES.with(|u| u.borrow_mut().retain(|x| x != from.as_bytes()));
        Ok(Value::True)
    } else {
        vm.emit_warning(&format!(
            "move_uploaded_file({}): Failed to open stream: Permission denied",
            t
        ));
        Ok(Value::False)
    }
}

pub fn register(vm: &mut Vm) {
    vm.register_function(b"is_uploaded_file", is_uploaded_file_fn);
    vm.register_function(b"move_uploaded_file", move_uploaded_file_fn);
}
