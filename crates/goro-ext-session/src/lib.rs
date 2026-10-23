//! ext/session, ported from php-src ext/session/session.c, mod_files.c,
//! mod_user.c and mod_user_class.c.

mod files;
mod rewrite;
mod upload;
mod var;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm, VmError};

pub(crate) type R<T> = Result<T, VmError>;

pub(crate) const E_ERROR: i64 = 1;
pub(crate) const E_WARNING: i64 = 2;
pub(crate) const E_NOTICE: i64 = 8;
pub(crate) const E_DEPRECATED: i64 = 8192;

const PS_MAX_SID_LENGTH: usize = 256;
const SESSION_FORBIDDEN_CHARS: &[u8] = b"=,;.[ \t\r\n\x0b\x0c";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    Disabled = 0,
    None = 1,
    Active = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Module {
    Files,
    User,
}

impl Module {
    fn name(self) -> &'static str {
        match self {
            Module::Files => "files",
            Module::User => "user",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Serializer {
    PhpSerialize,
    Php,
    PhpBinary,
}

fn find_module(name: &[u8]) -> Option<Module> {
    if name.eq_ignore_ascii_case(b"files") {
        Some(Module::Files)
    } else if name.eq_ignore_ascii_case(b"user") {
        Some(Module::User)
    } else {
        None
    }
}

fn find_serializer(name: &[u8]) -> Option<Serializer> {
    if name.eq_ignore_ascii_case(b"php_serialize") {
        Some(Serializer::PhpSerialize)
    } else if name.eq_ignore_ascii_case(b"php") {
        Some(Serializer::Php)
    } else if name.eq_ignore_ascii_case(b"php_binary") {
        Some(Serializer::PhpBinary)
    } else {
        None
    }
}

#[derive(Default, Clone)]
pub(crate) struct UserNames {
    open: Option<Value>,
    close: Option<Value>,
    read: Option<Value>,
    write: Option<Value>,
    destroy: Option<Value>,
    gc: Option<Value>,
    create_sid: Option<Value>,
    validate_sid: Option<Value>,
    update_timestamp: Option<Value>,
}

/// php_ps_globals
pub(crate) struct Ps {
    // INI-backed settings
    save_path: Vec<u8>,
    session_name: Vec<u8>,
    auto_start: bool,
    gc_probability: i64,
    gc_divisor: i64,
    gc_maxlifetime: i64,
    cookie_lifetime: i64,
    cookie_path: Vec<u8>,
    cookie_domain: Vec<u8>,
    cookie_secure: bool,
    cookie_partitioned: bool,
    cookie_httponly: bool,
    cookie_samesite: Vec<u8>,
    use_cookies: bool,
    use_only_cookies: bool,
    use_strict_mode: bool,
    extern_referer_chk: Vec<u8>,
    cache_limiter: Vec<u8>,
    cache_expire: i64,
    use_trans_sid: bool,
    sid_length: i64,
    sid_bits_per_character: i64,
    lazy_write: bool,
    rfc1867_enabled: bool,
    rfc1867_cleanup: bool,
    rfc1867_prefix: Vec<u8>,
    rfc1867_name: Vec<u8>,
    rfc1867_freq: i64,
    rfc1867_min_freq: f64,
    // Runtime state
    id: Option<Vec<u8>>,
    status: Status,
    module: Option<Module>,
    default_mod: Option<Module>,
    serializer: Option<Serializer>,
    pub(crate) mod_data: Option<files::FilesData>,
    mod_user_implemented: bool,
    mod_user_is_open: bool,
    mod_user_class_name: Option<Vec<u8>>,
    mod_user_names: UserNames,
    in_save_handler: bool,
    set_handler: bool,
    define_sid: bool,
    send_cookie: bool,
    session_vars: Option<Vec<u8>>,
    http_session_vars: Option<Rc<RefCell<Value>>>,
    started_at: Option<(String, u32)>,
    modules_activated: bool,
}

impl Default for Ps {
    fn default() -> Self {
        Ps {
            save_path: Vec::new(),
            session_name: b"PHPSESSID".to_vec(),
            auto_start: false,
            gc_probability: 1,
            gc_divisor: 100,
            gc_maxlifetime: 1440,
            cookie_lifetime: 0,
            cookie_path: b"/".to_vec(),
            cookie_domain: Vec::new(),
            cookie_secure: false,
            cookie_partitioned: false,
            cookie_httponly: false,
            cookie_samesite: Vec::new(),
            use_cookies: true,
            use_only_cookies: true,
            use_strict_mode: false,
            extern_referer_chk: Vec::new(),
            cache_limiter: b"nocache".to_vec(),
            cache_expire: 180,
            use_trans_sid: false,
            sid_length: 32,
            sid_bits_per_character: 4,
            lazy_write: true,
            rfc1867_enabled: true,
            rfc1867_cleanup: true,
            rfc1867_prefix: b"upload_progress_".to_vec(),
            rfc1867_name: b"PHP_SESSION_UPLOAD_PROGRESS".to_vec(),
            rfc1867_freq: -1,
            rfc1867_min_freq: 1.0,
            id: None,
            status: Status::None,
            module: Some(Module::Files),
            default_mod: None,
            serializer: Some(Serializer::Php),
            mod_data: None,
            mod_user_implemented: false,
            mod_user_is_open: false,
            mod_user_class_name: None,
            mod_user_names: UserNames::default(),
            in_save_handler: false,
            set_handler: false,
            define_sid: true,
            send_cookie: false,
            session_vars: None,
            http_session_vars: None,
            started_at: None,
            modules_activated: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Startup,
    RequestStartup,
    Runtime,
    RequestShutdown,
}

thread_local! {
    static PS: RefCell<Ps> = RefCell::new(Ps::default());
    /// Active internal function names (for php_error_docref prefixes),
    /// with the user file/line they were called from.
    static FN_STACK: RefCell<Vec<(String, goro_core::rcstr::RcStr, u32)>> = const { RefCell::new(Vec::new()) };
    static PHASE: Cell<Phase> = const { Cell::new(Phase::Startup) };
    static MODIFIED: RefCell<Vec<(Vec<u8>, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn ps<T>(f: impl FnOnce(&mut Ps) -> T) -> T {
    PS.with(|p| f(&mut p.borrow_mut()))
}

/// Marks an internal function as active for error messages.
pub(crate) struct FnGuard;

impl FnGuard {
    pub(crate) fn new(vm: &Vm, name: &str) -> FnGuard {
        FN_STACK.with(|s| {
            s.borrow_mut()
                .push((name.to_string(), vm.current_file.clone(), vm.current_line))
        });
        FnGuard
    }
}

impl Drop for FnGuard {
    fn drop(&mut self) {
        FN_STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

fn level_label(level: i64) -> &'static str {
    match level {
        E_ERROR => "Fatal error",
        E_WARNING => "Warning",
        E_NOTICE => "Notice",
        E_DEPRECATED => "Deprecated",
        _ => "Warning",
    }
}

/// php_error_docref() from inside the session module.
pub(crate) fn err(vm: &mut Vm, level: i64, msg: &str) {
    let top = FN_STACK.with(|s| s.borrow().last().cloned());
    match top {
        Some((fname, file, line)) => {
            let full = format!("{}(): {}", fname, msg);
            let (sf, sl) = (vm.current_file.clone(), vm.current_line);
            vm.current_file = file;
            vm.current_line = line;
            match level {
                E_NOTICE => vm.emit_notice_at(&full, line),
                E_DEPRECATED => vm.emit_deprecated_at(&full, line),
                E_ERROR => {
                    vm.emit_fatal_at(&full, line);
                }
                _ => vm.emit_warning_at(&full, line),
            }
            vm.current_file = sf;
            vm.current_line = sl;
        }
        None => {
            let origin = match PHASE.with(|p| p.get()) {
                Phase::Startup => "PHP Startup",
                Phase::RequestStartup => "PHP Request Startup",
                Phase::RequestShutdown => "PHP Request Shutdown",
                Phase::Runtime => "Unknown",
            };
            if origin == "PHP Startup" {
                let dse = vm
                    .constants
                    .get(b"display_startup_errors".as_slice())
                    .map(|v| {
                        let b = v.to_php_string();
                        ini_bool(b.as_bytes())
                            || b.as_bytes().eq_ignore_ascii_case(b"stderr")
                            || b.as_bytes().eq_ignore_ascii_case(b"stdout")
                    });
                if dse == Some(false) {
                    return;
                }
            }
            if vm.error_reporting & level != 0 || level == E_ERROR {
                let (hs, os) = (vm.headers_sent, vm.output_start.clone());
                vm.display_error(
                    level_label(level),
                    &format!("{}: {}", origin, msg),
                    "Unknown",
                    0,
                );
                if origin == "PHP Startup" {
                    // MINIT messages bypass the SAPI output layer.
                    vm.headers_sent = hs;
                    vm.output_start = os;
                }
            }
        }
    }
}

/// php_error() (no function prefix).
pub(crate) fn err_plain(vm: &mut Vm, level: i64, msg: &str) {
    let top = FN_STACK.with(|s| s.borrow().last().cloned());
    match top {
        Some((_, _, line)) => match level {
            E_NOTICE => vm.emit_notice_at(msg, line),
            _ => vm.emit_warning_at(msg, line),
        },
        None => {
            if vm.error_reporting & level != 0 {
                vm.display_error(level_label(level), msg, "Unknown", 0);
            }
        }
    }
}

/// EG(exception) is set (exit() unwinds like an exception).
pub(crate) fn pending(vm: &Vm) -> bool {
    vm.current_exception.is_some() || vm.exit_requested
}

pub(crate) fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    // The active session function appears in the exception's trace.
    let top = FN_STACK.with(|s| s.borrow().last().cloned());
    let mut pushed = false;
    let mut line = vm.current_line;
    if let Some((fname, file, l)) = top {
        line = l;
        if !vm
            .call_stack
            .last()
            .map(|f| f.0.eq_ignore_ascii_case(&fname))
            .unwrap_or(false)
        {
            let is_method = fname.contains("::");
            vm.call_stack.push((fname, file, l, Vec::new(), is_method));
            pushed = true;
        }
    }
    let exc = vm.create_exception(class, msg, line);
    if pushed {
        vm.call_stack.pop();
    }
    vm.current_exception = Some(exc);
}

fn s(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn headers_already_sent_error(vm: &mut Vm, level: i64, msg: &str) {
    match vm.output_start.clone() {
        Some((f, l)) => err(
            vm,
            level,
            &format!("{} (sent from {} on line {})", msg, f, l),
        ),
        None => err(vm, level, msg),
    }
}

fn already_started_error(vm: &mut Vm, level: i64, msg: &str) {
    let (at, auto) = ps(|p| (p.started_at.clone(), p.auto_start));
    match at {
        Some((f, l)) => err(
            vm,
            level,
            &format!("{} (started from {} on line {})", msg, f, l),
        ),
        None if auto => err(
            vm,
            level,
            &format!("{} (session started automatically)", msg),
        ),
        None => err(vm, level, msg),
    }
}

// ---------------------------------------------------------------------------
// INI handling
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Startup,
    Runtime,
    Deactivate,
}

const INI_ENTRIES: &[(&str, &str, bool)] = &[
    // (name, default, per-dir only)
    ("session.save_path", "", false),
    ("session.name", "PHPSESSID", false),
    ("session.save_handler", "files", false),
    ("session.auto_start", "0", true),
    ("session.gc_probability", "1", false),
    ("session.gc_divisor", "100", false),
    ("session.gc_maxlifetime", "1440", false),
    ("session.serialize_handler", "php", false),
    ("session.cookie_lifetime", "0", false),
    ("session.cookie_path", "/", false),
    ("session.cookie_domain", "", false),
    ("session.cookie_secure", "0", false),
    ("session.cookie_partitioned", "0", false),
    ("session.cookie_httponly", "0", false),
    ("session.cookie_samesite", "", false),
    ("session.use_cookies", "1", false),
    ("session.use_only_cookies", "1", false),
    ("session.use_strict_mode", "0", false),
    ("session.referer_check", "", false),
    ("session.cache_limiter", "nocache", false),
    ("session.cache_expire", "180", false),
    ("session.use_trans_sid", "0", false),
    ("session.sid_length", "32", false),
    ("session.sid_bits_per_character", "4", false),
    ("session.lazy_write", "1", false),
    ("session.upload_progress.enabled", "1", true),
    ("session.upload_progress.cleanup", "1", true),
    ("session.upload_progress.prefix", "upload_progress_", true),
    (
        "session.upload_progress.name",
        "PHP_SESSION_UPLOAD_PROGRESS",
        true,
    ),
    ("session.upload_progress.freq", "1%", true),
    ("session.upload_progress.min_freq", "1", true),
    // url_scanner_ex (ext/standard)
    (
        "session.trans_sid_tags",
        "a=href,area=href,frame=src,form=",
        false,
    ),
    ("session.trans_sid_hosts", "", false),
];

/// zend_ini_parse_bool
fn ini_bool(v: &[u8]) -> bool {
    if v.eq_ignore_ascii_case(b"true")
        || v.eq_ignore_ascii_case(b"yes")
        || v.eq_ignore_ascii_case(b"on")
    {
        true
    } else {
        atol(v) != 0
    }
}

/// ZEND_STRTOL(s, NULL, 10) / atol
pub(crate) fn atol(v: &[u8]) -> i64 {
    strtol(v, 10).0
}

/// strtol: returns (value, bytes consumed, overflow)
pub(crate) fn strtol(v: &[u8], base: u32) -> (i64, usize, bool) {
    let mut i = 0;
    while i < v.len() && (v[i] == b' ' || (b'\t'..=b'\r').contains(&v[i])) {
        i += 1;
    }
    let mut neg = false;
    if i < v.len() && (v[i] == b'+' || v[i] == b'-') {
        neg = v[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut acc: i128 = 0;
    let mut overflow = false;
    while i < v.len() {
        let d = match (v[i] as char).to_digit(base) {
            Some(d) => d as i128,
            None => break,
        };
        acc = acc * base as i128 + d;
        if acc > i64::MAX as i128 + 1 {
            overflow = true;
            acc = i64::MAX as i128 + 1;
        }
        i += 1;
    }
    if i == start {
        return (0, 0, false);
    }
    let mut r = if neg { -acc } else { acc };
    if r > i64::MAX as i128 {
        r = i64::MAX as i128;
        overflow = true;
    }
    if r < i64::MIN as i128 {
        r = i64::MIN as i128;
        overflow = true;
    }
    (r as i64, i, overflow)
}

fn is_numeric_str(v: &[u8]) -> bool {
    goro_core::value::parse_numeric_string(v).is_some()
}

fn check_active_and_output(vm: &mut Vm, stage: Stage) -> bool {
    if ps(|p| p.status) == Status::Active {
        already_started_error(
            vm,
            E_WARNING,
            "Session ini settings cannot be changed when a session is active",
        );
        return false;
    }
    if vm.headers_sent && stage != Stage::Deactivate {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session ini settings cannot be changed after headers have already been sent",
        );
        return false;
    }
    true
}

/// The PHP_INI_MH handlers. Returns whether the new value is accepted.
fn on_update(vm: &mut Vm, name: &str, value: &[u8], stage: Stage) -> bool {
    let key = &name["session.".len()..];
    match key {
        "save_path" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            if stage == Stage::Runtime {
                if value.contains(&0) {
                    return false;
                }
                let p = match value.iter().position(|&c| c == b';') {
                    Some(i) => {
                        let rest = &value[i + 1..];
                        match rest.iter().position(|&c| c == b';') {
                            Some(j) => &rest[j + 1..],
                            None => rest,
                        }
                    }
                    None => value,
                };
                if !p.is_empty() && !basedir_ok(vm, p) {
                    return false;
                }
            }
            ps(|p| p.save_path = value.to_vec());
            true
        }
        "name" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            if value.is_empty()
                || value.contains(&0)
                || is_numeric_str(value)
                || value.iter().any(|c| SESSION_FORBIDDEN_CHARS.contains(c))
            {
                if stage != Stage::Deactivate {
                    err(
                        vm,
                        E_WARNING,
                        &format!(
                            "session.name \"{}\" must not be numeric, empty, contain null bytes or any of the following characters \"=,;.[ \\t\\r\\n\\013\\014\"",
                            lossy(cstr(value))
                        ),
                    );
                }
                return false;
            }
            ps(|p| p.session_name = value.to_vec());
            true
        }
        "save_handler" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let tmp = find_module(cstr(value));
            let err_type = if stage == Stage::Runtime {
                E_WARNING
            } else {
                E_ERROR
            };
            let activated = ps(|p| p.modules_activated);
            if activated && tmp.is_none() {
                if stage != Stage::Deactivate {
                    err(
                        vm,
                        err_type,
                        &format!(
                            "Session save handler \"{}\" cannot be found",
                            lossy(cstr(value))
                        ),
                    );
                }
                return false;
            }
            if !ps(|p| p.set_handler) && tmp == Some(Module::User) {
                err(
                    vm,
                    err_type,
                    "Session save handler \"user\" cannot be set by ini_set()",
                );
                return false;
            }
            ps(|p| {
                p.default_mod = p.module;
                p.module = tmp;
            });
            true
        }
        "auto_start" => {
            ps(|p| p.auto_start = ini_bool(value));
            true
        }
        "gc_probability" | "gc_divisor" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let (v, warn) = goro_core::ini_quantity::parse_quantity(value, true);
            if let Some(w) = warn {
                err_plain(
                    vm,
                    E_WARNING,
                    &w.replace(
                        "Invalid quantity",
                        &format!("Invalid \"{}\" setting.", name),
                    ),
                );
            }
            let v = v as i64;
            if key == "gc_probability" {
                if v < 0 {
                    err(
                        vm,
                        E_WARNING,
                        "session.gc_probability must be greater than or equal to 0",
                    );
                    return false;
                }
                ps(|p| p.gc_probability = v);
            } else {
                if v <= 0 {
                    err(vm, E_WARNING, "session.gc_divisor must be greater than 0");
                    return false;
                }
                ps(|p| p.gc_divisor = v);
            }
            true
        }
        "gc_maxlifetime" | "cache_expire" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let v = atol(value);
            ps(|p| {
                if key == "gc_maxlifetime" {
                    p.gc_maxlifetime = v
                } else {
                    p.cache_expire = v
                }
            });
            true
        }
        "serialize_handler" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let tmp = find_serializer(cstr(value));
            if ps(|p| p.modules_activated) && tmp.is_none() {
                let et = if stage == Stage::Runtime {
                    E_WARNING
                } else {
                    E_ERROR
                };
                if stage != Stage::Deactivate {
                    err(
                        vm,
                        et,
                        &format!(
                            "Serialization handler \"{}\" cannot be found",
                            lossy(cstr(value))
                        ),
                    );
                }
                return false;
            }
            ps(|p| p.serializer = tmp);
            true
        }
        "cookie_lifetime" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let maxcookie: i64 = i64::MAX - i32::MAX as i64 - 1;
            let v = atol(value);
            if v < 0 {
                err(vm, E_WARNING, "CookieLifetime cannot be negative");
                return false;
            } else if v > maxcookie {
                return true;
            }
            ps(|p| p.cookie_lifetime = v);
            true
        }
        "cookie_path" | "cookie_domain" | "cookie_samesite" | "cache_limiter" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let v = value.to_vec();
            ps(|p| match key {
                "cookie_path" => p.cookie_path = v,
                "cookie_domain" => p.cookie_domain = v,
                "cookie_samesite" => p.cookie_samesite = v,
                _ => p.cache_limiter = v,
            });
            true
        }
        "cookie_secure" | "cookie_partitioned" | "cookie_httponly" | "use_cookies"
        | "use_strict_mode" | "lazy_write" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let b = ini_bool(value);
            ps(|p| match key {
                "cookie_secure" => p.cookie_secure = b,
                "cookie_partitioned" => p.cookie_partitioned = b,
                "cookie_httponly" => p.cookie_httponly = b,
                "use_cookies" => p.use_cookies = b,
                "use_strict_mode" => p.use_strict_mode = b,
                _ => p.lazy_write = b,
            });
            true
        }
        "use_only_cookies" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let b = ini_bool(value);
            ps(|p| p.use_only_cookies = b);
            if !b {
                err(
                    vm,
                    E_DEPRECATED,
                    "Disabling session.use_only_cookies INI setting is deprecated",
                );
            }
            true
        }
        "use_trans_sid" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let b = ini_bool(value);
            ps(|p| p.use_trans_sid = b);
            if b {
                err(
                    vm,
                    E_DEPRECATED,
                    "Enabling session.use_trans_sid INI setting is deprecated",
                );
            }
            true
        }
        "referer_check" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            if !value.is_empty() {
                err(
                    vm,
                    E_DEPRECATED,
                    "Usage of session.referer_check INI setting is deprecated",
                );
            }
            ps(|p| p.extern_referer_chk = value.to_vec());
            true
        }
        "sid_length" | "sid_bits_per_character" => {
            if !check_active_and_output(vm, stage) {
                return false;
            }
            let (val, used, _) = strtol(value, 10);
            let full = used == value.len() && !value.is_empty();
            let is_len = key == "sid_length";
            if val != if is_len { 32 } else { 4 } {
                err(
                    vm,
                    E_DEPRECATED,
                    &format!("session.{} INI setting is deprecated", key),
                );
            }
            let (lo, hi) = if is_len {
                (22, PS_MAX_SID_LENGTH as i64)
            } else {
                (4, 6)
            };
            if full && val >= lo && val <= hi {
                ps(|p| {
                    if is_len {
                        p.sid_length = val
                    } else {
                        p.sid_bits_per_character = val
                    }
                });
                return true;
            }
            err(
                vm,
                E_WARNING,
                &format!(
                    "session.configuration \"session.{}\" must be between {} and {}",
                    key, lo, hi
                ),
            );
            false
        }
        "upload_progress.enabled" => {
            ps(|p| p.rfc1867_enabled = ini_bool(value));
            true
        }
        "upload_progress.cleanup" => {
            ps(|p| p.rfc1867_cleanup = ini_bool(value));
            true
        }
        "upload_progress.prefix" => {
            ps(|p| p.rfc1867_prefix = value.to_vec());
            true
        }
        "upload_progress.name" => {
            ps(|p| p.rfc1867_name = value.to_vec());
            true
        }
        "upload_progress.freq" => {
            let tmp = atol(value);
            if tmp < 0 {
                err(
                    vm,
                    E_WARNING,
                    "session.upload_progress.freq must be greater than or equal to 0",
                );
                return false;
            }
            if value.last() == Some(&b'%') {
                if tmp > 100 {
                    err(
                        vm,
                        E_WARNING,
                        "session.upload_progress.freq must be less than or equal to 100%",
                    );
                    return false;
                }
                ps(|p| p.rfc1867_freq = -tmp);
            } else {
                ps(|p| p.rfc1867_freq = tmp);
            }
            true
        }
        "upload_progress.min_freq" => {
            let f = lossy(value).trim().parse::<f64>().unwrap_or(0.0);
            ps(|p| p.rfc1867_min_freq = f);
            true
        }
        "trans_sid_tags" | "trans_sid_hosts" => {
            rewrite::on_update_ini(vm, key, value, stage == Stage::Startup)
        }
        _ => true,
    }
}

fn cstr(v: &[u8]) -> &[u8] {
    match v.iter().position(|&c| c == 0) {
        Some(p) => &v[..p],
        None => v,
    }
}

pub(crate) fn basedir_ok(vm: &mut Vm, path: &[u8]) -> bool {
    let fname = FN_STACK
        .with(|s| s.borrow().last().map(|f| f.0.clone()))
        .unwrap_or_default();
    goro_core::basedir::check_open_basedir_ex(vm, &fname, &lossy(path), true)
}

fn ini_entry(name: &[u8]) -> Option<&'static (&'static str, &'static str, bool)> {
    INI_ENTRIES.iter().find(|e| e.0.as_bytes() == name)
}

fn ini_get_str(vm: &Vm, name: &str) -> Vec<u8> {
    vm.constants
        .get(name.as_bytes())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

/// zend_alter_ini_entry(name, value, PHP_INI_USER, PHP_INI_STAGE_RUNTIME)
fn alter_ini(vm: &mut Vm, name: &[u8], value: &[u8]) -> bool {
    let e = match ini_entry(name) {
        Some(e) => e,
        None => return false,
    };
    if e.2 {
        return false;
    }
    if !on_update(vm, e.0, value, Stage::Runtime) {
        return false;
    }
    record_modified(vm, name);
    vm.constants
        .insert(name.to_vec(), Value::String(PhpString::from_bytes(value)));
    true
}

/// EG(modified_ini_directives): remember the value to restore at request end.
fn record_modified(vm: &Vm, name: &[u8]) {
    let orig = vm
        .constants
        .get(name)
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    MODIFIED.with(|m| {
        let mut m = m.borrow_mut();
        if !m.iter().any(|(n, _)| n.as_slice() == name) {
            m.push((name.to_vec(), orig));
        }
    });
}

/// zend_ini_deactivate for the session entries.
fn ini_deactivate(vm: &mut Vm) {
    let mods = MODIFIED.with(|m| std::mem::take(&mut *m.borrow_mut()));
    for (name, orig) in mods {
        if let Some(e) = ini_entry(&name) {
            on_update(vm, e.0, &orig, Stage::Deactivate);
        }
        vm.constants
            .insert(name, Value::String(PhpString::from_vec(orig)));
    }
}

/// ini_set() hook for session.* entries.
fn ini_set_hook(vm: &mut Vm, key: &[u8], value: &Value) -> bool {
    let e = match ini_entry(key) {
        Some(e) => e,
        None => return true,
    };
    if e.2 {
        return false;
    }
    let v = value.to_php_string().as_bytes().to_vec();
    let _g = FnGuard::new(vm, "ini_set");
    let ok = on_update(vm, e.0, &v, Stage::Runtime);
    if ok {
        record_modified(vm, key);
    }
    ok
}

/// MINIT: REGISTER_INI_ENTRIES — run every entry's handler with the
/// configured value, in table order.
fn startup(vm: &mut Vm) {
    PHASE.with(|p| p.set(Phase::Startup));
    // Startup messages do not go through the SAPI output layer.
    let (hs, os) = (vm.headers_sent, vm.output_start.clone());
    vm.headers_sent = false;
    for (name, default, _) in INI_ENTRIES {
        let configured = vm
            .constants
            .get(name.as_bytes())
            .map(|v| v.to_php_string().as_bytes().to_vec());
        let val = configured
            .clone()
            .unwrap_or_else(|| default.as_bytes().to_vec());
        if !on_update(vm, name, &val, Stage::Startup) {
            // Keep the default when the configured value is rejected.
            on_update_silent(vm, name, default.as_bytes());
            vm.constants
                .insert(name.as_bytes().to_vec(), s(default.as_bytes()));
        }
    }
    ps(|p| p.modules_activated = true);
    PHASE.with(|p| p.set(Phase::Runtime));
    vm.headers_sent = hs;
    vm.output_start = os;
}

fn on_update_silent(vm: &mut Vm, name: &str, value: &[u8]) {
    let er = vm.error_reporting;
    vm.error_reporting = goro_core::errdisplay::ER_SILENT;
    on_update(vm, name, value, Stage::Startup);
    vm.error_reporting = er;
}

// ---------------------------------------------------------------------------
// Session variables
// ---------------------------------------------------------------------------

/// The $_SESSION array, if the session variables are set up (IF_SESSION_VARS).
fn session_vars_arr() -> Option<Rc<RefCell<PhpArray>>> {
    let cell = ps(|p| p.http_session_vars.clone())?;
    let v = cell.borrow();
    match &*v {
        Value::Array(a) => Some(a.clone()),
        _ => None,
    }
}

/// SEPARATE_ARRAY on $_SESSION: make sure we own the array we mutate.
fn session_vars_arr_mut() -> Option<Rc<RefCell<PhpArray>>> {
    let cell = ps(|p| p.http_session_vars.clone())?;
    let mut v = cell.borrow_mut();
    match &mut *v {
        Value::Array(a) => {
            if Rc::strong_count(a) > 1 {
                let copy = a.borrow().clone();
                *a = Rc::new(RefCell::new(copy));
            }
            Some(a.clone())
        }
        _ => None,
    }
}

/// php_session_track_init
fn track_init(vm: &mut Vm) {
    // The global's slot is reused: the previous session array is released now.
    let cell = vm.global_cell(b"_SESSION");
    let old = std::mem::replace(
        &mut *cell.borrow_mut(),
        Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
    );
    drop(old);
    ps(|p| p.http_session_vars = Some(cell));
}

fn set_session_vars(vm: &mut Vm, v: Value) {
    let cell = match ps(|p| p.http_session_vars.clone()) {
        Some(c) => c,
        None => {
            track_init(vm);
            ps(|p| p.http_session_vars.clone()).unwrap()
        }
    };
    *cell.borrow_mut() = v;
}

// ---------------------------------------------------------------------------
// Serializers
// ---------------------------------------------------------------------------

fn encode_loop(vm: &mut Vm) -> Option<Vec<(Vec<u8>, Value)>> {
    let arr = session_vars_arr()?;
    let entries: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut out = Vec::new();
    for (k, v) in entries {
        match k {
            ArrayKey::Int(n) => err(vm, E_WARNING, &format!("Skipping numeric key {}", n)),
            ArrayKey::String(s) => out.push((s.as_bytes().to_vec(), v)),
        }
    }
    Some(out)
}

/// php_session_encode
fn session_encode_data(vm: &mut Vm) -> Option<Vec<u8>> {
    use goro_ext_standard::serialize::{php_serialize, serialize_scope_hook};
    let cell_ok = session_vars_arr().is_some();
    if !cell_ok {
        err(vm, E_WARNING, "Cannot encode non-existent session");
        return None;
    }
    let ser = ps(|p| p.serializer).unwrap_or(Serializer::Php);
    // PHP_VAR_SERIALIZE_INIT: every variable shares one var_hash (with
    // serialize() calls made by Serializable::serialize() methods too).
    serialize_scope_hook(true);
    let buf = session_encode_vars(vm, ser);
    serialize_scope_hook(false);
    let buf = buf?;
    if vm.current_exception.is_some() || buf.is_empty() {
        return None;
    }
    return Some(buf);

    fn session_encode_vars(vm: &mut Vm, ser: Serializer) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        match ser {
            Serializer::PhpSerialize => {
                let arr = session_vars_arr()?;
                buf.extend(php_serialize(vm, &Value::Array(arr))?);
            }
            Serializer::Php => {
                for (key, v) in encode_loop(vm)? {
                    buf.extend_from_slice(&key);
                    if key.contains(&b'|') {
                        err(
                            vm,
                            E_WARNING,
                            &format!(
                                "Failed to write session data. Data contains invalid key \"{}\"",
                                lossy(cstr(&key))
                            ),
                        );
                        return None;
                    }
                    buf.push(b'|');
                    buf.extend(php_serialize(vm, &v)?);
                }
            }
            Serializer::PhpBinary => {
                for (key, v) in encode_loop(vm)? {
                    if key.len() > 127 {
                        continue;
                    }
                    buf.push(key.len() as u8);
                    buf.extend_from_slice(&key);
                    buf.extend(php_serialize(vm, &v)?);
                }
            }
        }
        Some(buf)
    }
}

/// Decode `data` with the active serializer. Returns false on failure.
fn serializer_decode(vm: &mut Vm, data: &[u8]) -> bool {
    use goro_ext_standard::serialize::UnserializeScope;
    let ser = ps(|p| p.serializer).unwrap_or(Serializer::Php);
    // A fatal error (in an autoloader) while decoding is a bailout.
    let was_fatal = vm.fatal_emitted;
    let st = UnserializeScope::begin(vm);
    let ok = match ser {
        Serializer::PhpSerialize => {
            let mut pos = 0;
            let r = st.value(vm, data, &mut pos);
            let v = match r {
                Some(i) => st.tmp(i).deref(),
                None => Value::Null,
            };
            let v = match v {
                Value::Array(_) => v,
                Value::Null => Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                other => other,
            };
            if ps(|p| p.http_session_vars.is_none()) {
                track_init(vm);
            }
            set_session_vars(vm, v);
            r.is_some() || data.is_empty()
        }
        Serializer::Php | Serializer::PhpBinary => {
            let mut pos = 0;
            let mut vars: Vec<(Vec<u8>, usize)> = Vec::new();
            let mut ok = true;
            while pos < data.len() {
                let name;
                if ser == Serializer::Php {
                    let q = match data[pos..].iter().position(|&c| c == b'|') {
                        Some(q) => pos + q,
                        None => {
                            ok = false;
                            break;
                        }
                    };
                    name = data[pos..q].to_vec();
                    pos = q + 1;
                } else {
                    let namelen = (data[pos] & 0x7f) as usize;
                    if pos + namelen >= data.len() {
                        ok = false;
                        break;
                    }
                    name = data[pos + 1..pos + 1 + namelen].to_vec();
                    pos += namelen + 1;
                }
                let Some(idx) = st.value(vm, data, &mut pos) else {
                    ok = false;
                    break;
                };
                vars.push((name.clone(), idx));
                // php_set_session_var: replaces (and frees) the old value now.
                if let Some(arr) = session_vars_arr_mut() {
                    let v = st.tmp(idx);
                    arr.borrow_mut()
                        .set(ArrayKey::String(PhpString::from_vec(name)), v);
                }
            }
            // php_session_normalize_vars: variables a later R: turned into references.
            if let Some(arr) = session_vars_arr_mut() {
                for (n, idx) in &vars {
                    if let v @ Value::Reference(_) = st.tmp(*idx) {
                        arr.borrow_mut()
                            .set(ArrayKey::String(PhpString::from_vec(n.clone())), v);
                    }
                }
            }
            ok
        }
    };
    st.end(vm);
    if vm.fatal_emitted && !was_fatal {
        var::BAILOUT.with(|b| b.set(true));
    }
    ok
}

/// php_session_decode
fn session_decode_data(vm: &mut Vm, data: &[u8]) -> R<bool> {
    let ok = serializer_decode(vm, data);
    if var::BAILOUT.with(|b| b.replace(false)) {
        // zend_catch: cancel the decode, then continue the bailout. The
        // bailout is caught while the session is destroyed, so the destroy
        // and close handlers run normally.
        let bailout = std::mem::replace(&mut vm.fatal_bailout, false);
        let _ = session_destroy_internal(vm);
        track_init(vm);
        vm.fatal_bailout |= bailout;
        if vm.error_reporting & E_WARNING != 0 {
            vm.display_error(
                "Warning",
                "Unknown: Failed to decode session object. Session has been destroyed",
                "Unknown",
                0,
            );
        }
        return Err(VmError {
            message: "bailout".into(),
            line: vm.current_line,
        });
    }
    if !ok {
        // php_session_cancel_decode
        session_destroy_internal(vm)?;
        track_init(vm);
        err(
            vm,
            E_WARNING,
            "Failed to decode session object. Session has been destroyed",
        );
        return Ok(false);
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Session IDs
// ---------------------------------------------------------------------------

const HEXCONVTAB: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ,-";

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    let mut filled = 0;
    while filled < n {
        let r = unsafe {
            libc::getrandom(
                buf[filled..].as_mut_ptr() as *mut libc::c_void,
                n - filled,
                0,
            )
        };
        if r <= 0 {
            break;
        }
        filled += r as usize;
    }
    buf
}

/// php_session_create_id
pub(crate) fn create_id() -> Vec<u8> {
    let (len, nbits) = ps(|p| (p.sid_length as usize, p.sid_bits_per_character as u32));
    let rbuf = random_bytes(len);
    let mut out = Vec::with_capacity(len);
    let mask = (1u32 << nbits) - 1;
    let mut w: u32 = 0;
    let mut have: u32 = 0;
    let mut p = 0;
    while out.len() < len {
        if have < nbits {
            if p < rbuf.len() {
                w |= (rbuf[p] as u32) << have;
                p += 1;
                have += 8;
            } else {
                break;
            }
        }
        out.push(HEXCONVTAB[(w & mask) as usize]);
        w >>= nbits;
        have -= nbits;
    }
    out
}

/// php_session_valid_key
pub(crate) fn valid_key(key: &[u8]) -> bool {
    let key = cstr(key);
    if key.is_empty() || key.len() > PS_MAX_SID_LENGTH {
        return false;
    }
    key.iter()
        .all(|&c| c.is_ascii_alphanumeric() || c == b',' || c == b'-')
}

// ---------------------------------------------------------------------------
// Save handler module dispatch
// ---------------------------------------------------------------------------

/// ps_call_handler: call a user save handler. Ok(None) = no return value.
fn call_handler(vm: &mut Vm, func: &Option<Value>, args: &[Value]) -> R<Option<Value>> {
    let func = match func {
        Some(f) => f.clone(),
        None => return Ok(None),
    };
    // zend_call_function() refuses to run with an exception pending.
    if pending(vm) {
        return Ok(None);
    }
    if ps(|p| p.in_save_handler) {
        ps(|p| p.in_save_handler = false);
        err(
            vm,
            E_WARNING,
            "Cannot call session save handler in a recursive manner",
        );
        return Ok(None);
    }
    ps(|p| p.in_save_handler = true);
    // Stack frames: the session function, then the handler called from it.
    let outer = FN_STACK.with(|s| s.borrow().last().cloned());
    let pushed_outer = outer.is_some();
    if let Some((fname, file, line)) = outer {
        let (name, is_method) = match fname.find("::") {
            Some(_) => (fname.clone(), true),
            None => (fname.clone(), false),
        };
        vm.call_stack
            .push((name, file, line, Vec::new(), is_method));
    }
    let (cname, is_method) = callable_frame_name(&func);
    vm.call_stack.push((
        cname,
        goro_core::rcstr::RcStr::new(),
        0,
        args.to_vec(),
        is_method,
    ));
    let r = vm.call_callback(&func, args);
    vm.call_stack.pop();
    if pushed_outer {
        vm.call_stack.pop();
    }
    let at_shutdown = PHASE.with(|p| p.get()) == Phase::RequestShutdown
        && FN_STACK.with(|s| s.borrow().is_empty());
    match r {
        Ok(v) if vm.current_exception.is_none() => {
            ps(|p| p.in_save_handler = false);
            Ok(if vm.exit_requested {
                None
            } else if matches!(v, Value::Undef) {
                Some(Value::Null)
            } else {
                Some(v.deref())
            })
        }
        Ok(_) | Err(_) if vm.current_exception.is_some() => {
            if at_shutdown {
                // No user frame left: the exception is fatal (bailout),
                // leaving the handler marked as running.
                let e = r.err().unwrap_or(VmError {
                    message: "Uncaught exception".into(),
                    line: 0,
                });
                vm.report_uncaught(&e);
                return Err(e);
            }
            ps(|p| p.in_save_handler = false);
            Ok(None)
        }
        Ok(_) => unreachable!(),
        // Fatal error (bailout): in_save_handler stays set, as in PHP.
        Err(e) => Err(e),
    }
}

fn callable_frame_name(f: &Value) -> (String, bool) {
    match f.deref() {
        Value::Array(a) => {
            let vals: Vec<Value> = a.borrow().values().cloned().collect();
            if vals.len() == 2 {
                let m = vals[1].to_php_string().to_string_lossy();
                match vals[0].deref() {
                    Value::Object(o) => {
                        let c = goro_core::value::display_class_name(&o.borrow().class_name);
                        return (format!("{}::{}", c, m), true);
                    }
                    other => {
                        return (
                            format!("{}::{}", other.to_php_string().to_string_lossy(), m),
                            false,
                        );
                    }
                }
            }
            ("{closure}".into(), false)
        }
        Value::String(s) => {
            let b = s.as_bytes();
            if b.starts_with(b"__closure_")
                || b.starts_with(b"__arrow_")
                || b.starts_with(b"__bound_closure_")
            {
                ("{closure}".into(), false)
            } else {
                (lossy(b), false)
            }
        }
        Value::Object(_) => ("{closure}".into(), false),
        _ => (String::new(), false),
    }
}

/// verify_bool_return_type_userland_calls
fn verify_bool(vm: &mut Vm, v: &Option<Value>) -> bool {
    let v = match v {
        None => return false,
        Some(v) => v,
    };
    match v {
        Value::True => true,
        Value::False => false,
        Value::Long(-1) | Value::Long(0) => {
            if !pending(vm) {
                err(
                    vm,
                    E_DEPRECATED,
                    "Session callback must have a return value of type bool, int returned",
                );
            }
            matches!(v, Value::Long(0))
        }
        other => {
            if !pending(vm) {
                let msg = format!(
                    "Session callback must have a return value of type bool, {} returned",
                    Vm::value_type_name(other)
                );
                throw(vm, b"TypeError", &msg);
            }
            false
        }
    }
}

fn names() -> UserNames {
    ps(|p| p.mod_user_names.clone())
}

pub(crate) fn mod_open(vm: &mut Vm, m: Module, save_path: &[u8], name: &[u8]) -> R<bool> {
    match m {
        Module::Files => Ok(files::open(vm, save_path)),
        Module::User => {
            let f = names().open;
            let r = call_handler(vm, &f, &[s(save_path), s(name)]);
            let r = match r {
                Ok(r) => r,
                Err(e) => {
                    ps(|p| p.status = Status::None);
                    return Err(e);
                }
            };
            ps(|p| p.mod_user_implemented = true);
            Ok(verify_bool(vm, &r))
        }
    }
}

pub(crate) fn mod_close(vm: &mut Vm, m: Module) -> R<bool> {
    match m {
        Module::Files => {
            files::close();
            Ok(true)
        }
        Module::User => {
            if !ps(|p| p.mod_user_implemented) {
                return Ok(true);
            }
            let f = names().close;
            let r = call_handler(vm, &f, &[]);
            ps(|p| p.mod_user_implemented = false);
            let r = r?;
            Ok(verify_bool(vm, &r))
        }
    }
}

pub(crate) fn mod_read(vm: &mut Vm, m: Module, key: &[u8]) -> R<Option<Vec<u8>>> {
    match m {
        Module::Files => Ok(files::read(vm, key)),
        Module::User => {
            let f = names().read;
            let r = call_handler(vm, &f, &[s(key)])?;
            Ok(match r {
                Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
                _ => None,
            })
        }
    }
}

pub(crate) fn mod_write(vm: &mut Vm, m: Module, key: &[u8], val: &[u8]) -> R<bool> {
    match m {
        Module::Files => Ok(files::write(vm, key, val)),
        Module::User => {
            let f = names().write;
            let r = call_handler(vm, &f, &[s(key), s(val)])?;
            Ok(verify_bool(vm, &r))
        }
    }
}

pub(crate) fn mod_destroy(vm: &mut Vm, m: Module, key: &[u8]) -> R<bool> {
    match m {
        Module::Files => Ok(files::destroy(key)),
        Module::User => {
            let f = names().destroy;
            let r = call_handler(vm, &f, &[s(key)])?;
            Ok(verify_bool(vm, &r))
        }
    }
}

/// Returns nrdels (-1 = failure).
pub(crate) fn mod_gc(vm: &mut Vm, m: Module, maxlifetime: i64) -> R<i64> {
    match m {
        Module::Files => Ok(files::gc(vm, maxlifetime)),
        Module::User => {
            let f = names().gc;
            let r = call_handler(vm, &f, &[Value::Long(maxlifetime)])?;
            Ok(match r {
                Some(Value::Long(n)) => n,
                Some(Value::True) => 1,
                _ => -1,
            })
        }
    }
}

pub(crate) fn mod_create_sid(vm: &mut Vm, m: Module) -> R<Option<Vec<u8>>> {
    match m {
        Module::Files => Ok(files::create_sid()),
        Module::User => {
            let f = names().create_sid;
            if f.is_some() {
                let r = call_handler(vm, &f, &[])?;
                return Ok(match r {
                    None => {
                        if !pending(vm) {
                            throw(vm, b"Error", "No session id returned by function");
                        }
                        None
                    }
                    Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
                    Some(_) => {
                        throw(vm, b"Error", "Session id must be a string");
                        None
                    }
                });
            }
            Ok(Some(create_id()))
        }
    }
}

/// Some(true) = valid (exists), Some(false) = invalid. None: module has no validator.
pub(crate) fn mod_validate_sid(vm: &mut Vm, m: Module, key: &[u8]) -> R<bool> {
    match m {
        Module::Files => Ok(files::key_exists(key)),
        Module::User => {
            let f = names().validate_sid;
            if f.is_some() {
                let r = call_handler(vm, &f, &[s(key)])?;
                return Ok(verify_bool(vm, &r));
            }
            Ok(true)
        }
    }
}

pub(crate) fn mod_update_timestamp(vm: &mut Vm, m: Module, key: &[u8], val: &[u8]) -> R<bool> {
    match m {
        Module::Files => Ok(files::update_timestamp(vm, key, val)),
        Module::User => {
            let n = names();
            let f = if n.update_timestamp.is_some() {
                n.update_timestamp
            } else {
                n.write
            };
            let r = call_handler(vm, &f, &[s(key), s(val)])?;
            Ok(verify_bool(vm, &r))
        }
    }
}

fn has_mod_data() -> bool {
    ps(|p| p.mod_data.is_some() || p.mod_user_implemented)
}

// ---------------------------------------------------------------------------
// Core session operations
// ---------------------------------------------------------------------------

/// php_rinit_session_globals
fn rinit_globals() {
    ps(|p| {
        p.id = None;
        p.status = Status::None;
        p.in_save_handler = false;
        p.set_handler = false;
        p.mod_data = None;
        p.mod_user_is_open = false;
        p.define_sid = true;
        p.session_vars = None;
        p.http_session_vars = None;
    });
}

/// php_rshutdown_session_globals
fn rshutdown_globals(vm: &mut Vm) -> R<()> {
    ps(|p| p.http_session_vars = None);
    let m = ps(|p| p.module);
    let r = if has_mod_data() {
        match m {
            Some(m) => mod_close(vm, m).map(|_| ()),
            None => Ok(()),
        }
    } else {
        Ok(())
    };
    ps(|p| {
        p.id = None;
        p.session_vars = None;
        p.mod_user_class_name = None;
        p.started_at = None;
        p.status = Status::None;
        p.mod_data = None;
    });
    r
}

/// php_session_destroy
fn session_destroy_internal(vm: &mut Vm) -> R<bool> {
    if ps(|p| p.status) != Status::Active {
        err(vm, E_WARNING, "Trying to destroy uninitialized session");
        return Ok(false);
    }
    let mut ret = true;
    let (id, m) = ps(|p| (p.id.clone(), p.module));
    if let (Some(id), Some(m)) = (id, m) {
        if !mod_destroy(vm, m, &id)? {
            ret = false;
            if !pending(vm) {
                err(vm, E_WARNING, "Session object destruction failed");
            }
        }
    }
    rshutdown_globals(vm)?;
    rinit_globals();
    Ok(ret)
}

/// php_session_abort
fn session_abort_internal(vm: &mut Vm) -> R<bool> {
    if ps(|p| p.status) == Status::Active {
        if has_mod_data() {
            if let Some(m) = ps(|p| p.module) {
                mod_close(vm, m)?;
            }
        }
        ps(|p| p.status = Status::None);
        return Ok(true);
    }
    Ok(false)
}

fn session_gc_internal(vm: &mut Vm, immediate: bool) -> R<i64> {
    let mut deleted = -1;
    if has_mod_data() {
        let (prob, div, maxl, m) =
            ps(|p| (p.gc_probability, p.gc_divisor, p.gc_maxlifetime, p.module));
        let mut collect = immediate;
        if !collect && prob > 0 {
            let r = u64::from_le_bytes(random_bytes(8).try_into().unwrap_or([0; 8]));
            collect = (r % (div.max(1) as u64)) < prob as u64;
        }
        if collect {
            if let Some(m) = m {
                deleted = mod_gc(vm, m, maxl)?;
            }
        }
    }
    Ok(deleted)
}

/// php_session_initialize
fn session_initialize(vm: &mut Vm) -> R<bool> {
    ps(|p| p.status = Status::Active);
    let m = match ps(|p| p.module) {
        Some(m) => m,
        None => {
            ps(|p| p.status = Status::Disabled);
            err(
                vm,
                E_WARNING,
                "No storage module chosen - failed to initialize session",
            );
            return Ok(false);
        }
    };
    let (save_path, name) = ps(|p| (p.save_path.clone(), p.session_name.clone()));
    if !mod_open(vm, m, &save_path, &name)? {
        session_abort_internal(vm)?;
        if !pending(vm) {
            err(
                vm,
                E_WARNING,
                &format!(
                    "Failed to initialize storage module: {} (path: {})",
                    m.name(),
                    lossy(&save_path)
                ),
            );
        }
        return Ok(false);
    }
    let id = ps(|p| p.id.clone());
    if id
        .as_ref()
        .map(|i| i.is_empty() || i[0] == 0)
        .unwrap_or(true)
    {
        let nid = mod_create_sid(vm, m)?;
        match nid {
            Some(n) => ps(|p| p.id = Some(n)),
            None => {
                ps(|p| p.id = None);
                session_abort_internal(vm)?;
                if !pending(vm) {
                    throw(
                        vm,
                        b"Error",
                        &format!(
                            "Failed to create session ID: {} (path: {})",
                            m.name(),
                            lossy(&save_path)
                        ),
                    );
                }
                return Ok(false);
            }
        }
        if ps(|p| p.use_cookies) {
            ps(|p| p.send_cookie = true);
        }
    } else if ps(|p| p.use_strict_mode) {
        let id = id.unwrap();
        if !mod_validate_sid(vm, m, &id)? {
            let nid = mod_create_sid(vm, m)?.unwrap_or_else(create_id);
            ps(|p| {
                p.id = Some(nid);
                if p.use_cookies {
                    p.send_cookie = true;
                }
            });
        }
    }
    if !session_reset_id(vm)? {
        session_abort_internal(vm)?;
        return Ok(false);
    }
    // Read data
    track_init(vm);
    let id = ps(|p| p.id.clone()).unwrap_or_default();
    let val = mod_read(vm, m, &id)?;
    let val = match val {
        Some(v) => v,
        None => {
            session_abort_internal(vm)?;
            if !pending(vm) {
                err(
                    vm,
                    E_WARNING,
                    &format!(
                        "Failed to read session data: {} (path: {})",
                        m.name(),
                        lossy(&save_path)
                    ),
                );
            }
            return Ok(false);
        }
    };
    // GC must be done after read
    session_gc_internal(vm, false)?;
    ps(|p| p.session_vars = None);
    if ps(|p| p.lazy_write) {
        ps(|p| p.session_vars = Some(val.clone()));
    }
    session_decode_data(vm, &val)?;
    let loc = FN_STACK.with(|s| s.borrow().last().map(|(_, f, l)| (f.to_string(), *l)));
    ps(|p| p.started_at = loc);
    Ok(true)
}

/// php_session_save_current_state
fn save_current_state(vm: &mut Vm, write: bool) -> R<()> {
    let m = ps(|p| p.module);
    if write && session_vars_arr().is_some() {
        let mut ret = false;
        let mut handler_fn = "write";
        let class_name = ps(|p| p.mod_user_class_name.clone());
        if has_mod_data() {
            let m = m.unwrap();
            let (id, maxl) = ps(|p| (p.id.clone().unwrap_or_default(), p.gc_maxlifetime));
            let _ = maxl;
            let val = session_encode_data(vm);
            match val {
                Some(val) => {
                    let same = ps(|p| p.lazy_write && p.session_vars.as_ref() == Some(&val));
                    if same {
                        ret = mod_update_timestamp(vm, m, &id, &val)?;
                        handler_fn = if class_name.is_some() {
                            "updateTimestamp"
                        } else {
                            "update_timestamp"
                        };
                    } else {
                        ret = mod_write(vm, m, &id, &val)?;
                    }
                }
                None => {
                    ret = mod_write(vm, m, &id, b"")?;
                }
            }
        }
        if !ret && !pending(vm) {
            let (user_impl, save_path) = ps(|p| (p.mod_user_implemented, p.save_path.clone()));
            if !user_impl {
                err(
                    vm,
                    E_WARNING,
                    &format!(
                        "Failed to write session data ({}). Please verify that the current setting of session.save_path is correct ({})",
                        m.map(|m| m.name()).unwrap_or(""),
                        lossy(&save_path)
                    ),
                );
            } else if let Some(cn) = class_name {
                err(
                    vm,
                    E_WARNING,
                    &format!(
                        "Failed to write session data using user defined save handler. (session.save_path: {}, handler: {}::{})",
                        lossy(&save_path),
                        lossy(&cn),
                        handler_fn
                    ),
                );
            } else {
                err(
                    vm,
                    E_WARNING,
                    &format!(
                        "Failed to write session data using user defined save handler. (session.save_path: {}, handler: {})",
                        lossy(&save_path),
                        handler_fn
                    ),
                );
            }
        }
    }
    if has_mod_data() {
        if let Some(m) = m {
            mod_close(vm, m)?;
        }
    }
    Ok(())
}

/// php_session_flush
fn session_flush(vm: &mut Vm, write: bool) -> R<bool> {
    if ps(|p| p.status) == Status::Active {
        save_current_state(vm, write)?;
        ps(|p| p.status = Status::None);
        return Ok(true);
    }
    Ok(false)
}

/// php_url_encode
fn url_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.' {
            out.push(c);
        } else if c == b' ' {
            out.push(b'+');
        } else {
            out.extend_from_slice(format!("%{:02X}", c).as_bytes());
        }
    }
    out
}

fn gmt_date(t: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let wd = DAYS[days.rem_euclid(7) as usize];
    // civil_from_days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        wd,
        d,
        MONTHS[(m - 1) as usize],
        y,
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// php_session_send_cookie
fn send_cookie(vm: &mut Vm) -> bool {
    if vm.headers_sent {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session cookie cannot be sent after headers have already been sent",
        );
        return false;
    }
    let p = PS.with(|p| {
        let p = p.borrow();
        (
            p.cookie_partitioned,
            p.cookie_secure,
            p.session_name.clone(),
            p.id.clone().unwrap_or_default(),
            p.cookie_lifetime,
            p.cookie_path.clone(),
            p.cookie_domain.clone(),
            p.cookie_httponly,
            p.cookie_samesite.clone(),
        )
    });
    let (partitioned, secure, name, id, lifetime, path, domain, httponly, samesite) = p;
    if partitioned && !secure {
        err(
            vm,
            E_WARNING,
            "Partitioned session cookie cannot be used without also configuring it as secure",
        );
        return false;
    }
    let mut c = Vec::new();
    c.extend_from_slice(b"Set-Cookie: ");
    c.extend_from_slice(&name);
    c.push(b'=');
    c.extend_from_slice(&url_encode(&id));
    if lifetime > 0 {
        let t = now() + lifetime;
        if t > 0 {
            c.extend_from_slice(b"; expires=");
            c.extend_from_slice(gmt_date(t).as_bytes());
            c.extend_from_slice(b"; Max-Age=");
            c.extend_from_slice(lifetime.to_string().as_bytes());
        }
    }
    if !path.is_empty() {
        c.extend_from_slice(b"; path=");
        c.extend_from_slice(&path);
    }
    if !domain.is_empty() {
        c.extend_from_slice(b"; domain=");
        c.extend_from_slice(&domain);
    }
    if secure {
        c.extend_from_slice(b"; secure");
    }
    if partitioned {
        c.extend_from_slice(b"; Partitioned");
    }
    if httponly {
        c.extend_from_slice(b"; HttpOnly");
    }
    if !samesite.is_empty() {
        c.extend_from_slice(b"; SameSite=");
        c.extend_from_slice(&samesite);
    }
    let mut prefix = name.clone();
    prefix.push(b'=');
    goro_ext_standard::misc::sapi_delete_header_prefix(b"Set-Cookie", &prefix);
    goro_ext_standard::misc::sapi_add_header(&c, false);
    true
}

fn cookie_has_session_name(vm: &Vm, name: &[u8]) -> bool {
    match vm.get_global(b"_COOKIE").map(|v| v.deref()) {
        Some(Value::Array(a)) => a
            .borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(name)))
            .is_some(),
        _ => false,
    }
}

/// php_session_reset_id
fn session_reset_id(vm: &mut Vm) -> R<bool> {
    if ps(|p| p.id.is_none()) {
        err(
            vm,
            E_WARNING,
            "Cannot set session ID - session ID is not initialized",
        );
        return Ok(false);
    }
    if ps(|p| p.use_cookies && p.send_cookie) {
        if !send_cookie(vm) {
            return Ok(false);
        }
        ps(|p| p.send_cookie = false);
    }
    let (define_sid, name, id) = ps(|p| {
        (
            p.define_sid,
            p.session_name.clone(),
            p.id.clone().unwrap_or_default(),
        )
    });
    let sid = if define_sid {
        let mut v = name.clone();
        v.push(b'=');
        v.extend_from_slice(&id);
        v
    } else {
        Vec::new()
    };
    if !vm.constants.contains_key(b"SID".as_slice()) {
        vm.internal_deprecated_constants.insert(
            b"SID".to_vec(),
            "Constant SID is deprecated since 8.4, as GET/POST sessions were deprecated"
                .to_string(),
        );
    }
    vm.constants
        .insert(b"SID".to_vec(), Value::String(PhpString::from_vec(sid)));

    let (use_trans_sid, use_only_cookies, use_cookies) =
        ps(|p| (p.use_trans_sid, p.use_only_cookies, p.use_cookies));
    let mut apply = false;
    if use_trans_sid && !use_only_cookies {
        apply = true;
        if use_cookies && cookie_has_session_name(vm, &name) {
            apply = false;
        }
    }
    if apply {
        rewrite::reset_session_var(vm, &name);
        rewrite::add_session_var(vm, &name, &id);
    }
    Ok(true)
}

fn try_find_id_in_global(vm: &Vm, global: &[u8]) {
    if ps(|p| p.id.is_some()) {
        return;
    }
    let name = ps(|p| p.session_name.clone());
    if let Some(Value::Array(a)) = vm.get_global(global).map(|v| v.deref()) {
        if let Some(v) = a.borrow().get(&ArrayKey::String(PhpString::from_vec(name))) {
            proposed_id(v);
        }
    }
}

fn proposed_id(v: &Value) {
    match v.deref() {
        Value::String(s) => ps(|p| {
            p.id = Some(s.as_bytes().to_vec());
            p.send_cookie = false;
        }),
        _ => ps(|p| {
            p.id = None;
            p.send_cookie = true;
        }),
    }
}

/// php_session_start
fn session_start_internal(vm: &mut Vm) -> R<bool> {
    match ps(|p| p.status) {
        Status::Active => {
            already_started_error(
                vm,
                E_NOTICE,
                "Ignoring session_start() because a session has already been started",
            );
            return Ok(false);
        }
        Status::Disabled => {
            let value = ini_get_str(vm, "session.save_handler");
            if ps(|p| p.module.is_none()) {
                match find_module(&value) {
                    Some(m) => ps(|p| p.module = Some(m)),
                    None => {
                        err(
                            vm,
                            E_WARNING,
                            &format!(
                                "Cannot find session save handler \"{}\" - session startup failed",
                                lossy(&value)
                            ),
                        );
                        return Ok(false);
                    }
                }
            }
            let value = ini_get_str(vm, "session.serialize_handler");
            if ps(|p| p.serializer.is_none()) {
                match find_serializer(&value) {
                    Some(s) => ps(|p| p.serializer = Some(s)),
                    None => {
                        err(
                            vm,
                            E_WARNING,
                            &format!(
                                "Cannot find session serialization handler \"{}\" - session startup failed",
                                lossy(&value)
                            ),
                        );
                        return Ok(false);
                    }
                }
            }
            ps(|p| p.status = Status::None);
            ps(|p| {
                p.define_sid = !p.use_only_cookies;
                p.send_cookie = p.use_cookies || p.use_only_cookies;
            });
        }
        Status::None => ps(|p| {
            p.define_sid = !p.use_only_cookies;
            p.send_cookie = p.use_cookies || p.use_only_cookies;
        }),
    }

    if ps(|p| p.id.is_none()) {
        // try_find_session_id_in_cookies
        if ps(|p| p.use_cookies) {
            let name = ps(|p| p.session_name.clone());
            if let Some(Value::Array(a)) = vm.get_global(b"_COOKIE").map(|v| v.deref()) {
                let v = a
                    .borrow()
                    .get(&ArrayKey::String(PhpString::from_vec(name)))
                    .cloned();
                if let Some(v) = v {
                    proposed_id(&v);
                    ps(|p| {
                        p.send_cookie = false;
                        p.define_sid = false;
                    });
                }
            }
        }
        if !ps(|p| p.use_only_cookies) {
            try_find_id_in_global(vm, b"_GET");
            try_find_id_in_global(vm, b"_POST");
            // should_invalidate_session_for_external_referer
            let chk = ps(|p| p.extern_referer_chk.clone());
            if ps(|p| p.id.is_some()) && !chk.is_empty() {
                if let Some(Value::Array(srv)) = vm.get_global(b"_SERVER").map(|v| v.deref()) {
                    let r = srv
                        .borrow()
                        .get(&ArrayKey::String(PhpString::from_bytes(b"HTTP_REFERER")))
                        .cloned();
                    if let Some(Value::String(r)) = r {
                        if !r.is_empty()
                            && !r.as_bytes().windows(chk.len()).any(|w| w == chk.as_slice())
                        {
                            ps(|p| p.id = None);
                        }
                    }
                }
            }
        }
    }
    // Check session id for dangerous characters
    if ps(|p| {
        p.id.as_ref()
            .map(|id| cstr(id).iter().any(|c| b"\r\n\t <>'\"\\".contains(c)))
            .unwrap_or(false)
    }) {
        ps(|p| p.id = None);
    }
    let ok = session_initialize(vm)?;
    if !ok || cache_limiter(vm)? == -2 {
        ps(|p| {
            p.status = Status::None;
            p.id = None;
        });
        return Ok(false);
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Cache limiters
// ---------------------------------------------------------------------------

fn last_modified(vm: &Vm) {
    let path = vm.current_file.to_string();
    let main_path = vm.get_global(b"_SERVER").and_then(|s| match s.deref() {
        Value::Array(a) => a
            .borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(b"PATH_TRANSLATED")))
            .map(|v| v.to_php_string().to_string_lossy()),
        _ => None,
    });
    let p = main_path.unwrap_or(path);
    if let Ok(md) = std::fs::metadata(&p) {
        use std::os::unix::fs::MetadataExt;
        let line = format!("Last-Modified: {}", gmt_date(md.mtime()));
        goro_ext_standard::misc::sapi_add_header(line.as_bytes(), true);
    }
}

/// php_session_cache_limiter: 0 ok, -1 unknown/inactive, -2 headers sent
fn cache_limiter(vm: &mut Vm) -> R<i32> {
    let (lim, status, expire) = ps(|p| (p.cache_limiter.clone(), p.status, p.cache_expire));
    if lim.is_empty() {
        return Ok(0);
    }
    if status != Status::Active {
        return Ok(-1);
    }
    if vm.headers_sent {
        session_abort_internal(vm)?;
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session cache limiter cannot be sent after headers have already been sent",
        );
        return Ok(-2);
    }
    let add = |l: &str| goro_ext_standard::misc::sapi_add_header(l.as_bytes(), true);
    let l = lim.to_ascii_lowercase();
    match l.as_slice() {
        b"public" => {
            add(&format!("Expires: {}", gmt_date(now() + expire * 60)));
            add(&format!("Cache-Control: public, max-age={}", expire * 60));
            last_modified(vm);
        }
        b"private" => {
            add("Expires: Thu, 19 Nov 1981 08:52:00 GMT");
            add(&format!("Cache-Control: private, max-age={}", expire * 60));
            last_modified(vm);
        }
        b"private_no_expire" => {
            add(&format!("Cache-Control: private, max-age={}", expire * 60));
            last_modified(vm);
        }
        b"nocache" => {
            add("Expires: Thu, 19 Nov 1981 08:52:00 GMT");
            add("Cache-Control: no-store, no-cache, must-revalidate");
            add("Pragma: no-cache");
        }
        _ => return Ok(-1),
    }
    Ok(0)
}

// ---------------------------------------------------------------------------
// Argument parsing helpers (zpp)
// ---------------------------------------------------------------------------

fn arg_count_error(vm: &mut Vm, fname: &str, min: usize, max: usize, given: usize) {
    let (kind, n) = if given < min {
        (if min == max { "exactly" } else { "at least" }, min)
    } else {
        (if min == max { "exactly" } else { "at most" }, max)
    };
    let msg = format!(
        "{}() expects {} {} argument{}, {} given",
        fname,
        kind,
        n,
        if n == 1 { "" } else { "s" },
        given
    );
    throw(vm, b"ArgumentCountError", &msg);
}

fn check_args(vm: &mut Vm, fname: &str, args: &[Value], min: usize, max: usize) -> bool {
    if args.len() < min || args.len() > max {
        arg_count_error(vm, fname, min, max, args.len());
        return false;
    }
    true
}

fn type_err(vm: &mut Vm, fname: &str, n: usize, pname: &str, ty: &str, v: &Value) {
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type {}, {} given",
        fname,
        n,
        pname,
        ty,
        zend_zval_type_name(v)
    );
    throw(vm, b"TypeError", &msg);
}

fn zend_zval_type_name(v: &Value) -> String {
    match v.deref() {
        Value::Null | Value::Undef => "null".into(),
        Value::True | Value::False => "bool".into(),
        Value::Long(_) => "int".into(),
        Value::Double(_) => "float".into(),
        Value::String(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(o) => goro_core::value::display_class_name(&o.borrow().class_name),
        _ => "resource".into(),
    }
}

/// Object to string via __toString, if any.
fn obj_to_string(vm: &mut Vm, v: &Value) -> Option<Vec<u8>> {
    if let Value::Object(o) = v {
        let cl = o.borrow().class_name.to_ascii_lowercase();
        if vm
            .classes
            .get(&cl)
            .map(|c| c.get_method(b"__tostring").is_some())
            .unwrap_or(false)
        {
            let r = vm.call_object_method(v, b"__toString", &[])?;
            return Some(r.to_php_string().as_bytes().to_vec());
        }
    }
    None
}

/// Z_PARAM_STR / P (path: no NUL) with optional nullability.
/// Ok(None) = null passed to nullable. Err(()) = exception thrown.
fn str_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    i: usize,
    pname: &str,
    nullable: bool,
    path: bool,
) -> Result<Option<Vec<u8>>, ()> {
    let v = match args.get(i) {
        Some(v) => v.deref(),
        None => return Ok(None),
    };
    let ty = if nullable { "?string" } else { "string" };
    let b = match &v {
        Value::Null | Value::Undef => {
            if nullable {
                return Ok(None);
            }
            if vm.caller_strict_types {
                type_err(vm, fname, i + 1, pname, ty, &v);
                return Err(());
            }
            err_deprecated_null(vm, fname, i + 1, pname, "string");
            Vec::new()
        }
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            if vm.caller_strict_types {
                type_err(vm, fname, i + 1, pname, ty, &v);
                return Err(());
            }
            v.to_php_string().as_bytes().to_vec()
        }
        Value::Object(_) => match obj_to_string(vm, &v) {
            Some(s) => s,
            None => {
                type_err(vm, fname, i + 1, pname, ty, &v);
                return Err(());
            }
        },
        _ => {
            type_err(vm, fname, i + 1, pname, ty, &v);
            return Err(());
        }
    };
    if path && b.contains(&0) {
        let msg = format!(
            "{}(): Argument #{} (${}) must not contain any null bytes",
            fname,
            i + 1,
            pname
        );
        throw(vm, b"ValueError", &msg);
        return Err(());
    }
    Ok(Some(b))
}

fn err_deprecated_null(vm: &mut Vm, fname: &str, n: usize, pname: &str, ty: &str) {
    let msg = format!(
        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
        fname, n, pname, ty
    );
    vm.emit_deprecated(&msg);
}

fn long_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    i: usize,
    pname: &str,
    nullable: bool,
) -> Result<Option<i64>, ()> {
    let v = match args.get(i) {
        Some(v) => v.deref(),
        None => return Ok(None),
    };
    let ty = if nullable { "?int" } else { "int" };
    match &v {
        Value::Null | Value::Undef if nullable => Ok(None),
        Value::Long(n) => Ok(Some(*n)),
        Value::True => Ok(Some(1)),
        Value::False => Ok(Some(0)),
        Value::Double(f) => {
            if f.is_finite() && f.fract() == 0.0 {
                Ok(Some(*f as i64))
            } else if f.is_finite() {
                let msg = format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_core::value::format_php_float_repr(*f)
                );
                vm.emit_deprecated(&msg);
                Ok(Some(*f as i64))
            } else {
                type_err(vm, fname, i + 1, pname, ty, &v);
                Err(())
            }
        }
        Value::String(s) => match goro_core::value::parse_numeric_string(s.as_bytes()) {
            Some(f) if f.fract() == 0.0 => Ok(Some(f as i64)),
            _ => {
                type_err(vm, fname, i + 1, pname, ty, &v);
                Err(())
            }
        },
        Value::Null | Value::Undef => {
            err_deprecated_null(vm, fname, i + 1, pname, "int");
            Ok(Some(0))
        }
        _ => {
            type_err(vm, fname, i + 1, pname, ty, &v);
            Err(())
        }
    }
}

fn bool_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    i: usize,
    pname: &str,
    nullable: bool,
) -> Result<Option<bool>, ()> {
    let v = match args.get(i) {
        Some(v) => v.deref(),
        None => return Ok(None),
    };
    let ty = if nullable { "?bool" } else { "bool" };
    match &v {
        Value::Null | Value::Undef if nullable => Ok(None),
        Value::Null | Value::Undef => {
            err_deprecated_null(vm, fname, i + 1, pname, "bool");
            Ok(Some(false))
        }
        Value::Array(_) | Value::Object(_) => {
            type_err(vm, fname, i + 1, pname, ty, &v);
            Err(())
        }
        _ => Ok(Some(v.is_truthy())),
    }
}

macro_rules! tryarg {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(()) => return Ok(Value::Null),
        }
    };
}

fn b(v: bool) -> Value {
    if v { Value::True } else { Value::False }
}

// ---------------------------------------------------------------------------
// PHP functions
// ---------------------------------------------------------------------------

fn php_can_change_session_setting(vm: &mut Vm, setting: &str, check_cookies: bool) -> bool {
    if ps(|p| p.status) == Status::Active {
        already_started_error(
            vm,
            E_WARNING,
            &format!(
                "Session {} cannot be changed when a session is active",
                setting
            ),
        );
        return false;
    }
    if vm.headers_sent && (!check_cookies || ps(|p| p.use_cookies)) {
        headers_already_sent_error(
            vm,
            E_WARNING,
            &format!(
                "Session {} cannot be changed after headers have already been sent",
                setting
            ),
        );
        return false;
    }
    true
}

fn f_session_set_cookie_params(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_set_cookie_params";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 1, 5) {
        return Ok(Value::Null);
    }
    let first = args[0].deref();
    let options = match &first {
        Value::Array(a) => Some(a.clone()),
        _ => None,
    };
    let mut lifetime: Option<Vec<u8>> = None;
    if options.is_none() {
        let l = tryarg!(long_arg_strict_arr(vm, fname, &first));
        lifetime = Some(l.to_string().into_bytes());
    }
    let mut path = tryarg!(str_arg(vm, fname, args, 1, "path", true, false));
    let mut domain = tryarg!(str_arg(vm, fname, args, 2, "domain", true, false));
    let mut secure = tryarg!(bool_arg(vm, fname, args, 3, "secure", true));
    let mut httponly = tryarg!(bool_arg(vm, fname, args, 4, "httponly", true));
    let mut partitioned: Option<bool> = None;
    let mut samesite: Option<Vec<u8>> = None;

    if !ps(|p| p.use_cookies) {
        err(
            vm,
            E_WARNING,
            "Session cookies cannot be used when session.use_cookies is disabled",
        );
        return Ok(Value::False);
    }
    if ps(|p| p.status) == Status::Active {
        already_started_error(
            vm,
            E_WARNING,
            "Session cookie parameters cannot be changed when a session is active",
        );
        return Ok(Value::False);
    }
    if vm.headers_sent {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session cookie parameters cannot be changed after headers have already been sent",
        );
        return Ok(Value::False);
    }
    if let Some(opts) = options {
        let bad = [
            (path.is_some(), 2),
            (domain.is_some(), 3),
            (secure.is_some(), 4),
            (httponly.is_some(), 5),
        ];
        for (is_set, n) in bad {
            if is_set {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #{} (${}) must be null when argument #1 ($lifetime_or_options) is an array",
                        fname,
                        n,
                        ["", "", "path", "domain", "secure", "httponly"][n]
                    ),
                );
                return Ok(Value::Null);
            }
        }
        let mut found = 0;
        let entries: Vec<(ArrayKey, Value)> = opts
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in entries {
            let v = v.deref();
            match k {
                ArrayKey::String(key) => {
                    let kl = key.as_bytes().to_ascii_lowercase();
                    let sval = |vm: &mut Vm, v: &Value| -> Vec<u8> {
                        match v {
                            Value::Object(_) => obj_to_string(vm, v).unwrap_or_default(),
                            _ => v.to_php_string().as_bytes().to_vec(),
                        }
                    };
                    match kl.as_slice() {
                        b"lifetime" => {
                            lifetime = Some(sval(vm, &v));
                            found += 1;
                        }
                        b"path" => {
                            path = Some(sval(vm, &v));
                            found += 1;
                        }
                        b"domain" => {
                            domain = Some(sval(vm, &v));
                            found += 1;
                        }
                        b"secure" => {
                            secure = Some(v.is_truthy());
                            found += 1;
                        }
                        b"partitioned" => {
                            partitioned = Some(v.is_truthy());
                            found += 1;
                        }
                        b"httponly" => {
                            httponly = Some(v.is_truthy());
                            found += 1;
                        }
                        b"samesite" => {
                            samesite = Some(sval(vm, &v));
                            found += 1;
                        }
                        _ => err(
                            vm,
                            E_WARNING,
                            &format!(
                                "Argument #1 ($lifetime_or_options) contains an unrecognized key \"{}\"",
                                lossy(key.as_bytes())
                            ),
                        ),
                    }
                }
                ArrayKey::Int(_) => err(
                    vm,
                    E_WARNING,
                    "Argument #1 ($lifetime_or_options) cannot contain numeric keys",
                ),
            }
        }
        if found == 0 {
            throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #1 ($lifetime_or_options) must contain at least 1 valid key",
                    fname
                ),
            );
            return Ok(Value::Null);
        }
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let bs = |b: bool| if b { b"1".to_vec() } else { b"0".to_vec() };
    let items: Vec<(&[u8], Option<Vec<u8>>)> = vec![
        (b"session.cookie_lifetime", lifetime),
        (b"session.cookie_path", path),
        (b"session.cookie_domain", domain),
        (b"session.cookie_secure", secure.map(bs)),
        (b"session.cookie_partitioned", partitioned.map(bs)),
        (b"session.cookie_httponly", httponly.map(bs)),
        (b"session.cookie_samesite", samesite),
    ];
    for (k, v) in items {
        if let Some(v) = v {
            if !alter_ini(vm, k, &v) {
                return Ok(Value::False);
            }
        }
    }
    Ok(Value::True)
}

fn long_arg_strict_arr(vm: &mut Vm, fname: &str, v: &Value) -> Result<i64, ()> {
    let tmp = [v.clone()];
    match long_arg(vm, fname, &tmp, 0, "lifetime_or_options", false) {
        Ok(Some(n)) => Ok(n),
        Ok(None) => Ok(0),
        Err(()) => {
            // Fix the type in the message: array|int
            if let Some(Value::Object(o)) = &vm.current_exception {
                let msg = format!(
                    "{}(): Argument #1 ($lifetime_or_options) must be of type array|int, {} given",
                    fname,
                    zend_zval_type_name(v)
                );
                o.borrow_mut().set_property(
                    b"message".to_vec(),
                    Value::String(PhpString::from_string(msg)),
                );
            }
            Err(())
        }
    }
}

fn f_session_get_cookie_params(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_get_cookie_params";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    let mut arr = PhpArray::new();
    PS.with(|p| {
        let p = p.borrow();
        let k = |s: &str| ArrayKey::String(PhpString::from_string(s.to_string()));
        arr.set(k("lifetime"), Value::Long(p.cookie_lifetime));
        arr.set(k("path"), s(&p.cookie_path));
        arr.set(k("domain"), s(&p.cookie_domain));
        arr.set(k("secure"), b(p.cookie_secure));
        arr.set(k("partitioned"), b(p.cookie_partitioned));
        arr.set(k("httponly"), b(p.cookie_httponly));
        arr.set(k("samesite"), s(&p.cookie_samesite));
    });
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn f_session_name(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_name";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let name = tryarg!(str_arg(vm, fname, args, 0, "name", true, true));
    if name.is_some() && !php_can_change_session_setting(vm, "name", false) {
        return Ok(Value::False);
    }
    let ret = s(&ps(|p| p.session_name.clone()));
    if let Some(n) = name {
        alter_ini(vm, b"session.name", &n);
    }
    Ok(ret)
}

fn f_session_module_name(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_module_name";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let name = tryarg!(str_arg(vm, fname, args, 0, "module", true, false));
    if name.is_some() && !php_can_change_session_setting(vm, "save handler module", false) {
        return Ok(Value::False);
    }
    let ret = s(ps(|p| p.module.map(|m| m.name()).unwrap_or("")).as_bytes());
    if let Some(n) = name {
        if n.eq_ignore_ascii_case(b"user") {
            throw(
                vm,
                b"ValueError",
                &format!("{}(): Argument #1 ($module) cannot be \"user\"", fname),
            );
            return Ok(Value::Null);
        }
        if find_module(cstr(&n)).is_none() {
            err(
                vm,
                E_WARNING,
                &format!(
                    "Session handler module \"{}\" cannot be found",
                    lossy(cstr(&n))
                ),
            );
            return Ok(Value::False);
        }
        if has_mod_data() {
            if let Some(m) = ps(|p| p.module) {
                mod_close(vm, m)?;
            }
        }
        ps(|p| p.mod_data = None);
        alter_ini(vm, b"session.save_handler", &n);
    }
    Ok(ret)
}

fn can_session_handler_be_changed(vm: &mut Vm) -> bool {
    if ps(|p| p.status) == Status::Active {
        already_started_error(
            vm,
            E_WARNING,
            "Session save handler cannot be changed when a session is active",
        );
        return false;
    }
    if vm.headers_sent {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session save handler cannot be changed after headers have already been sent",
        );
        return false;
    }
    true
}

fn set_user_save_handler_ini(vm: &mut Vm) {
    ps(|p| p.set_handler = true);
    alter_ini(vm, b"session.save_handler", b"user");
    ps(|p| p.set_handler = false);
}

fn is_shutdown_entry(cb: &Value) -> bool {
    matches!(cb, Value::String(s) if s.as_bytes().eq_ignore_ascii_case(b"session_register_shutdown"))
}

fn class_has_method(vm: &Vm, cl: &[u8], m: &[u8]) -> bool {
    if vm
        .classes
        .get(cl)
        .map(|c| c.get_method(m).is_some())
        .unwrap_or(false)
    {
        return true;
    }
    // Inherited from SessionHandler (native)
    vm.class_extends(cl, b"sessionhandler") && session_handler_has_method(m)
}

fn callable_ok(vm: &mut Vm, v: &Value) -> bool {
    vm.zend_is_callable_ex(v, 0).ok
}

fn callback_error(vm: &mut Vm, fname: &str, n: usize, pname: &str, v: &Value, nullable: bool) {
    let detail = vm
        .is_callable_pure(v, goro_core::callable::IS_CALLABLE_SUPPRESS_DEPRECATIONS)
        .error
        .unwrap_or_default();
    let msg = format!(
        "{}(): Argument #{} (${}) must be a valid callback{}, {}",
        fname,
        n,
        pname,
        if nullable { " or null" } else { "" },
        detail
    );
    throw(vm, b"TypeError", &msg);
}

fn f_session_set_save_handler(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_set_save_handler";
    let _g = FnGuard::new(vm, fname);
    if args.len() <= 2 {
        // OOP version
        if args.is_empty() {
            arg_count_error(vm, fname, 1, 2, 0);
            return Ok(Value::Null);
        }
        let obj = args[0].deref();
        let ok = match &obj {
            Value::Object(o) => {
                let cl = o.borrow().class_name.to_ascii_lowercase();
                vm.class_implements_interface(&cl, b"sessionhandlerinterface")
            }
            _ => false,
        };
        if !ok {
            type_err(vm, fname, 1, "open", "SessionHandlerInterface", &obj);
            return Ok(Value::Null);
        }
        let register_shutdown =
            tryarg!(bool_arg(vm, fname, args, 1, "close", false)).unwrap_or(true);
        if !can_session_handler_be_changed(vm) {
            return Ok(Value::False);
        }
        let (class_name, cl) = match &obj {
            Value::Object(o) => (
                o.borrow().class_name.clone(),
                o.borrow().class_name.to_ascii_lowercase(),
            ),
            _ => unreachable!(),
        };
        let mk = |m: &str| -> Value {
            let mut a = PhpArray::new();
            a.push(obj.clone());
            a.push(Value::String(PhpString::from_string(m.to_string())));
            Value::Array(Rc::new(RefCell::new(a)))
        };
        let create_sid = if vm.class_implements_interface(&cl, b"sessionidinterface")
            || class_has_method(vm, &cl, b"create_sid")
        {
            Some(mk("create_sid"))
        } else {
            None
        };
        let (validate, update) =
            if vm.class_implements_interface(&cl, b"sessionupdatetimestamphandlerinterface") {
                (Some(mk("validateId")), Some(mk("updateTimestamp")))
            } else {
                (
                    if class_has_method(vm, &cl, b"validateid") {
                        Some(mk("validateId"))
                    } else {
                        None
                    },
                    if class_has_method(vm, &cl, b"updatetimestamp") {
                        Some(mk("updateTimestamp"))
                    } else {
                        None
                    },
                )
            };
        let n = UserNames {
            open: Some(mk("open")),
            close: Some(mk("close")),
            read: Some(mk("read")),
            write: Some(mk("write")),
            destroy: Some(mk("destroy")),
            gc: Some(mk("gc")),
            create_sid,
            validate_sid: validate,
            update_timestamp: update,
        };
        let old = ps(|p| {
            p.mod_user_class_name =
                Some(goro_core::value::display_class_name(&class_name).into_bytes());
            std::mem::replace(&mut p.mod_user_names, n)
        });
        // Releasing the previous handler object may destroy it.
        drop(old);
        vm.run_pending_destructors_pub();
        vm.shutdown_functions
            .retain(|(cb, _)| !is_shutdown_entry(cb));
        if register_shutdown {
            vm.shutdown_functions
                .push((s(b"session_register_shutdown"), Vec::new()));
        }
        if ps(|p| p.status != Status::Active && p.module != Some(Module::User)) {
            set_user_save_handler_ini(vm);
        }
        return Ok(Value::True);
    }

    err(
        vm,
        E_DEPRECATED,
        "Providing individual callbacks instead of an object implementing SessionHandlerInterface is deprecated",
    );
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    const PNAMES: [&str; 9] = [
        "open",
        "close",
        "read",
        "write",
        "destroy",
        "gc",
        "create_sid",
        "validate_sid",
        "update_timestamp",
    ];
    if args.len() < 6 || args.len() > 9 {
        arg_count_error(vm, fname, 6, 9, args.len());
        return Ok(Value::Null);
    }
    let mut cbs: Vec<Option<Value>> = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let a = a.deref();
        if i >= 6 && matches!(a, Value::Null) {
            cbs.push(None);
            continue;
        }
        if !callable_ok(vm, &a) {
            callback_error(vm, fname, i + 1, PNAMES[i], &a, i >= 6);
            return Ok(Value::Null);
        }
        cbs.push(Some(a));
    }
    while cbs.len() < 9 {
        cbs.push(None);
    }
    if !can_session_handler_be_changed(vm) {
        return Ok(Value::False);
    }
    ps(|p| p.mod_user_class_name = None);
    vm.shutdown_functions
        .retain(|(cb, _)| !is_shutdown_entry(cb));
    if ps(|p| p.module != Some(Module::User)) {
        set_user_save_handler_ini(vm);
    }
    ps(|p| {
        let n = &mut p.mod_user_names;
        n.open = cbs[0].clone();
        n.close = cbs[1].clone();
        n.read = cbs[2].clone();
        n.write = cbs[3].clone();
        n.destroy = cbs[4].clone();
        n.gc = cbs[5].clone();
        if cbs[6].is_some() {
            n.create_sid = cbs[6].clone();
        }
        if cbs[7].is_some() {
            n.validate_sid = cbs[7].clone();
        }
        if cbs[8].is_some() {
            n.update_timestamp = cbs[8].clone();
        }
    });
    Ok(Value::True)
}

fn f_session_save_path(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_save_path";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let name = tryarg!(str_arg(vm, fname, args, 0, "path", true, true));
    if name.is_some() && !php_can_change_session_setting(vm, "save path", false) {
        return Ok(Value::False);
    }
    let ret = s(&ps(|p| p.save_path.clone()));
    if let Some(n) = name {
        alter_ini(vm, b"session.save_path", &n);
    }
    Ok(ret)
}

fn f_session_id(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_id";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let name = tryarg!(str_arg(vm, fname, args, 0, "id", true, false));
    if name.is_some() && !php_can_change_session_setting(vm, "ID", true) {
        return Ok(Value::False);
    }
    let ret = match ps(|p| p.id.clone()) {
        Some(id) => s(cstr(&id)),
        None => s(b""),
    };
    if let Some(n) = name {
        ps(|p| p.id = Some(n));
    }
    Ok(ret)
}

fn f_session_regenerate_id(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_regenerate_id";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let del = tryarg!(bool_arg(vm, fname, args, 0, "delete_old_session", false)).unwrap_or(false);
    if ps(|p| p.status) != Status::Active {
        already_started_error(
            vm,
            E_WARNING,
            "Session ID cannot be regenerated when there is no active session",
        );
        return Ok(Value::False);
    }
    if vm.headers_sent {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session ID cannot be regenerated after headers have already been sent",
        );
        return Ok(Value::False);
    }
    let m = ps(|p| p.module).unwrap_or(Module::Files);
    let (id, save_path, name) = ps(|p| {
        (
            p.id.clone().unwrap_or_default(),
            p.save_path.clone(),
            p.session_name.clone(),
        )
    });
    if del {
        if !mod_destroy(vm, m, &id)? {
            mod_close(vm, m)?;
            ps(|p| p.status = Status::None);
            if !pending(vm) {
                err(
                    vm,
                    E_WARNING,
                    &format!(
                        "Session object destruction failed. ID: {} (path: {})",
                        m.name(),
                        lossy(&save_path)
                    ),
                );
            }
            return Ok(Value::False);
        }
    } else {
        let data = session_encode_data(vm);
        let ret = mod_write(vm, m, &id, data.as_deref().unwrap_or(b""))?;
        if !ret {
            mod_close(vm, m)?;
            ps(|p| p.status = Status::None);
            err(
                vm,
                E_WARNING,
                &format!(
                    "Session write failed. ID: {} (path: {})",
                    m.name(),
                    lossy(&save_path)
                ),
            );
            return Ok(Value::False);
        }
    }
    mod_close(vm, m)?;
    ps(|p| {
        p.session_vars = None;
        p.id = None;
    });
    if !mod_open(vm, m, &save_path, &name)? {
        ps(|p| p.status = Status::None);
        if !pending(vm) {
            throw(
                vm,
                b"Error",
                &format!(
                    "Failed to open session: {} (path: {})",
                    m.name(),
                    lossy(&save_path)
                ),
            );
        }
        return Ok(Value::Null);
    }
    match mod_create_sid(vm, m)? {
        Some(n) => ps(|p| p.id = Some(n)),
        None => {
            ps(|p| p.status = Status::None);
            if !pending(vm) {
                throw(
                    vm,
                    b"Error",
                    &format!(
                        "Failed to create new session ID: {} (path: {})",
                        m.name(),
                        lossy(&save_path)
                    ),
                );
            }
            return Ok(Value::Null);
        }
    }
    if ps(|p| p.use_strict_mode) {
        let has_validator = match m {
            Module::Files => true,
            Module::User => ps(|p| p.mod_user_names.validate_sid.is_some()),
        };
        if has_validator {
            let mut limit = 3;
            loop {
                if limit == 0 {
                    break;
                }
                limit -= 1;
                let cur = ps(|p| p.id.clone().unwrap_or_default());
                if !mod_validate_sid(vm, m, &cur)? {
                    break;
                }
                match mod_create_sid(vm, m)? {
                    Some(n) => ps(|p| p.id = Some(n)),
                    None => {
                        mod_close(vm, m)?;
                        ps(|p| p.status = Status::None);
                        if !pending(vm) {
                            throw(
                                vm,
                                b"Error",
                                &format!(
                                    "Failed to create session ID by collision: {} (path: {})",
                                    m.name(),
                                    lossy(&save_path)
                                ),
                            );
                        }
                        return Ok(Value::Null);
                    }
                }
            }
        }
    }
    let cur = ps(|p| p.id.clone().unwrap_or_default());
    if mod_read(vm, m, &cur)?.is_none() {
        mod_close(vm, m)?;
        ps(|p| p.status = Status::None);
        if !pending(vm) {
            throw(
                vm,
                b"Error",
                &format!(
                    "Failed to create(read) session ID: {} (path: {})",
                    m.name(),
                    lossy(&save_path)
                ),
            );
        }
        return Ok(Value::Null);
    }
    if ps(|p| p.use_cookies) {
        ps(|p| p.send_cookie = true);
    }
    if !session_reset_id(vm)? {
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn f_session_create_id(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_create_id";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let prefix = tryarg!(str_arg(vm, fname, args, 0, "prefix", false, true));
    let mut id = Vec::new();
    if let Some(p) = prefix {
        if !p.is_empty() {
            if p.len() > PS_MAX_SID_LENGTH {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #1 ($prefix) cannot be longer than {} characters",
                        fname, PS_MAX_SID_LENGTH
                    ),
                );
                return Ok(Value::Null);
            }
            if !valid_key(&p) {
                err(
                    vm,
                    E_WARNING,
                    "Prefix cannot contain special characters. Only the A-Z, a-z, 0-9, \"-\", and \",\" characters are allowed",
                );
                return Ok(Value::False);
            }
            id.extend_from_slice(&p);
        }
    }
    let new_id = if !ps(|p| p.in_save_handler) && ps(|p| p.status) == Status::Active {
        let m = ps(|p| p.module).unwrap_or(Module::Files);
        let mut result = None;
        let mut limit = 3;
        while limit > 0 {
            limit -= 1;
            let nid = mod_create_sid(vm, m)?;
            let no_validator = m == Module::User && ps(|p| p.mod_user_names.validate_sid.is_none());
            match nid {
                None => {
                    result = None;
                    break;
                }
                Some(n) => {
                    if no_validator {
                        result = Some(n);
                        break;
                    }
                    if mod_validate_sid(vm, m, &n)? {
                        result = None;
                        continue;
                    }
                    result = Some(n);
                    break;
                }
            }
        }
        result
    } else {
        Some(create_id())
    };
    match new_id {
        Some(n) => {
            id.extend_from_slice(&n);
            Ok(s(&id))
        }
        None => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            err(vm, E_WARNING, "Failed to create new ID");
            Ok(Value::False)
        }
    }
}

fn f_session_cache_limiter(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_cache_limiter";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let limiter = tryarg!(str_arg(vm, fname, args, 0, "value", true, false));
    if limiter.is_some() && ps(|p| p.status) == Status::Active {
        already_started_error(
            vm,
            E_WARNING,
            "Session cache limiter cannot be changed when a session is active",
        );
        return Ok(Value::False);
    }
    if limiter.is_some() && vm.headers_sent {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session cache limiter cannot be changed after headers have already been sent",
        );
        return Ok(Value::False);
    }
    let ret = s(&ps(|p| p.cache_limiter.clone()));
    if let Some(l) = limiter {
        alter_ini(vm, b"session.cache_limiter", &l);
    }
    Ok(ret)
}

fn f_session_cache_expire(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_cache_expire";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let expires = tryarg!(long_arg(vm, fname, args, 0, "value", true));
    if expires.is_some() && ps(|p| p.status) == Status::Active {
        already_started_error(
            vm,
            E_WARNING,
            "Session cache expiration cannot be changed when a session is active",
        );
        return Ok(Value::Long(ps(|p| p.cache_expire)));
    }
    if expires.is_some() && vm.headers_sent {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session cache expiration cannot be changed after headers have already been sent",
        );
        return Ok(Value::False);
    }
    let ret = Value::Long(ps(|p| p.cache_expire));
    if let Some(e) = expires {
        alter_ini(vm, b"session.cache_expire", e.to_string().as_bytes());
    }
    Ok(ret)
}

fn f_session_encode(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_encode";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    match session_encode_data(vm) {
        Some(v) => Ok(s(&v)),
        None => Ok(Value::False),
    }
}

fn f_session_decode(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_decode";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 1, 1) {
        return Ok(Value::Null);
    }
    let data = tryarg!(str_arg(vm, fname, args, 0, "data", false, false)).unwrap_or_default();
    if ps(|p| p.status) != Status::Active {
        err(
            vm,
            E_WARNING,
            "Session data cannot be decoded when there is no active session",
        );
        return Ok(Value::False);
    }
    Ok(b(session_decode_data(vm, &data)?))
}

fn f_session_start(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_start";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 1) {
        return Ok(Value::Null);
    }
    let options = match args.first().map(|v| v.deref()) {
        None => None,
        Some(Value::Array(a)) => Some(a),
        Some(other) => {
            type_err(vm, fname, 1, "options", "array", &other);
            return Ok(Value::Null);
        }
    };
    if ps(|p| p.status) == Status::Active {
        already_started_error(
            vm,
            E_NOTICE,
            "Ignoring session_start() because a session is already active",
        );
        return Ok(Value::True);
    }
    if ps(|p| p.use_cookies) && vm.headers_sent {
        headers_already_sent_error(
            vm,
            E_WARNING,
            "Session cannot be started after headers have already been sent",
        );
        return Ok(Value::False);
    }
    let mut read_and_close = false;
    if let Some(opts) = options {
        let entries: Vec<(ArrayKey, Value)> = opts
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in entries {
            let key = match k {
                ArrayKey::String(s) => s.as_bytes().to_vec(),
                ArrayKey::Int(_) => {
                    throw(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #1 ($options) must be of type array with keys as string",
                            fname
                        ),
                    );
                    return Ok(Value::Null);
                }
            };
            let v = v.deref();
            match &v {
                Value::String(_) | Value::True | Value::False | Value::Long(_) => {
                    if key == b"read_and_close" {
                        let tmp = match &v {
                            Value::String(sv) => {
                                match goro_core::value::parse_numeric_string(sv.as_bytes()) {
                                    Some(f)
                                        if f.fract() == 0.0 && !sv.as_bytes().contains(&b'.') =>
                                    {
                                        f as i64
                                    }
                                    _ => {
                                        throw(
                                            vm,
                                            b"TypeError",
                                            &format!(
                                                "{}(): Option \"{}\" value must be of type compatible with int, \"{}\" given",
                                                fname,
                                                lossy(&key),
                                                lossy(sv.as_bytes())
                                            ),
                                        );
                                        return Ok(Value::Null);
                                    }
                                }
                            }
                            other => other.to_long(),
                        };
                        read_and_close = tmp > 0;
                    } else {
                        let val = v.to_php_string().as_bytes().to_vec();
                        let mut ini = b"session.".to_vec();
                        ini.extend_from_slice(&key);
                        if !alter_ini(vm, &ini, &val) {
                            err(
                                vm,
                                E_WARNING,
                                &format!("Setting option \"{}\" failed", lossy(&key)),
                            );
                        }
                    }
                }
                other => {
                    throw(
                        vm,
                        b"TypeError",
                        &format!(
                            "{}(): Option \"{}\" must be of type string|int|bool, {} given",
                            fname,
                            lossy(&key),
                            Vm::value_type_name(other)
                        ),
                    );
                    return Ok(Value::Null);
                }
            }
        }
    }
    session_start_internal(vm)?;
    if ps(|p| p.status) != Status::Active {
        if let Some(arr) = session_vars_arr_mut() {
            *arr.borrow_mut() = PhpArray::new();
        }
        return Ok(Value::False);
    }
    if read_and_close {
        session_flush(vm, false)?;
    }
    Ok(Value::True)
}

fn f_session_destroy(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_destroy";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    Ok(b(session_destroy_internal(vm)?))
}

fn f_session_unset(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_unset";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    if ps(|p| p.status) != Status::Active {
        return Ok(Value::False);
    }
    if let Some(arr) = session_vars_arr_mut() {
        *arr.borrow_mut() = PhpArray::new();
    }
    Ok(Value::True)
}

fn f_session_gc(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_gc";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    if ps(|p| p.status) != Status::Active {
        err(
            vm,
            E_WARNING,
            "Session cannot be garbage collected when there is no active session",
        );
        return Ok(Value::False);
    }
    let n = session_gc_internal(vm, true)?;
    if n < 0 {
        return Ok(Value::False);
    }
    Ok(Value::Long(n))
}

fn f_session_write_close(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_write_close";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    if ps(|p| p.status) != Status::Active {
        return Ok(Value::False);
    }
    session_flush(vm, true)?;
    Ok(Value::True)
}

fn f_session_commit(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_commit";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    if ps(|p| p.status) != Status::Active {
        return Ok(Value::False);
    }
    session_flush(vm, true)?;
    Ok(Value::True)
}

fn f_session_abort(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_abort";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    if ps(|p| p.status) != Status::Active {
        return Ok(Value::False);
    }
    session_abort_internal(vm)?;
    Ok(Value::True)
}

fn f_session_reset(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_reset";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    if ps(|p| p.status) != Status::Active {
        return Ok(Value::False);
    }
    session_initialize(vm)?;
    Ok(Value::True)
}

fn f_session_status(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_status";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    Ok(Value::Long(ps(|p| p.status) as i64))
}

fn f_session_register_shutdown(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let fname = "session_register_shutdown";
    let _g = FnGuard::new(vm, fname);
    if !check_args(vm, fname, args, 0, 0) {
        return Ok(Value::Null);
    }
    vm.shutdown_functions
        .push((s(b"session_write_close"), Vec::new()));
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// SessionHandler class (mod_user_class.c)
// ---------------------------------------------------------------------------

fn session_handler_has_method(m: &[u8]) -> bool {
    matches!(
        m,
        b"open" | b"close" | b"read" | b"write" | b"destroy" | b"gc" | b"create_sid"
    )
}

fn sh_has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    !is_static && session_handler_has_method(method)
}

fn sh_param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    Some(match method {
        b"open" => &["path", "name"],
        b"read" | b"destroy" => &["id"],
        b"write" => &["id", "data"],
        b"gc" => &["max_lifetime"],
        _ => &[],
    })
}

/// PS_SANITY_CHECK (+ _IS_OPEN). Returns Err(ret) if the method must return.
fn sh_sanity(vm: &mut Vm, is_open: bool) -> Result<Module, Value> {
    if ps(|p| p.status) != Status::Active {
        throw(vm, b"Error", "Session is not active");
        return Err(Value::Null);
    }
    let dm = match ps(|p| p.default_mod) {
        Some(m) => m,
        None => {
            throw(vm, b"Error", "Cannot call default session handler");
            return Err(Value::Null);
        }
    };
    if is_open && !ps(|p| p.mod_user_is_open) {
        err(vm, E_WARNING, "Parent session handler is not open");
        return Err(Value::False);
    }
    Ok(dm)
}

fn sh_call(
    vm: &mut Vm,
    _native: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&Value>,
    args: &[Value],
) -> Value {
    let frame = format!("SessionHandler::{}", lossy(method));
    let pushed = !vm
        .call_stack
        .last()
        .map(|f| f.0.eq_ignore_ascii_case(&frame))
        .unwrap_or(false);
    if pushed {
        vm.call_stack.push((
            frame,
            vm.current_file.clone(),
            vm.current_line,
            args.to_vec(),
            true,
        ));
    }
    let r = sh_call_frame(vm, method, args);
    if pushed {
        vm.call_stack.pop();
    }
    r
}

fn sh_call_frame(vm: &mut Vm, method: &[u8], args: &[Value]) -> Value {
    match sh_call_inner(vm, method, args) {
        Ok(v) => v,
        Err(e) => {
            // Bailout inside a handler method: remember and abort the request.
            if !pending(vm) && !vm.fatal_emitted {
                vm.emit_fatal_at(&e.message, e.line);
            }
            vm.exit_requested = true;
            Value::Null
        }
    }
}

fn sh_call_inner(vm: &mut Vm, method: &[u8], args: &[Value]) -> R<Value> {
    let mname = match method {
        b"open" => "open",
        b"close" => "close",
        b"read" => "read",
        b"write" => "write",
        b"destroy" => "destroy",
        b"gc" => "gc",
        _ => "create_sid",
    };
    let fname = format!("SessionHandler::{}", mname);
    let _g = FnGuard::new(vm, &fname);
    macro_rules! sanity {
        ($open:expr) => {
            match sh_sanity(vm, $open) {
                Ok(m) => m,
                Err(v) => return Ok(v),
            }
        };
    }
    let maxl = ps(|p| p.gc_maxlifetime);
    match method {
        b"open" => {
            if !check_args(vm, &fname, args, 2, 2) {
                return Ok(Value::Null);
            }
            let path =
                tryarg!(str_arg(vm, &fname, args, 0, "path", false, false)).unwrap_or_default();
            let name =
                tryarg!(str_arg(vm, &fname, args, 1, "name", false, false)).unwrap_or_default();
            let dm = sanity!(false);
            let r = match mod_open(vm, dm, &path, &name) {
                Ok(r) => r,
                Err(e) => {
                    ps(|p| p.status = Status::None);
                    return Err(e);
                }
            };
            if r {
                ps(|p| p.mod_user_is_open = true);
            }
            Ok(b(r))
        }
        b"close" => {
            // zend_parse_parameters_none(): the error does not stop close.
            check_args(vm, &fname, args, 0, 0);
            let dm = sanity!(true);
            ps(|p| p.mod_user_is_open = false);
            let r = match mod_close(vm, dm) {
                Ok(r) => r,
                Err(e) => {
                    ps(|p| p.status = Status::None);
                    return Err(e);
                }
            };
            Ok(b(r))
        }
        b"read" => {
            if !check_args(vm, &fname, args, 1, 1) {
                return Ok(Value::Null);
            }
            let key = tryarg!(str_arg(vm, &fname, args, 0, "id", false, false)).unwrap_or_default();
            let dm = sanity!(true);
            let _ = maxl;
            match mod_read(vm, dm, &key)? {
                Some(v) => Ok(s(&v)),
                None => Ok(Value::False),
            }
        }
        b"write" => {
            if !check_args(vm, &fname, args, 2, 2) {
                return Ok(Value::Null);
            }
            let key = tryarg!(str_arg(vm, &fname, args, 0, "id", false, false)).unwrap_or_default();
            let val =
                tryarg!(str_arg(vm, &fname, args, 1, "data", false, false)).unwrap_or_default();
            let dm = sanity!(true);
            Ok(b(mod_write(vm, dm, &key, &val)?))
        }
        b"destroy" => {
            if !check_args(vm, &fname, args, 1, 1) {
                return Ok(Value::Null);
            }
            let key = tryarg!(str_arg(vm, &fname, args, 0, "id", false, false)).unwrap_or_default();
            let dm = sanity!(true);
            Ok(b(mod_destroy(vm, dm, &key)?))
        }
        b"gc" => {
            if !check_args(vm, &fname, args, 1, 1) {
                return Ok(Value::Null);
            }
            let ml = tryarg!(long_arg(vm, &fname, args, 0, "max_lifetime", false)).unwrap_or(0);
            let dm = sanity!(true);
            let n = mod_gc(vm, dm, ml)?;
            if n == -1 {
                return Ok(Value::False);
            }
            Ok(Value::Long(n))
        }
        _ => {
            if !check_args(vm, &fname, args, 0, 0) {
                return Ok(Value::Null);
            }
            let dm = sanity!(false);
            match mod_create_sid(vm, dm)? {
                Some(id) => Ok(s(&id)),
                None => Ok(Value::Null),
            }
        }
    }
}

fn register_classes(vm: &mut Vm) {
    let mut iface = |name: &[u8]| {
        let mut ce = ClassEntry::new(name.to_vec());
        ce.is_interface = true;
        vm.classes.entry(name.to_ascii_lowercase()).or_insert(ce);
    };
    iface(b"SessionHandlerInterface");
    iface(b"SessionIdInterface");
    iface(b"SessionUpdateTimestampHandlerInterface");
    let mut sh = ClassEntry::new(b"SessionHandler".to_vec());
    sh.interfaces = vec![
        b"SessionHandlerInterface".to_vec(),
        b"SessionIdInterface".to_vec(),
    ];
    vm.classes.insert(b"sessionhandler".to_vec(), sh);
    let h = NativeClassHandler {
        has_method: sh_has_method,
        call: sh_call,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(sh_param_names),
    };
    vm.native_class_handlers
        .insert(b"sessionhandler".to_vec(), h);
    let _ = Visibility::Public;
}

// ---------------------------------------------------------------------------
// Module lifecycle
// ---------------------------------------------------------------------------

/// RINIT
fn request_startup(vm: &mut Vm) {
    PHASE.with(|p| p.set(Phase::RequestStartup));
    let _ = rinit(vm, ps(|p| p.auto_start));
    PHASE.with(|p| p.set(Phase::Runtime));
    vm.request_shutdown_hooks.push(request_shutdown);
}

fn rinit(vm: &mut Vm, auto_start: bool) -> R<()> {
    rinit_globals();
    let sh = ini_get_str(vm, "session.save_handler");
    ps(|p| p.module = find_module(&sh));
    if ps(|p| p.serializer.is_none()) {
        let v = ini_get_str(vm, "session.serialize_handler");
        ps(|p| p.serializer = find_serializer(&v));
    }
    if ps(|p| p.module.is_none() || p.serializer.is_none()) {
        ps(|p| p.status = Status::Disabled);
        return Ok(());
    }
    if auto_start {
        session_start_internal(vm)?;
    }
    Ok(())
}

/// RSHUTDOWN
fn request_shutdown(vm: &mut Vm) {
    PHASE.with(|p| p.set(Phase::RequestShutdown));
    let exc = vm.current_exception.take();
    if ps(|p| p.status) == Status::Active {
        let _ = session_flush(vm, true);
    }
    let _ = rshutdown_globals(vm);
    ps(|p| p.mod_user_names = UserNames::default());
    ini_deactivate(vm);
    PHASE.with(|p| p.set(Phase::Runtime));
    if vm.current_exception.is_none() {
        vm.current_exception = exc;
    }
}

/// Register all session extension functions, classes and INI entries.
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"session");
    PS.with(|p| *p.borrow_mut() = Ps::default());
    FN_STACK.with(|s| s.borrow_mut().clear());
    MODIFIED.with(|m| m.borrow_mut().clear());
    PHASE.with(|p| p.set(Phase::Startup));
    rewrite::reset();

    let fns: &[(&[u8], goro_core::vm::BuiltinFn, &[&[u8]])] = &[
        (b"session_name", f_session_name, &[b"name"]),
        (b"session_module_name", f_session_module_name, &[b"module"]),
        (b"session_save_path", f_session_save_path, &[b"path"]),
        (b"session_id", f_session_id, &[b"id"]),
        (b"session_create_id", f_session_create_id, &[b"prefix"]),
        (
            b"session_regenerate_id",
            f_session_regenerate_id,
            &[b"delete_old_session"],
        ),
        (b"session_decode", f_session_decode, &[b"data"]),
        (b"session_encode", f_session_encode, &[]),
        (b"session_destroy", f_session_destroy, &[]),
        (b"session_unset", f_session_unset, &[]),
        (b"session_gc", f_session_gc, &[]),
        (
            b"session_get_cookie_params",
            f_session_get_cookie_params,
            &[],
        ),
        (b"session_write_close", f_session_write_close, &[]),
        (b"session_abort", f_session_abort, &[]),
        (b"session_reset", f_session_reset, &[]),
        (b"session_status", f_session_status, &[]),
        (
            b"session_register_shutdown",
            f_session_register_shutdown,
            &[],
        ),
        (b"session_commit", f_session_commit, &[]),
        (
            b"session_set_save_handler",
            f_session_set_save_handler,
            &[
                b"open",
                b"close",
                b"read",
                b"write",
                b"destroy",
                b"gc",
                b"create_sid",
                b"validate_sid",
                b"update_timestamp",
            ],
        ),
        (
            b"session_cache_limiter",
            f_session_cache_limiter,
            &[b"value"],
        ),
        (b"session_cache_expire", f_session_cache_expire, &[b"value"]),
        (
            b"session_set_cookie_params",
            f_session_set_cookie_params,
            &[
                b"lifetime_or_options",
                b"path",
                b"domain",
                b"secure",
                b"httponly",
            ],
        ),
        (b"session_start", f_session_start, &[b"options"]),
    ];
    for (name, f, params) in fns {
        vm.register_function_with_params(name, *f, params);
    }
    rewrite::register(vm);

    vm.constants
        .insert(b"PHP_SESSION_DISABLED".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"PHP_SESSION_NONE".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"PHP_SESSION_ACTIVE".to_vec(), Value::Long(2));
    // (SID once a session sets it)
    vm.module_constants.push((
        b"session".to_vec(),
        [
            "PHP_SESSION_DISABLED",
            "PHP_SESSION_NONE",
            "PHP_SESSION_ACTIVE",
            "SID",
        ]
        .iter()
        .map(|n| n.as_bytes().to_vec())
        .collect(),
    ));

    for (name, default, _) in INI_ENTRIES {
        vm.constants
            .entry(name.as_bytes().to_vec())
            .or_insert_with(|| s(default.as_bytes()));
        vm.ini_set_hooks
            .insert(name.as_bytes().to_vec(), ini_set_hook);
    }
    register_classes(vm);
    vm.startup_hooks.push(startup);
    vm.request_startup_hooks.push(request_startup);
    vm.rfc1867_callback = Some(upload::callback);
}
