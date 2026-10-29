//! ext/session (PHP manual: Session Handling): the session_* functions,
//! the files save handler, user save handlers (SessionHandler and the
//! SessionHandler* interfaces, session_set_save_handler()), the php,
//! php_binary and php_serialize serializers, session IDs, the session
//! cookie and cache limiter headers, and transparent session IDs (with
//! output_add_rewrite_var() of ext/standard, which shares the URL
//! rewriter).
//!
//! The order of the handler calls, the messages and the results follow
//! the ext/session tests and what PHP 8.5 prints.

mod files;
mod ini;
pub(crate) mod rewrite;
mod serial;
mod user;

use goro_runtime::{Key, PhpArray, PhpRef, PhpStr, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::errors::{E_DEPRECATED, E_NOTICE, E_WARNING};
use crate::vm::{R, Vm};

pub use user::UserHandler;

pub static MODULE: Module = Module {
    name: "session",
    functions: &[
        (
            "function session_name(?string $name = null): string|false",
            session_name,
        ),
        (
            "function session_module_name(?string $module = null): string|false",
            session_module_name,
        ),
        (
            "function session_save_path(?string $path = null): string|false",
            session_save_path,
        ),
        (
            "function session_id(?string $id = null): string|false",
            session_id,
        ),
        (
            "function session_create_id(string $prefix = \"\"): string|false",
            session_create_id,
        ),
        (
            "function session_regenerate_id(bool $delete_old_session = false): bool",
            session_regenerate_id,
        ),
        (
            "function session_decode(string $data): bool",
            session_decode,
        ),
        ("function session_encode(): string|false", session_encode),
        ("function session_destroy(): bool", session_destroy),
        ("function session_unset(): bool", session_unset),
        ("function session_gc(): int|false", session_gc),
        (
            "function session_get_cookie_params(): array",
            session_get_cookie_params,
        ),
        ("function session_write_close(): bool", session_write_close),
        ("function session_abort(): bool", session_abort),
        ("function session_reset(): bool", session_reset),
        ("function session_status(): int", session_status),
        (
            "function session_register_shutdown(): void",
            session_register_shutdown,
        ),
        ("function session_commit(): bool", session_write_close),
        (
            "function session_set_save_handler($open, $close = UNKNOWN, $read = UNKNOWN, $write = UNKNOWN, $destroy = UNKNOWN, $gc = UNKNOWN, $create_sid = null, $validate_sid = null, $update_timestamp = null): bool",
            user::session_set_save_handler,
        ),
        (
            "function session_cache_limiter(?string $value = null): string|false",
            session_cache_limiter,
        ),
        (
            "function session_cache_expire(?int $value = null): int|false",
            session_cache_expire,
        ),
        (
            "function session_set_cookie_params(array|int $lifetime_or_options, ?string $path = null, ?string $domain = null, ?bool $secure = null, ?bool $httponly = null): bool",
            session_set_cookie_params,
        ),
        (
            "function session_start(array $options = []): bool",
            session_start,
        ),
    ],
    classes: user::CLASSES,
    methods: user::METHODS,
    hooks: &[],
    constants: &[
        ("PHP_SESSION_DISABLED", ConstVal::Int(0)),
        ("PHP_SESSION_NONE", ConstVal::Int(1)),
        ("PHP_SESSION_ACTIVE", ConstVal::Int(2)),
        // Defined by session_start() (listed for get_defined_constants()).
        ("SID", ConstVal::Str("")),
    ],
    ini: ini::INI,
    init: Some(init),
    shutdown: Some(rshutdown),
    startup_ini: Some(ini::startup_ini),
    on_ini_set: Some(ini::on_ini_set),
    listed: true,
};

thread_local! {
    /// SID is the session's constant (deprecated since PHP 8.4), not one
    /// a script defined first (test SID_type_confusion.phpt).
    static SID_INTERNAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the SID constant is the one session_start() defines.
pub(crate) fn sid_is_internal() -> bool {
    SID_INTERNAL.with(|c| c.get())
}

/// Where a diagnostic is raised from.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Phase {
    /// A function call: "name(): ".
    #[default]
    Run,
    /// The startup settings: "PHP Startup: ", in Unknown on line 0.
    Startup,
    /// session.auto_start: "PHP Request Startup: ".
    RequestStartup,
    /// The end of the request: "PHP Request Shutdown: ".
    Shutdown,
}

/// The save handler module in use.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum Mod {
    #[default]
    Files,
    User,
}

impl Mod {
    fn name(self) -> &'static str {
        match self {
            Mod::Files => "files",
            Mod::User => "user",
        }
    }
}

/// The session state of the request.
#[derive(Default)]
pub(crate) struct Sess {
    pub active: bool,
    /// The session ID (session_id()); None when unset.
    pub id: Option<PhpStr>,
    /// Where session_start() was called; None with session.auto_start.
    pub started_at: Option<(PhpStr, u32)>,
    /// $_SESSION's cell, shared with the global variable.
    pub vars: Option<PhpRef>,
    /// The data read() returned (for session.lazy_write).
    pub data_read: Option<Vec<u8>>,
    /// The module of the active session.
    pub module: Mod,
    /// The user save handler (session_set_save_handler()).
    pub user: Option<UserHandler>,
    /// SessionHandler's parent module is the files handler.
    pub default_files: Option<bool>,
    /// SessionHandler::open() succeeded (and close() did not run since).
    pub parent_open: bool,
    pub files: files::Files,
    pub phase: Phase,
    /// The ID came with the request's cookie.
    pub from_cookie: bool,
    /// The session cookie is to be sent.
    pub send_cookie: bool,
    /// The registered save handler's write runs from a shutdown function.
    pub shutdown_registered: bool,
}

fn st(vm: &mut Vm) -> &mut Sess {
    vm.ext::<Sess>()
}

fn init(vm: &mut Vm) {
    // SID exists only once a session started.
    vm.constants.remove(&PhpStr::from("SID"));
}

/// Raises a diagnostic with the prefix of the current phase.
pub(crate) fn raise(vm: &mut Vm, level: i64, msg: &str) -> R<()> {
    let unknown = PhpStr::from("Unknown");
    match st(vm).phase {
        Phase::Run => {
            let f = vm.active_function_name();
            vm.raise(level, &format!("{f}(): {msg}"))
        }
        Phase::Startup => vm.raise_at(level, &format!("PHP Startup: {msg}"), unknown, 0),
        Phase::RequestStartup => {
            vm.raise_at(level, &format!("PHP Request Startup: {msg}"), unknown, 0)
        }
        Phase::Shutdown => vm.raise_at(level, &format!("PHP Request Shutdown: {msg}"), unknown, 0),
    }
}

pub(crate) fn warn(vm: &mut Vm, msg: &str) -> R<()> {
    raise(vm, E_WARNING, msg)
}

pub(crate) fn deprecated(vm: &mut Vm, msg: &str) -> R<()> {
    raise(vm, E_DEPRECATED, msg)
}

/// " (started from F on line L)" or " (session started automatically)".
fn started_from(vm: &mut Vm) -> String {
    match &st(vm).started_at {
        Some((f, l)) => format!(" (started from {} on line {})", f.to_string_lossy(), l),
        None => " (session started automatically)".to_string(),
    }
}

/// " (sent from F on line L)".
fn sent_from(vm: &Vm) -> String {
    match &vm.out.sapi.output_start {
        Some((f, l)) => format!(" (sent from {} on line {})", String::from_utf8_lossy(f), l),
        None => String::new(),
    }
}

/// The headers were sent: output started (startup diagnostics don't
/// count: tests 015.phpt, bug66481.phpt print them before
/// session_start()).
pub(crate) fn headers_sent(vm: &Vm) -> bool {
    vm.out.sapi.sent
        || (vm.out.written > 0
            && vm
                .out
                .sapi
                .output_start
                .as_ref()
                .is_some_and(|(f, l)| !f.is_empty() || *l > 0))
}

/// The warning of a setter called while a session is active.
fn active_refusal(vm: &mut Vm, what: &str) -> R<bool> {
    if st(vm).active {
        let at = started_from(vm);
        warn(vm, &format!("{what} when a session is active{at}"))?;
        return Ok(true);
    }
    Ok(false)
}

/// The warning of a setter called after the headers were sent.
fn sent_refusal(vm: &mut Vm, what: &str) -> R<bool> {
    if headers_sent(vm) {
        let at = sent_from(vm);
        warn(
            vm,
            &format!("{what} after headers have already been sent{at}"),
        )?;
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn ini_str(vm: &Vm, k: &str) -> PhpStr {
    vm.get_ini(k.as_bytes()).unwrap_or_default()
}

pub(crate) fn ini_bool(vm: &Vm, k: &str) -> bool {
    ini::truthy(&ini_str(vm, k))
}

pub(crate) fn ini_int(vm: &Vm, k: &str) -> i64 {
    goro_runtime::convert::str_to_int(&ini_str(vm, k))
}

// ---------------------------------------------------------------- $_SESSION

/// Binds $_SESSION to a new cell holding `arr`.
fn bind_vars(vm: &mut Vm, arr: PhpArray) {
    let cell = PhpRef::new(Value::Array(arr));
    let p = vm.global_var_ptr(&PhpStr::from("_SESSION"), true).unwrap();
    // SAFETY: the pointer is the global's slot, used before any other
    // access to the globals.
    unsafe { *p = Value::Ref(cell.clone()) };
    st(vm).vars = Some(cell);
}

/// The session variables, if $_SESSION holds an array.
fn vars_array(vm: &mut Vm) -> Option<PhpArray> {
    match st(vm).vars.as_ref()?.get() {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

fn set_vars(vm: &mut Vm, arr: PhpArray) {
    match st(vm).vars.clone() {
        Some(c) => drop(c.set(Value::Array(arr))),
        None => bind_vars(vm, arr),
    }
}

// ---------------------------------------------------------------- IDs

const SID_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-,";

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    let mut got = 0;
    while got < n {
        // SAFETY: writes at most `n - got` bytes into the buffer.
        let r =
            unsafe { libc::getrandom(buf[got..].as_mut_ptr() as *mut libc::c_void, n - got, 0) };
        if r <= 0 {
            break;
        }
        got += r as usize;
    }
    buf
}

/// A new session ID: session.sid_length characters of
/// session.sid_bits_per_character random bits each (PHP manual: Session
/// configuration).
pub(crate) fn new_id(vm: &Vm) -> PhpStr {
    let len = ini_int(vm, "session.sid_length").clamp(22, 256) as usize;
    let bits = ini_int(vm, "session.sid_bits_per_character").clamp(4, 6) as u32;
    let bytes = random_bytes((len * bits as usize).div_ceil(8) + 1);
    let mut out = Vec::with_capacity(len);
    let mut acc: u32 = 0;
    let mut have = 0u32;
    let mut it = bytes.into_iter();
    while out.len() < len {
        if have < bits {
            acc |= (it.next().unwrap_or(0) as u32) << have;
            have += 8;
        }
        out.push(SID_CHARS[(acc & ((1 << bits) - 1)) as usize]);
        acc >>= bits;
        have -= bits;
    }
    PhpStr::from(out)
}

/// The characters a session ID may contain (PHP manual: session_id()).
pub(crate) fn valid_id(id: &[u8]) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || c == b',' || c == b'-')
}

// ---------------------------------------------------------------- the module

/// The module named by session.save_handler.
fn module_of(vm: &mut Vm) -> Option<Mod> {
    match &ini_str(vm, "session.save_handler")[..] {
        b"files" => Some(Mod::Files),
        b"user" if st(vm).user.is_some() => Some(Mod::User),
        _ => None,
    }
}

pub(crate) fn mod_open(vm: &mut Vm, m: Mod, path: &[u8], name: &[u8]) -> R<bool> {
    match m {
        Mod::Files => files::open(vm, path),
        Mod::User => user::open(vm, path, name),
    }
}

pub(crate) fn mod_close(vm: &mut Vm, m: Mod) -> R<bool> {
    match m {
        Mod::Files => Ok(files::close(vm)),
        Mod::User => user::close(vm),
    }
}

fn mod_read(vm: &mut Vm, m: Mod, id: &[u8]) -> R<Option<Vec<u8>>> {
    match m {
        Mod::Files => files::read(vm, id),
        Mod::User => user::read(vm, id),
    }
}

fn mod_write(vm: &mut Vm, m: Mod, id: &[u8], data: &[u8]) -> R<bool> {
    match m {
        Mod::Files => files::write(vm, id, data),
        Mod::User => user::write(vm, id, data),
    }
}

fn mod_destroy(vm: &mut Vm, m: Mod, id: &[u8]) -> R<bool> {
    match m {
        Mod::Files => Ok(files::destroy(vm, id)),
        Mod::User => user::destroy(vm, id),
    }
}

fn mod_gc(vm: &mut Vm, m: Mod, maxlifetime: i64) -> R<Option<i64>> {
    match m {
        Mod::Files => Ok(files::gc(vm, maxlifetime)),
        Mod::User => user::gc(vm, maxlifetime),
    }
}

/// A new ID from the module; None when it gave none.
fn mod_create_sid(vm: &mut Vm, m: Mod) -> R<Option<PhpStr>> {
    match m {
        Mod::Files => Ok(Some(new_id(vm))),
        Mod::User => user::create_sid(vm),
    }
}

/// Whether the session `id` exists (strict mode, collisions).
fn mod_validate(vm: &mut Vm, m: Mod, id: &[u8]) -> R<bool> {
    match m {
        Mod::Files => Ok(files::exists(vm, id)),
        Mod::User => user::validate_sid(vm, id),
    }
}

/// Whether the module validates IDs (use_strict_mode).
fn mod_has_validate(vm: &mut Vm, m: Mod) -> bool {
    match m {
        Mod::Files => true,
        Mod::User => st(vm).user.as_ref().is_some_and(|u| u.has(user::VALIDATE)),
    }
}

/// Lazy write: the timestamp update, or None when the module has none.
fn mod_update_timestamp(vm: &mut Vm, m: Mod, id: &[u8], data: &[u8]) -> R<Option<bool>> {
    match m {
        Mod::Files => Ok(Some(files::update_timestamp(vm, id))),
        Mod::User => {
            if st(vm).user.as_ref().is_some_and(|u| u.has(user::UPDATE)) {
                user::update_timestamp(vm, id, data).map(Some)
            } else {
                Ok(None)
            }
        }
    }
}

// ---------------------------------------------------------------- start

/// The ID the request carries: the cookie, then (without
/// use_only_cookies) the query and the form (PHP manual: Passing the
/// Session ID).
fn request_id(vm: &mut Vm) -> Option<PhpStr> {
    let name = ini_str(vm, "session.name");
    let lookup = |vm: &mut Vm, g: &str| -> Option<PhpStr> {
        let p = vm.global_var_ptr(&PhpStr::from(g), false)?;
        // SAFETY: read of the global's slot.
        let v = unsafe { (*p).deref() };
        match v {
            Value::Array(a) => match a.get_str(&name).map(|v| v.deref()) {
                Some(Value::Str(s)) => Some(s),
                _ => None,
            },
            _ => None,
        }
    };
    if ini_bool(vm, "session.use_cookies")
        && let Some(id) = lookup(vm, "_COOKIE")
    {
        st(vm).from_cookie = true;
        return Some(id);
    }
    if !ini_bool(vm, "session.use_only_cookies") {
        if let Some(id) = lookup(vm, "_GET") {
            return Some(id);
        }
        if let Some(id) = lookup(vm, "_POST") {
            return Some(id);
        }
    }
    None
}

/// Opens, reads and decodes the session (the ID is settled first).
/// Ok(false) after a failure (reported).
fn initialize(vm: &mut Vm) -> R<bool> {
    let m = st(vm).module;
    let path = ini_str(vm, "session.save_path");
    let name = ini_str(vm, "session.name");
    match mod_open(vm, m, &path, &name) {
        Ok(true) => {}
        Ok(false) => {
            // The handler is closed (test gh13856.phpt).
            let r = mod_close(vm, m);
            fail_reset(vm);
            r?;
            warn(
                vm,
                &format!(
                    "Failed to initialize storage module: {} (path: {})",
                    m.name(),
                    path.to_string_lossy()
                ),
            )?;
            return Ok(false);
        }
        Err(e) => {
            fail_reset(vm);
            return Err(e);
        }
    }
    // $_SESSION exists once the module opened, even if reading fails
    // (tests session_set_save_handler_error2.phpt, class_012).
    bind_vars(vm, PhpArray::new());
    let r = settle_id(vm, m);
    match r {
        Ok(true) => {}
        Ok(false) => {
            let _ = mod_close(vm, m);
            fail_reset(vm);
            return Ok(false);
        }
        Err(e) => {
            fail_reset(vm);
            return Err(e);
        }
    }
    let id = st(vm).id.clone().unwrap_or_default();
    let data = match mod_read(vm, m, &id) {
        Ok(d) => d,
        Err(e) => {
            fail_reset(vm);
            return Err(e);
        }
    };
    let Some(data) = data else {
        let r = mod_close(vm, m);
        fail_reset(vm);
        r?;
        warn(
            vm,
            &format!(
                "Failed to read session data: {} (path: {})",
                m.name(),
                path.to_string_lossy()
            ),
        )?;
        return Ok(false);
    };
    st(vm).data_read = Some(data.clone());
    if !decode_into(vm, &data)? {
        decode_failed(vm)?;
        return Ok(false);
    }
    gc_maybe(vm)?;
    Ok(true)
}

/// The ID for the session being opened: the one set or received, a new
/// one when there is none, it is malformed, or (use_strict_mode) the
/// module does not know it.
fn settle_id(vm: &mut Vm, m: Mod) -> R<bool> {
    let cur = st(vm).id.clone().filter(|i| !i.is_empty());
    let mut need_new = match &cur {
        None => true,
        Some(i) => !valid_id(i) && m == Mod::Files,
    };
    if !need_new
        && ini_bool(vm, "session.use_strict_mode")
        && mod_has_validate(vm, m)
        && let Some(i) = &cur
        && !mod_validate(vm, m, i)?
    {
        need_new = true;
    }
    if need_new {
        let Some(id) = mod_create_sid(vm, m)? else {
            warn(
                vm,
                &format!(
                    "Failed to create session ID: {} (path: {})",
                    m.name(),
                    ini_str(vm, "session.save_path").to_string_lossy()
                ),
            )?;
            return Ok(false);
        };
        let s = st(vm);
        s.id = Some(id);
        s.send_cookie = true;
        s.from_cookie = false;
    }
    Ok(true)
}

/// The session did not start: no ID (test
/// session_set_save_handler_class_005.phpt).
fn fail_reset(vm: &mut Vm) {
    let s = st(vm);
    s.active = false;
    s.id = None;
}

/// Runs the garbage collection with probability gc_probability /
/// gc_divisor (PHP manual: session.gc_probability).
fn gc_maybe(vm: &mut Vm) -> R<()> {
    let p = ini_int(vm, "session.gc_probability");
    let d = ini_int(vm, "session.gc_divisor");
    if p <= 0 || d <= 0 {
        return Ok(());
    }
    let r = u64::from_le_bytes(random_bytes(8).try_into().unwrap_or([0; 8]));
    let x = (r as f64 / u64::MAX as f64) * d as f64;
    if x < p as f64 {
        let m = st(vm).module;
        let life = ini_int(vm, "session.gc_maxlifetime");
        mod_gc(vm, m, life)?;
    }
    Ok(())
}

/// session_decode() and the read data: the variables merge into
/// $_SESSION. Ok(false) for malformed data.
fn decode_into(vm: &mut Vm, data: &[u8]) -> R<bool> {
    let handler = ini_str(vm, "session.serialize_handler");
    let Some(vars) = serial::decode(vm, &handler, data)? else {
        return Ok(false);
    };
    match vars {
        serial::Decoded::Replace(arr) => set_vars(vm, arr),
        serial::Decoded::Merge(list) => {
            let mut arr = vars_array(vm).unwrap_or_default();
            for (k, v) in list {
                arr.insert(Key::from_bytes(&k), v);
            }
            set_vars(vm, arr);
        }
        serial::Decoded::Nothing => {}
    }
    Ok(true)
}

/// "Failed to decode session object": the session is destroyed.
fn decode_failed(vm: &mut Vm) -> R<()> {
    let m = st(vm).module;
    let id = st(vm).id.clone().unwrap_or_default();
    set_vars(vm, PhpArray::new());
    let r1 = mod_destroy(vm, m, &id);
    let r2 = mod_close(vm, m);
    end_session(vm, true);
    r1?;
    r2?;
    warn(
        vm,
        "Failed to decode session object. Session has been destroyed",
    )
}

/// The session is no longer active; `forget` clears the ID too.
fn end_session(vm: &mut Vm, forget: bool) {
    let s = st(vm);
    s.active = false;
    s.data_read = None;
    if forget {
        s.id = None;
    }
    rewrite::session_ended(vm);
}

/// Defines SID and starts the transparent session ID rewriting.
fn publish_id(vm: &mut Vm) {
    let id = st(vm).id.clone().unwrap_or_default();
    let name = ini_str(vm, "session.name");
    let only_cookies = ini_bool(vm, "session.use_only_cookies");
    let cookie = ini_bool(vm, "session.use_cookies") && st(vm).from_cookie;
    let sid = if !only_cookies && !cookie {
        let mut s = name.to_vec();
        s.push(b'=');
        s.extend_from_slice(&id);
        s
    } else {
        Vec::new()
    };
    let key = PhpStr::from("SID");
    let user_defined = vm.constants.contains_key(&key) && !sid_is_internal();
    vm.constants
        .insert(key, Value::Str(PhpStr::from(sid.clone())));
    if !user_defined {
        SID_INTERNAL.with(|c| c.set(true));
    }
    if !sid.is_empty() && ini_bool(vm, "session.use_trans_sid") {
        rewrite::session_started(vm, &name, &id);
    }
}

/// The Set-Cookie header of the session (PHP manual:
/// session_set_cookie_params()).
fn send_cookie(vm: &mut Vm) -> R<()> {
    let name = ini_str(vm, "session.name");
    let id = st(vm).id.clone().unwrap_or_default();
    let mut h = b"Set-Cookie: ".to_vec();
    h.extend_from_slice(&name);
    h.push(b'=');
    h.extend_from_slice(&id);
    let life = ini_int(vm, "session.cookie_lifetime");
    if life > 0 {
        let t = crate::vm::sapi::now().saturating_add(life);
        h.extend_from_slice(b"; expires=");
        h.extend_from_slice(http_date(t).as_bytes());
        h.extend_from_slice(format!("; Max-Age={life}").as_bytes());
    }
    for (k, attr) in [
        ("session.cookie_path", "path"),
        ("session.cookie_domain", "domain"),
    ] {
        let v = ini_str(vm, k);
        if !v.is_empty() {
            h.extend_from_slice(format!("; {attr}=").as_bytes());
            h.extend_from_slice(&v);
        }
    }
    if ini_bool(vm, "session.cookie_secure") {
        h.extend_from_slice(b"; secure");
    }
    if ini_bool(vm, "session.cookie_httponly") {
        h.extend_from_slice(b"; HttpOnly");
    }
    let ss = ini_str(vm, "session.cookie_samesite");
    if !ss.is_empty() {
        h.extend_from_slice(b"; SameSite=");
        h.extend_from_slice(&ss);
    }
    if ini_bool(vm, "session.cookie_partitioned") {
        h.extend_from_slice(b"; Partitioned");
    }
    // The CLI keeps no headers (observed on PHP 8.5: headers_list() is
    // empty).
    if vm.out.sapi.server {
        super::headers::add_header(vm, &h, false, 0)?;
    }
    Ok(())
}

/// An RFC 7231 date.
fn http_date(t: i64) -> String {
    use crate::vm::sapi::{DAYS, MONTHS, civil};
    let (y, mo, d, h, mi, s, wd) = civil(t);
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        DAYS[wd as usize],
        d,
        MONTHS[mo as usize - 1],
        y,
        h,
        mi,
        s
    )
}

/// The cache limiter headers (PHP manual: session_cache_limiter()).
/// Ok(false) when the headers were sent already.
fn cache_limiter(vm: &mut Vm) -> R<bool> {
    let lim = ini_str(vm, "session.cache_limiter");
    if lim.is_empty() {
        return Ok(true);
    }
    if headers_sent(vm) {
        let at = sent_from(vm);
        warn(
            vm,
            &format!(
                "Session cache limiter cannot be sent after headers have already been sent{at}"
            ),
        )?;
        return Ok(false);
    }
    let expire = ini_int(vm, "session.cache_expire") * 60;
    let past = "Expires: Thu, 19 Nov 1981 08:52:00 GMT";
    let mut lines: Vec<String> = Vec::new();
    let last_modified = |vm: &Vm| -> Option<String> {
        use std::os::unix::ffi::OsStrExt;
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&vm.script_path));
        let m = std::fs::metadata(p).ok()?.modified().ok()?;
        let t = m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
        Some(format!("Last-Modified: {}", http_date(t)))
    };
    match &lim[..] {
        b"public" => {
            let t = crate::vm::sapi::now() + expire;
            lines.push(format!("Expires: {}", http_date(t)));
            lines.push(format!("Cache-Control: public, max-age={expire}"));
            lines.extend(last_modified(vm));
        }
        b"private" => {
            lines.push(past.to_string());
            lines.push(format!("Cache-Control: private, max-age={expire}"));
            lines.extend(last_modified(vm));
        }
        b"private_no_expire" => {
            lines.push(format!("Cache-Control: private, max-age={expire}"));
            lines.extend(last_modified(vm));
        }
        b"nocache" => {
            lines.push(past.to_string());
            lines.push("Cache-Control: no-store, no-cache, must-revalidate".to_string());
            lines.push("Pragma: no-cache".to_string());
        }
        _ => {
            return Ok(true);
        }
    }
    if vm.out.sapi.server {
        for l in lines {
            super::headers::add_header(vm, l.as_bytes(), true, 0)?;
        }
    }
    Ok(true)
}

/// Starts the session (session_start() and session.auto_start).
pub(crate) fn php_session_start(vm: &mut Vm, read_and_close: bool) -> R<bool> {
    let Some(m) = module_of(vm) else {
        let h = ini_str(vm, "session.save_handler");
        warn(
            vm,
            &format!(
                "Cannot find session save handler \"{}\" - session startup failed",
                h.to_string_lossy()
            ),
        )?;
        return Ok(false);
    };
    let ser = ini_str(vm, "session.serialize_handler");
    if !serial::known(&ser) {
        warn(
            vm,
            &format!(
                "Cannot find session serialization handler \"{}\" - session startup failed",
                ser.to_string_lossy()
            ),
        )?;
        return Ok(false);
    }
    if ini_bool(vm, "session.cookie_partitioned") && !ini_bool(vm, "session.cookie_secure") {
        warn(
            vm,
            "Partitioned session cookie cannot be used without also configuring it as secure",
        )?;
        return Ok(false);
    }
    {
        let s = st(vm);
        s.module = m;
        s.active = true;
        s.send_cookie = false;
        s.from_cookie = false;
    }
    if st(vm).id.as_ref().is_none_or(|i| i.is_empty()) {
        let rid = request_id(vm);
        st(vm).id = rid;
    }
    if st(vm).id.is_none() {
        st(vm).send_cookie = true;
    }
    if !initialize(vm)? {
        return Ok(false);
    }
    if ini_bool(vm, "session.use_cookies") && (st(vm).send_cookie || !st(vm).from_cookie) {
        send_cookie(vm)?;
    }
    if !cache_limiter(vm)? {
        let m = st(vm).module;
        let r = mod_close(vm, m);
        end_session(vm, false);
        r?;
        return Ok(false);
    }
    publish_id(vm);
    if read_and_close {
        let m = st(vm).module;
        let r = mod_close(vm, m);
        end_session(vm, false);
        r?;
    }
    Ok(true)
}

fn session_start(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if st(vm).active {
        let at = started_from(vm);
        raise(
            vm,
            E_NOTICE,
            &format!("Ignoring session_start() because a session is already active{at}"),
        )?;
        return Ok(Value::Bool(true));
    }
    if ini_bool(vm, "session.use_cookies") && headers_sent(vm) {
        let at = sent_from(vm);
        warn(
            vm,
            &format!("Session cannot be started after headers have already been sent{at}"),
        )?;
        return Ok(Value::Bool(false));
    }
    let mut read_and_close = false;
    if a.has(0) {
        let opts = a.array(0);
        for (k, v) in opts.iter() {
            let goro_runtime::KeyRef::Str(k) = k else {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #1 ($options) must be of type array with keys as string",
                );
            };
            let v = v.deref();
            let key = k.to_string_lossy();
            let sv = match &v {
                Value::Str(s) => s.clone(),
                Value::Int(i) => PhpStr::from(i.to_string()),
                Value::Bool(b) => PhpStr::from(if *b { "1" } else { "" }),
                other => {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        &format!(
                            "Option \"{}\" must be of type string|int|bool, {} given",
                            key,
                            crate::class::TypeName::tname(other)
                        ),
                    );
                }
            };
            if key == "read_and_close" {
                read_and_close = match &v {
                    Value::Str(s) => match goro_runtime::numeric::is_numeric_str(s) {
                        true => goro_runtime::convert::str_to_float(s) >= 1.0,
                        false => {
                            return vm.fthrow(
                                    a,
                                    "TypeError",
                                    &format!(
                                        "Option \"read_and_close\" value must be of type compatible with int, \"{}\" given",
                                        s.to_string_lossy()
                                    ),
                                );
                        }
                    },
                    // A positive int (test session_start_read_and_close.phpt:
                    // -1 keeps the session open).
                    Value::Int(i) => *i > 0,
                    Value::Bool(b) => *b,
                    _ => false,
                };
                continue;
            }
            let name = format!("session.{key}");
            let ok = vm.get_ini(name.as_bytes()).is_some()
                && ini::set_internal(vm, name.as_bytes(), &sv)?;
            if !ok {
                warn(vm, &format!("Setting option \"{key}\" failed"))?;
            }
        }
    }
    let file = vm.cur_file();
    let line = vm.cur_line();
    st(vm).started_at = Some((file, line));
    Ok(Value::Bool(php_session_start(vm, read_and_close)?))
}

// ---------------------------------------------------------------- write

/// Encodes $_SESSION with the configured serializer; None when it cannot
/// be (reported).
fn encode_vars(vm: &mut Vm, for_write: bool) -> R<Option<Vec<u8>>> {
    let handler = ini_str(vm, "session.serialize_handler");
    let vars = st(vm).vars.as_ref().map(|c| c.get());
    match vars {
        Some(Value::Array(a)) => serial::encode(vm, &handler, &a, for_write),
        _ => Ok(None),
    }
}

/// Writes the session data (or updates its timestamp).
fn save_state(vm: &mut Vm) -> R<()> {
    let m = st(vm).module;
    let id = st(vm).id.clone().unwrap_or_default();
    let is_array = matches!(st(vm).vars.as_ref().map(|c| c.get()), Some(Value::Array(_)));
    let data = if is_array {
        match encode_vars(vm, true)? {
            Some(d) => d,
            None => return Ok(()),
        }
    } else {
        Vec::new()
    };
    let lazy = ini_bool(vm, "session.lazy_write");
    let unchanged = !data.is_empty() && st(vm).data_read.as_deref() == Some(&data[..]);
    let (ok, which) = if lazy && unchanged {
        match mod_update_timestamp(vm, m, &id, &data)? {
            Some(ok) => (ok, user::UPDATE),
            None => (mod_write(vm, m, &id, &data)?, user::WRITE),
        }
    } else {
        (mod_write(vm, m, &id, &data)?, user::WRITE)
    };
    if !ok {
        let path = ini_str(vm, "session.save_path").to_string_lossy();
        match m {
            Mod::Files => warn(
                vm,
                &format!(
                    "Failed to write session data (files). Please verify that the current setting of session.save_path is correct ({path})"
                ),
            )?,
            Mod::User => {
                let h = user::callback_name(vm, which);
                warn(
                    vm,
                    &format!(
                        "Failed to write session data using user defined save handler. (session.save_path: {path}, handler: {h})"
                    ),
                )?
            }
        }
    }
    Ok(())
}

/// Writes and closes the active session. A write that throws or exits
/// skips the close (test user_session_module/bug60634.phpt).
fn flush(vm: &mut Vm) -> R<()> {
    let r = save_state(vm);
    if r.is_err() {
        end_session(vm, false);
        return r;
    }
    let m = st(vm).module;
    let c = mod_close(vm, m);
    end_session(vm, false);
    r?;
    c?;
    Ok(())
}

fn session_write_close(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !st(vm).active {
        return Ok(Value::Bool(false));
    }
    flush(vm)?;
    Ok(Value::Bool(true))
}

fn session_abort(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !st(vm).active {
        return Ok(Value::Bool(false));
    }
    let m = st(vm).module;
    let r = mod_close(vm, m);
    end_session(vm, false);
    r?;
    Ok(Value::Bool(true))
}

fn session_reset(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !st(vm).active {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(initialize(vm)?))
}

fn session_destroy(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !st(vm).active {
        warn(vm, "Trying to destroy uninitialized session")?;
        return Ok(Value::Bool(false));
    }
    let m = st(vm).module;
    let id = st(vm).id.clone().unwrap_or_default();
    let ok = match mod_destroy(vm, m, &id) {
        Ok(ok) => ok,
        Err(e) => {
            let _ = mod_close(vm, m);
            end_session(vm, true);
            return Err(e);
        }
    };
    let c = mod_close(vm, m);
    end_session(vm, true);
    st(vm).vars = None;
    c?;
    if !ok {
        warn(vm, "Session object destruction failed")?;
    }
    Ok(Value::Bool(ok))
}

fn session_unset(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !st(vm).active {
        return Ok(Value::Bool(false));
    }
    if let Some(c) = st(vm).vars.clone()
        && matches!(c.get(), Value::Array(_))
    {
        drop(c.set(Value::Array(PhpArray::new())));
    }
    Ok(Value::Bool(true))
}

fn session_gc(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !st(vm).active {
        warn(
            vm,
            "Session cannot be garbage collected when there is no active session",
        )?;
        return Ok(Value::Bool(false));
    }
    let m = st(vm).module;
    let life = ini_int(vm, "session.gc_maxlifetime");
    Ok(match mod_gc(vm, m, life)? {
        Some(n) => Value::Int(n),
        None => Value::Bool(false),
    })
}

/// PHP_SESSION_DISABLED when no save handler module is usable (test
/// session_status_disabled.phpt).
fn session_status(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if st(vm).active {
        return Ok(Value::Int(2));
    }
    let h = ini_str(vm, "session.save_handler");
    Ok(Value::Int(if matches!(&h[..], b"files" | b"user") {
        1
    } else {
        0
    }))
}

fn session_register_shutdown(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    register_shutdown(vm);
    Ok(Value::Null)
}

/// Writes the session from a shutdown function (session_set_save_handler()
/// with $register_shutdown).
pub(crate) fn register_shutdown(vm: &mut Vm) {
    if st(vm).shutdown_registered {
        return;
    }
    st(vm).shutdown_registered = true;
    vm.shutdown_functions
        .push((Value::Str(PhpStr::from("session_write_close")), Vec::new()));
}

/// The end of the request: an active session is written and closed;
/// settings changed at run time are restored.
fn rshutdown(vm: &mut Vm) {
    st(vm).phase = Phase::Shutdown;
    if st(vm).active {
        let r = flush(vm);
        let thrown = matches!(r, Err(crate::vm::Unwind::Throw(_)));
        vm.handle_unwind(r);
        if thrown {
            // (test user_session_module/bug60634_error_4.phpt)
            let _ = warn(vm, "Cannot call session save handler in a recursive manner");
        }
    }
    ini::shutdown_restore(vm);
    st(vm).phase = Phase::Run;
}

// ---------------------------------------------------------------- encode / decode

/// The variables of the last session, active or not; none after
/// session_destroy() (test session_encode_variation1.phpt).
fn session_encode(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let ok = vars_array(vm).is_some();
    if !ok {
        warn(vm, "Cannot encode non-existent session")?;
        return Ok(Value::Bool(false));
    }
    // No variables encode to nothing, which is false (tests
    // session_encode_variation*.phpt).
    Ok(match encode_vars(vm, false)? {
        Some(d) if !d.is_empty() => Value::Str(PhpStr::from(d)),
        _ => Value::Bool(false),
    })
}

fn session_decode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !st(vm).active {
        warn(
            vm,
            "Session data cannot be decoded when there is no active session",
        )?;
        return Ok(Value::Bool(false));
    }
    let data = a.str(0);
    if !decode_into(vm, &data)? {
        decode_failed(vm)?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- IDs

fn session_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = st(vm).id.clone().unwrap_or_default();
    if let Some(new) = a.opt_str(0) {
        if active_refusal(vm, "Session ID cannot be changed")? {
            return Ok(Value::Bool(false));
        }
        if ini_bool(vm, "session.use_cookies") && sent_refusal(vm, "Session ID cannot be changed")?
        {
            return Ok(Value::Bool(false));
        }
        // The ID is a C string (observed on PHP 8.5: "a\0b" sets "a").
        let new = new.split(|&c| c == 0).next().unwrap_or(&[]).to_vec();
        st(vm).id = if new.is_empty() {
            None
        } else {
            Some(PhpStr::from(new))
        };
    }
    Ok(Value::Str(old))
}

fn session_regenerate_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let delete_old = a.bool_or(0, false);
    if !st(vm).active {
        warn(
            vm,
            "Session ID cannot be regenerated when there is no active session",
        )?;
        return Ok(Value::Bool(false));
    }
    if headers_sent(vm) {
        let at = sent_from(vm);
        warn(
            vm,
            &format!("Session ID cannot be regenerated after headers have already been sent{at}"),
        )?;
        return Ok(Value::Bool(false));
    }
    let m = st(vm).module;
    let path = ini_str(vm, "session.save_path").to_string_lossy();
    let name = ini_str(vm, "session.name");
    if delete_old {
        let id = st(vm).id.clone().unwrap_or_default();
        if !mod_destroy(vm, m, &id)? {
            warn(
                vm,
                &format!(
                    "Session object destruction failed. ID: {} (path: {})",
                    m.name(),
                    path
                ),
            )?;
            return Ok(Value::Bool(false));
        }
    } else {
        save_state(vm)?;
    }
    mod_close(vm, m)?;
    st(vm).data_read = None;
    if !mod_open(vm, m, &ini_str(vm, "session.save_path"), &name)? {
        end_session(vm, false);
        warn(
            vm,
            &format!("Failed to open session: {} (path: {})", m.name(), path),
        )?;
        return Ok(Value::Bool(false));
    }
    let mut new = None;
    for _ in 0..3 {
        let Some(id) = mod_create_sid(vm, m)? else {
            break;
        };
        if ini_bool(vm, "session.use_strict_mode")
            && mod_has_validate(vm, m)
            && mod_validate(vm, m, &id)?
        {
            continue;
        }
        new = Some(id);
        break;
    }
    let Some(id) = new else {
        let _ = mod_close(vm, m);
        end_session(vm, false);
        vm.throw(
            "Error",
            &format!(
                "Failed to create new session ID: {} (path: {})",
                m.name(),
                path
            ),
        )?;
        return Ok(Value::Bool(false));
    };
    st(vm).id = Some(id.clone());
    match mod_read(vm, m, &id)? {
        Some(d) => st(vm).data_read = Some(d),
        None => {
            let _ = mod_close(vm, m);
            end_session(vm, false);
            warn(
                vm,
                &format!(
                    "Failed to create(read) session ID: {} (path: {})",
                    m.name(),
                    path
                ),
            )?;
            return Ok(Value::Bool(false));
        }
    }
    st(vm).from_cookie = false;
    if ini_bool(vm, "session.use_cookies") {
        send_cookie(vm)?;
    }
    rewrite::session_ended(vm);
    publish_id(vm);
    Ok(Value::Bool(true))
}

fn session_create_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let prefix = a.str(0);
    if prefix.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($prefix) must not contain any null bytes",
        );
    }
    if prefix.len() > 256 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($prefix) cannot be longer than 256 characters",
        );
    }
    if !prefix.is_empty() && !valid_id(&prefix) {
        warn(
            vm,
            "Prefix cannot contain special characters. Only the A-Z, a-z, 0-9, \"-\", and \",\" characters are allowed",
        )?;
        return Ok(Value::Bool(false));
    }
    let id = if st(vm).active {
        let m = st(vm).module;
        let mut got = None;
        for _ in 0..3 {
            let Some(id) = mod_create_sid(vm, m)? else {
                break;
            };
            if mod_has_validate(vm, m) && mod_validate(vm, m, &id)? {
                continue;
            }
            got = Some(id);
            break;
        }
        got
    } else {
        Some(new_id(vm))
    };
    let Some(id) = id else {
        warn(vm, "Failed to create new ID")?;
        return Ok(Value::Bool(false));
    };
    let mut out = prefix.to_vec();
    out.extend_from_slice(&id);
    Ok(Value::Str(PhpStr::from(out)))
}

// ---------------------------------------------------------------- settings

/// A string setting's getter/setter (session_name(), session_save_path(),
/// session_cache_limiter()).
fn setting_fn(vm: &mut Vm, a: &mut Args, key: &str, what: &str, arg: &str) -> R<Value> {
    let old = ini_str(vm, key);
    let Some(new) = a.opt_str(0) else {
        return Ok(Value::Str(old));
    };
    if active_refusal(vm, &format!("{what} cannot be changed"))? {
        return Ok(Value::Bool(false));
    }
    if sent_refusal(vm, &format!("{what} cannot be changed"))? {
        return Ok(Value::Bool(false));
    }
    if !arg.is_empty() && new.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #1 (${arg}) must not contain any null bytes"),
        );
    }
    ini::set_internal(vm, key.as_bytes(), &new)?;
    Ok(Value::Str(old))
}

fn session_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    setting_fn(vm, a, "session.name", "Session name", "name")
}

fn session_save_path(vm: &mut Vm, a: &mut Args) -> R<Value> {
    setting_fn(vm, a, "session.save_path", "Session save path", "path")
}

fn session_cache_limiter(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(v) = a.opt_str(0)
        && v.contains(&0)
    {
        // A C string (observed on PHP 8.5).
        let cut = v.split(|&c| c == 0).next().unwrap_or(&[]).to_vec();
        a.vals[0] = Value::Str(PhpStr::from(cut));
    }
    setting_fn(vm, a, "session.cache_limiter", "Session cache limiter", "")
}

fn session_cache_expire(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = ini_int(vm, "session.cache_expire");
    let Some(new) = a.opt_int(0) else {
        return Ok(Value::Int(old));
    };
    if active_refusal(vm, "Session cache expiration cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    if sent_refusal(vm, "Session cache expiration cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    ini::set_internal(vm, b"session.cache_expire", new.to_string().as_bytes())?;
    Ok(Value::Int(old))
}

fn session_module_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = ini_str(vm, "session.save_handler");
    let Some(new) = a.opt_str(0) else {
        return Ok(Value::Str(old));
    };
    if active_refusal(vm, "Session save handler module cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    if sent_refusal(vm, "Session save handler module cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    let new = new.split(|&c| c == 0).next().unwrap_or(&[]).to_vec();
    if new == b"user" {
        return vm.fthrow(a, "ValueError", "Argument #1 ($module) cannot be \"user\"");
    }
    if new != b"files" {
        warn(
            vm,
            &format!(
                "Session handler module \"{}\" cannot be found",
                String::from_utf8_lossy(&new)
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    if old.as_bytes() == b"user" {
        // Back to a built-in module: the user handler is dropped.
        st(vm).user = None;
    }
    ini::store(vm, b"session.save_handler", &new);
    Ok(Value::Str(old))
}

fn session_get_cookie_params(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut arr = PhpArray::new();
    arr.insert(
        Key::from("lifetime"),
        Value::Int(ini_int(vm, "session.cookie_lifetime")),
    );
    arr.insert(
        Key::from("path"),
        Value::Str(ini_str(vm, "session.cookie_path")),
    );
    arr.insert(
        Key::from("domain"),
        Value::Str(ini_str(vm, "session.cookie_domain")),
    );
    arr.insert(
        Key::from("secure"),
        Value::Bool(ini_bool(vm, "session.cookie_secure")),
    );
    arr.insert(
        Key::from("partitioned"),
        Value::Bool(ini_bool(vm, "session.cookie_partitioned")),
    );
    arr.insert(
        Key::from("httponly"),
        Value::Bool(ini_bool(vm, "session.cookie_httponly")),
    );
    arr.insert(
        Key::from("samesite"),
        Value::Str(ini_str(vm, "session.cookie_samesite")),
    );
    Ok(Value::Array(arr))
}

fn session_set_cookie_params(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !ini_bool(vm, "session.use_cookies") {
        warn(
            vm,
            "Session cookies cannot be used when session.use_cookies is disabled",
        )?;
        return Ok(Value::Bool(false));
    }
    if active_refusal(vm, "Session cookie parameters cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    if sent_refusal(vm, "Session cookie parameters cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    let bstr = |b: bool| PhpStr::from(if b { "1" } else { "0" });
    let mut sets: Vec<(&'static str, PhpStr)> = Vec::new();
    match a.val(0) {
        Value::Array(opts) => {
            for (i, n) in ["path", "domain", "secure", "httponly"].iter().enumerate() {
                if !matches!(a.get(i + 1).map(|v| v.deref()), None | Some(Value::Null)) {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        &format!(
                            "Argument #{} (${}) must be null when argument #1 ($lifetime_or_options) is an array",
                            i + 2,
                            n
                        ),
                    );
                }
            }
            let mut found = false;
            for (k, v) in opts.iter() {
                let v = v.deref();
                let kname = match k {
                    goro_runtime::KeyRef::Str(s) => s.to_string_lossy(),
                    goro_runtime::KeyRef::Int(i) => i.to_string(),
                };
                let key: &'static str = match kname.to_ascii_lowercase().as_str() {
                    "lifetime" => "session.cookie_lifetime",
                    "path" => "session.cookie_path",
                    "domain" => "session.cookie_domain",
                    "secure" => "session.cookie_secure",
                    "partitioned" => "session.cookie_partitioned",
                    "httponly" => "session.cookie_httponly",
                    "samesite" => "session.cookie_samesite",
                    _ => {
                        warn(
                            vm,
                            &format!(
                                "Argument #1 ($lifetime_or_options) contains an unrecognized key \"{kname}\""
                            ),
                        )?;
                        continue;
                    }
                };
                found = true;
                let s = match key {
                    "session.cookie_secure"
                    | "session.cookie_partitioned"
                    | "session.cookie_httponly" => bstr(vm.to_bool(&v)),
                    "session.cookie_lifetime" => PhpStr::from(vm.to_int(&v)?.to_string()),
                    _ => vm.to_php_str(&v)?,
                };
                sets.push((key, s));
            }
            if !found {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #1 ($lifetime_or_options) must contain at least 1 valid key",
                );
            }
        }
        v => {
            let life = vm.to_int(&v)?;
            sets.push(("session.cookie_lifetime", PhpStr::from(life.to_string())));
            if let Some(p) = a.opt_str(1) {
                sets.push(("session.cookie_path", p));
            }
            if let Some(d) = a.opt_str(2) {
                sets.push(("session.cookie_domain", d));
            }
            for (i, k) in [(3, "session.cookie_secure"), (4, "session.cookie_httponly")] {
                if let Some(v) = a.get(i).map(|v| v.deref())
                    && !matches!(v, Value::Null)
                {
                    sets.push((k, bstr(vm.to_bool(&v))));
                }
            }
        }
    }
    for (k, v) in sets {
        if !ini::set_internal(vm, k.as_bytes(), &v)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}
