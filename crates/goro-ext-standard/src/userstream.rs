//! Userspace stream wrappers: a port of `main/streams/userspace.c` together
//! with the parts of the generic stream layer (`main/streams/streams.c`) that
//! user streams depend on (read buffering, line reading, seeking, flushing,
//! wrapper lookup and wrapper error reporting).
//!
//! Integration: every stream / filesystem builtin that may receive a user
//! stream resource or a `scheme://` URL served by a class registered with
//! `stream_wrapper_register()` is wrapped (like `basedir_guard`). The wrapper
//! dispatches to the user stream implementation here and otherwise defers to
//! the original builtin, so plain-file behaviour is untouched.
//!
//! This is a child module of `misc` so it can reach the file-handle tables.

use super::*;
use goro_core::vm::BuiltinFn;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants (main/php_streams.h)
// ---------------------------------------------------------------------------

const USE_PATH: i64 = 0x1;
const REPORT_ERRORS: i64 = 0x8;
const STREAM_OPEN_FOR_INCLUDE: i64 = 0x80;
const STREAM_OPEN_FOR_ZEND_STREAM: i64 = 0x10000;

const PHP_STREAM_URL_STAT_LINK: i64 = 1;
const PHP_STREAM_URL_STAT_QUIET: i64 = 2;

const PHP_STREAM_OPTION_BLOCKING: i64 = 1;
const PHP_STREAM_OPTION_READ_BUFFER: i64 = 2;
const PHP_STREAM_OPTION_WRITE_BUFFER: i64 = 3;
const PHP_STREAM_OPTION_READ_TIMEOUT: i64 = 4;

const OPT_OK: i32 = 0;
const OPT_ERR: i32 = -1;
const OPT_NOTIMPL: i32 = -2;

const PHP_STREAM_META_TOUCH: i64 = 1;
const PHP_STREAM_META_OWNER_NAME: i64 = 2;
const PHP_STREAM_META_OWNER: i64 = 3;
const PHP_STREAM_META_GROUP_NAME: i64 = 4;
const PHP_STREAM_META_GROUP: i64 = 5;
const PHP_STREAM_META_ACCESS: i64 = 6;

const FLAG_NO_SEEK: u32 = 0x1;
const FLAG_NO_BUFFER: u32 = 0x2;
const FLAG_IS_DIR: u32 = 0x40;
const FLAG_NO_FCLOSE: u32 = 0x80;
const FLAG_WAS_WRITTEN: u32 = 0x8000_0000;

const CHUNK_SIZE: usize = 8192;
const BUFSIZ: i64 = 8192;

const SEEK_SET: i64 = 0;
const SEEK_CUR: i64 = 1;
const SEEK_END: i64 = 2;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A user-space stream (`php_stream` + `php_userstream_data_t`).
pub(crate) struct UserStream {
    /// The wrapper instance (`us->object`, also `stream->wrapperdata`).
    pub obj: Value,
    /// Name of the wrapper class, for diagnostics.
    pub class_name: Vec<u8>,
    pub readbuf: Vec<u8>,
    pub readpos: usize,
    pub position: i64,
    pub eof: bool,
    pub fatal_error: bool,
    pub chunk_size: usize,
    pub flags: u32,
    pub mode: String,
    pub orig_path: String,
    pub in_free: bool,
    pub closed: bool,
    /// The wrapper was registered with STREAM_IS_URL.
    pub is_url: bool,
    /// Ops of a natively implemented stream (instead of `obj`'s methods).
    pub native: Option<Rc<RefCell<dyn NativeStream>>>,
    /// wops->label of the wrapper that opened a native stream (None when
    /// it was opened without one, e.g. gzopen()).
    pub wrapper_label: Option<&'static str>,
}

pub(crate) type Us = Rc<RefCell<UserStream>>;

/// php_stream_ops of a stream implemented in Rust but served by this
/// stream layer (buffering, line reading, positions...), e.g. the
/// compress.zlib:// streams.
pub(crate) trait NativeStream {
    /// ops->read(): None == -1. Sets `*eof` like stream->eof.
    fn read(&mut self, vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>>;
    /// ops->write(): bytes written, < 0 on error.
    fn write(&mut self, vm: &mut Vm, buf: &[u8]) -> i64;
    /// ops->close()
    fn close(&mut self, vm: &mut Vm);
    /// ops->flush(): 0 on success.
    fn flush(&mut self, vm: &mut Vm) -> i32;
    /// ops->seek(): the new offset, None on failure.
    fn seek(&mut self, vm: &mut Vm, offset: i64, whence: i64) -> Option<i64>;
    /// set_option(PHP_STREAM_OPTION_LOCKING, value)
    fn lock(&mut self, vm: &mut Vm, value: i64) -> i32;
    /// ops->label
    fn label(&self) -> &'static str;
}

fn native_of(us: &Us) -> Option<Rc<RefCell<dyn NativeStream>>> {
    us.borrow().native.clone()
}

impl UserStream {
    fn buffered(&self) -> usize {
        self.readbuf.len() - self.readpos
    }
}

thread_local! {
    static USER_STREAMS: RefCell<HashMap<i64, Us>> = RefCell::new(HashMap::new());
    static ORIG: RefCell<HashMap<&'static str, BuiltinFn>> = RefCell::new(HashMap::new());
    /// Stack of the builtin names currently executing through this module,
    /// used to prefix diagnostics like php_error_docref() does.
    static FN_STACK: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    /// FG(wrapper_errors): errors logged by wrappers while REPORT_ERRORS is off.
    static WRAPPER_ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// FG(user_stream_current_filename) (infinite recursion guard).
    static CURRENT_FILENAME: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Builtin wrappers removed with stream_wrapper_unregister().
    static DISABLED_BUILTINS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    /// BG(CurrentStatFile)/BG(ssb) and BG(CurrentLStatFile)/BG(lssb).
    static STAT_CACHE: RefCell<Option<(Vec<u8>, StatBuf)>> = const { RefCell::new(None) };
    static LSTAT_CACHE: RefCell<Option<(Vec<u8>, StatBuf)>> = const { RefCell::new(None) };
}

pub(crate) fn get_us(fid: i64) -> Option<Us> {
    USER_STREAMS.with(|m| m.borrow().get(&fid).cloned())
}

fn fid_of(v: &Value) -> Option<i64> {
    match v {
        Value::Resource(n) => Some(*n),
        Value::Reference(r) => fid_of(&r.borrow()),
        _ => None,
    }
}

/// User stream behind a builtin argument (not a directory stream).
fn us_arg(args: &[Value], i: usize) -> Option<(i64, Us)> {
    let fid = fid_of(args.get(i)?)?;
    let us = get_us(fid)?;
    if us.borrow().flags & FLAG_IS_DIR != 0 {
        return None;
    }
    Some((fid, us))
}

fn cur_fn() -> &'static str {
    FN_STACK.with(|s| s.borrow().last().copied().unwrap_or(""))
}

/// php_error_docref(NULL, E_WARNING, ...)
fn warn(vm: &mut Vm, msg: &str) {
    let f = cur_fn();
    if f.is_empty() {
        vm.emit_warning(msg);
    } else {
        vm.emit_warning(&format!("{}(): {}", f, msg));
    }
}

fn notice(vm: &mut Vm, msg: &str) {
    let f = cur_fn();
    let line = vm.current_line;
    if f.is_empty() {
        vm.emit_notice_at(msg, line);
    } else {
        vm.emit_notice_at(&format!("{}(): {}", f, msg), line);
    }
}

fn exc_err(vm: &Vm) -> VmError {
    VmError {
        message: "exception".into(),
        line: vm.current_line,
    }
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = if class == b"TypeError" {
        vm.throw_type_error(msg.to_string())
    } else {
        vm.create_exception(class, msg, vm.current_line)
    };
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

fn s(v: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(v))
}

fn lossy(v: &[u8]) -> String {
    String::from_utf8_lossy(v).to_string()
}

fn arg_str(args: &[Value], i: usize) -> Option<Vec<u8>> {
    match args.get(i)?.deref() {
        Value::Null | Value::Undef => None,
        Value::Resource(_) | Value::Array(_) => None,
        v => Some(v.to_php_string().as_bytes().to_vec()),
    }
}

fn new_arr() -> PhpArray {
    PhpArray::new()
}

fn arr_val(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

// ---------------------------------------------------------------------------
// Calling into the wrapper object (zend_call_method_if_exists)
// ---------------------------------------------------------------------------

enum CallRes {
    /// The method does not exist (zend_call_method_if_exists() == FAILURE).
    NotImpl,
    /// The call happened but produced no value (an exception was thrown).
    Undef,
    Val(Value),
}

fn class_lower_of(obj: &Value) -> Vec<u8> {
    match obj {
        Value::Object(o) => o
            .borrow()
            .class_name
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect(),
        _ => Vec::new(),
    }
}

fn has_method(vm: &Vm, obj: &Value, name: &[u8]) -> bool {
    let cl = class_lower_of(obj);
    match vm.classes.get(&cl) {
        Some(c) => c.get_method(name).is_some() || c.get_method(b"__call").is_some(),
        None => false,
    }
}

/// Invoke a method directly (no visibility check, like
/// zend_call_known_instance_method), with by-ref parameter support.
fn invoke_method(
    vm: &mut Vm,
    obj: &Value,
    method_lower: &[u8],
    args: Vec<Value>,
) -> Result<Value, VmError> {
    // Diagnostics raised after the call are attributed to the caller's line.
    let saved_line = vm.current_line;
    let saved_er = vm.error_reporting;
    let r = invoke_method_inner(vm, obj, method_lower, args);
    if vm.current_exception.is_none() {
        vm.current_line = saved_line;
    }
    if r.is_err() {
        // Unwinding out of an @-silenced region restores error_reporting.
        vm.error_reporting = saved_er;
    }
    r
}

fn invoke_method_inner(
    vm: &mut Vm,
    obj: &Value,
    method_lower: &[u8],
    args: Vec<Value>,
) -> Result<Value, VmError> {
    let cl = class_lower_of(obj);
    let class = match vm.classes.get(&cl) {
        Some(c) => c,
        None => return Ok(Value::Null),
    };
    let class_name = class.name.clone();
    let (op, declaring, is_static) = match class.get_method(method_lower) {
        Some(m) => (m.op_array.clone(), m.declaring_class.clone(), m.is_static),
        None => {
            // __call($name, $args)
            let m = match class.get_method(b"__call") {
                Some(m) => m,
                None => return Ok(Value::Null),
            };
            let op = m.op_array.clone();
            let declaring = m.declaring_class.clone();
            let mut a = new_arr();
            for v in args {
                a.push(v.deref());
            }
            let display = vm
                .classes
                .get(&cl)
                .and_then(|c| c.get_method(method_lower).map(|m| m.name.clone()))
                .unwrap_or_else(|| method_lower.to_vec());
            let margs = vec![Value::String(PhpString::from_vec(display)), arr_val(a)];
            vm.called_class_stack.push(class_name);
            vm.push_class_scope(declaring);
            let r = vm.execute_fn_with_named_args(&op, margs, vec![], Some(obj.clone()));
            vm.pop_class_scope();
            vm.called_class_stack.pop();
            return r;
        }
    };
    let declared = (op.param_count as usize).saturating_sub(if is_static { 0 } else { 1 });
    let required = op.required_param_count as usize;
    if args.len() < required {
        let decl_lower: Vec<u8> = declaring.iter().map(|b| b.to_ascii_lowercase()).collect();
        let decl_name = vm
            .classes
            .get(&decl_lower)
            .map(|c| c.name.clone())
            .unwrap_or(class_name.clone());
        let mname = vm
            .classes
            .get(&cl)
            .and_then(|c| c.get_method(method_lower))
            .map(|m| m.name.clone())
            .unwrap_or_default();
        let q = if declared == required {
            "exactly"
        } else {
            "at least"
        };
        let msg = format!(
            "Too few arguments to function {}::{}(), {} passed and {} {} expected",
            goro_core::value::display_class_name(&decl_name),
            lossy(&mname),
            args.len(),
            q,
            required
        );
        return Err(throw(vm, b"ArgumentCountError", &msg));
    }
    // Pass references only to by-ref parameters; drop surplus arguments
    // unless the method is variadic.
    // by_ref_params is indexed by CV slot, which counts the hidden `$this`.
    let hidden = (op.param_count as usize).saturating_sub(op.param_attributes.len());
    let mut call_args = Vec::with_capacity(args.len());
    for (i, a) in args.into_iter().enumerate() {
        if i >= declared && op.variadic_param.is_none() {
            break;
        }
        let by_ref = op.by_ref_params.get(i + hidden).copied().unwrap_or(false);
        if by_ref {
            call_args.push(a);
        } else {
            call_args.push(a.deref());
        }
    }
    let this = if is_static { None } else { Some(obj.clone()) };
    vm.called_class_stack.push(class_name);
    vm.push_class_scope(declaring);
    let r = vm.execute_fn_with_named_args(&op, call_args, vec![], this);
    vm.pop_class_scope();
    vm.called_class_stack.pop();
    r
}

/// Take the unread data of a user stream's read buffer.
pub(crate) fn take_read_buffer(us: &Us) -> Vec<u8> {
    let mut b = us.borrow_mut();
    let rp = b.readpos;
    let mut v = std::mem::take(&mut b.readbuf);
    v.drain(..rp.min(v.len()));
    b.readpos = 0;
    v
}

/// Replace a user stream's read buffer contents.
pub(crate) fn put_read_buffer(us: &Us, data: Vec<u8>) {
    let mut b = us.borrow_mut();
    b.readbuf = data;
    b.readpos = 0;
}

/// _php_stream_write_buffer() on a user stream (bypassing its filters).
pub(crate) fn write_unfiltered(vm: &mut Vm, fid: i64, data: &[u8]) -> Result<i64, VmError> {
    match get_us(fid) {
        Some(us) => write_buffer(vm, &us, data),
        None => Ok(-1),
    }
}

/// call_method() for other modules: Ok(None) when the method does not
/// exist or the call produced no value (exception thrown).
pub(crate) fn call_method_pub(
    vm: &mut Vm,
    obj: &Value,
    name: &[u8],
    args: Vec<Value>,
) -> Result<Option<Value>, VmError> {
    Ok(match call_method(vm, obj, name, args)? {
        CallRes::Val(v) => Some(v),
        _ => None,
    })
}

/// zend_call_method_if_exists(): returns NotImpl when the object cannot
/// handle `name`; Err only for fatal (non-exception) errors.
fn call_method(
    vm: &mut Vm,
    obj: &Value,
    name: &[u8],
    args: Vec<Value>,
) -> Result<CallRes, VmError> {
    if !has_method(vm, obj, name) {
        return Ok(CallRes::NotImpl);
    }
    // zend_call_function() refuses to run with a pending exception.
    if vm.current_exception.is_some() {
        return Ok(CallRes::Undef);
    }
    let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
    match invoke_method(vm, obj, &lower, args) {
        Ok(v) => {
            if vm.current_exception.is_some() {
                Ok(CallRes::Undef)
            } else {
                Ok(CallRes::Val(v))
            }
        }
        Err(e) => {
            if vm.current_exception.is_some() {
                Ok(CallRes::Undef)
            } else {
                Err(e)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Wrapper registry / lookup (php_stream_locate_url_wrapper)
// ---------------------------------------------------------------------------

const BUILTIN_WRAPPERS: &[&[u8]] = &[
    b"php",
    b"file",
    b"glob",
    b"data",
    b"http",
    b"ftp",
    b"phar",
    b"compress.zlib",
    b"compress.bzip2",
    b"https",
    b"ftps",
    b"zip",
];

fn is_builtin(scheme_lower: &[u8]) -> bool {
    BUILTIN_WRAPPERS.contains(&scheme_lower)
}

fn builtin_disabled(scheme_lower: &[u8]) -> bool {
    DISABLED_BUILTINS.with(|d| d.borrow().iter().any(|x| x == scheme_lower))
}

#[derive(Clone)]
pub(crate) struct UserWrapperRef {
    pub scheme: Vec<u8>,
    pub class_name: Vec<u8>,
    pub is_url: bool,
    /// A builtin wrapper implemented in Rust (compress.zlib://).
    pub native: bool,
}

/// Result of locating the wrapper for a path.
#[derive(Clone)]
enum Loc {
    /// A user wrapper.
    User(UserWrapperRef),
    /// A builtin wrapper (or the plain files wrapper) handled by the original
    /// implementation. `path` is the (possibly rewritten) path to use.
    Builtin { path: Vec<u8> },
    /// No wrapper (NULL) — errors already reported.
    None,
}

fn user_wrapper(scheme_lower: &[u8]) -> Option<UserWrapperRef> {
    let user = USER_WRAPPERS.with(|w| {
        w.borrow().get(scheme_lower).map(|u| UserWrapperRef {
            scheme: scheme_lower.to_vec(),
            class_name: u.class_name.clone(),
            is_url: u.flags & 1 != 0,
            native: false,
        })
    });
    if user.is_some() {
        return user;
    }
    // php_stream_gzip_wrapper is served by this layer too.
    if scheme_lower == b"compress.zlib" && !builtin_disabled(scheme_lower) {
        return Some(UserWrapperRef {
            scheme: scheme_lower.to_vec(),
            class_name: Vec::new(),
            is_url: false,
            native: true,
        });
    }
    None
}

/// Length of the scheme part ("foo" for "foo://bar"), if `path` names a
/// wrapper protocol.
fn scheme_len(path: &[u8]) -> Option<usize> {
    let mut n = 0;
    while n < path.len()
        && (path[n].is_ascii_alphanumeric() || matches!(path[n], b'+' | b'-' | b'.'))
    {
        n += 1;
    }
    if n < path.len()
        && path[n] == b':'
        && n > 1
        && (path[n + 1..].starts_with(b"//")
            || (n == 4 && path[..5].eq_ignore_ascii_case(b"data:")))
    {
        Some(n)
    } else {
        None
    }
}

fn ini_bool(vm: &Vm, key: &[u8], default: bool) -> bool {
    match vm.constants.get(key) {
        Some(v) => {
            let s = v.to_php_string().to_string_lossy().to_ascii_lowercase();
            let t = s.trim();
            t == "1"
                || t == "on"
                || t == "yes"
                || t == "true"
                || t.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
        None => default,
    }
}

thread_local! {
    /// PG(in_user_include)
    static IN_USER_INCLUDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Cheap check: could this path be served by a user wrapper (or need the
/// wrapper-lookup diagnostics)? Avoids any work for ordinary paths.
fn needs_locate(path: &[u8]) -> bool {
    match scheme_len(path) {
        Some(n) => {
            let lower: Vec<u8> = path[..n].iter().map(|b| b.to_ascii_lowercase()).collect();
            !is_builtin(&lower) || builtin_disabled(&lower) || user_wrapper(&lower).is_some()
        }
        None => builtin_disabled(b"file") || user_wrapper(b"file").is_some(),
    }
}

/// php_stream_locate_url_wrapper()
fn locate(vm: &mut Vm, path: &[u8], options: i64) -> Loc {
    let n_opt = scheme_len(path);
    let mut protocol: Option<usize> = n_opt;
    let mut found: Option<Loc> = None;
    if let Some(n) = n_opt {
        let lower: Vec<u8> = path[..n].iter().map(|b| b.to_ascii_lowercase()).collect();
        if let Some(u) = user_wrapper(&lower) {
            found = Some(Loc::User(u));
        } else if is_builtin(&lower) && !builtin_disabled(&lower) {
            found = Some(Loc::Builtin {
                path: path.to_vec(),
            });
        } else {
            let name = &path[..n.min(31)];
            warn(
                vm,
                &format!(
                    "Unable to find the wrapper \"{}\" - did you forget to enable it when you configured PHP?",
                    lossy(name)
                ),
            );
            protocol = None;
        }
    }
    let is_file_proto = match protocol {
        None => true,
        Some(n) => path[..n].eq_ignore_ascii_case(b"file"),
    };
    if is_file_proto {
        // Unknown schemes fall back to plain files; virtual_file_ex()
        // normalizes such paths lexically.
        // (Diagnostics keep naming the path as given, so the normalized
        // form is only used when it names an existing file.)
        let plain_path = if n_opt.is_some() && protocol.is_none() {
            let np = normalize_path_lexical(&lossy(path));
            if std::path::Path::new(&np).exists() {
                np.into_bytes()
            } else {
                path.to_vec()
            }
        } else {
            path.to_vec()
        };
        let mut path_for_open = path.to_vec();
        if let Some(n) = protocol {
            let localhost =
                path.len() >= 17 && path[..17].eq_ignore_ascii_case(b"file://localhost/");
            let after = n + 3;
            if !localhost && after < path.len() && path[after] != b'/' {
                if options & REPORT_ERRORS != 0 {
                    warn(
                        vm,
                        &format!("Remote host file access not supported, {}", lossy(path)),
                    );
                }
                return Loc::None;
            }
            let _ = &mut path_for_open;
        }
        let modified = !DISABLED_BUILTINS.with(|d| d.borrow().is_empty())
            || USER_WRAPPERS.with(|w| !w.borrow().is_empty());
        if modified {
            if let Some(f) = found {
                return f;
            }
            if let Some(u) = user_wrapper(b"file") {
                return Loc::User(u);
            }
            if !builtin_disabled(b"file") {
                return Loc::Builtin { path: plain_path };
            }
            if options & REPORT_ERRORS != 0 {
                warn(
                    vm,
                    "file:// wrapper is disabled in the server configuration",
                );
            }
            return Loc::None;
        }
        return Loc::Builtin { path: plain_path };
    }
    if let Some(Loc::User(u)) = &found {
        let n = protocol.unwrap_or(0);
        let allow_fopen = ini_bool(vm, b"allow_url_fopen", true);
        let allow_include = ini_bool(vm, b"allow_url_include", false);
        let in_include =
            options & STREAM_OPEN_FOR_INCLUDE != 0 || IN_USER_INCLUDE.with(|c| c.get());
        if u.is_url && (!allow_fopen || (in_include && !allow_include)) {
            if options & REPORT_ERRORS != 0 {
                let proto = lossy(&path[..n]);
                if !allow_fopen {
                    warn(
                        vm,
                        &format!(
                            "{}:// wrapper is disabled in the server configuration by allow_url_fopen=0",
                            proto
                        ),
                    );
                } else {
                    warn(
                        vm,
                        &format!(
                            "{}:// wrapper is disabled in the server configuration by allow_url_include=0",
                            proto
                        ),
                    );
                }
            }
            return Loc::None;
        }
    }
    found.unwrap_or(Loc::None)
}

/// php_stream_wrapper_log_error()
fn log_error(vm: &mut Vm, options: i64, msg: String) {
    if options & REPORT_ERRORS != 0 {
        warn(vm, &msg);
    } else {
        WRAPPER_ERRORS.with(|e| e.borrow_mut().push(msg));
    }
}

/// php_stream_display_wrapper_errors()
fn display_wrapper_errors(vm: &mut Vm, wrapper_found: bool, path: &[u8], caption: &str) {
    let errs = WRAPPER_ERRORS.with(|e| std::mem::take(&mut *e.borrow_mut()));
    if vm.current_exception.is_some() {
        return;
    }
    let msg = if !wrapper_found {
        "no suitable wrapper could be found".to_string()
    } else if errs.is_empty() {
        "operation failed".to_string()
    } else {
        errs.join("\n")
    };
    let f = cur_fn();
    vm.emit_warning(&format!("{}({}): {}: {}", f, lossy(path), caption, msg));
}

fn tidy_wrapper_errors() {
    WRAPPER_ERRORS.with(|e| e.borrow_mut().clear());
}

// ---------------------------------------------------------------------------
// Contexts
// ---------------------------------------------------------------------------

/// php_stream_context_from_zval(zcontext, no_default)
fn context_from_arg(
    vm: &mut Vm,
    v: Option<&Value>,
    no_default: bool,
) -> Result<Option<i64>, VmError> {
    match v.map(|v| v.deref()) {
        Some(Value::Resource(id)) => {
            let is_ctx = STREAM_CONTEXTS.with(|c| c.borrow().contains_key(&id))
                && goro_core::value::resource_type_of(id).as_deref()
                    == Some(b"stream-context".as_slice());
            if !is_ctx {
                let msg = format!(
                    "{}(): supplied resource is not a valid Stream-Context resource",
                    cur_fn()
                );
                return Err(throw(vm, b"TypeError", &msg));
            }
            Ok(Some(id))
        }
        Some(Value::Null) | Some(Value::Undef) | None => {
            if no_default {
                Ok(None)
            } else {
                Ok(Some(default_context()))
            }
        }
        Some(_) => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Wrapper object creation (user_stream_create_object)
// ---------------------------------------------------------------------------

fn create_object(
    vm: &mut Vm,
    uw: &UserWrapperRef,
    context: Option<i64>,
) -> Result<Option<Value>, VmError> {
    let cl: Vec<u8> = uw
        .class_name
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if !vm.classes.contains_key(&cl) {
        vm.try_autoload_class(&uw.class_name);
    }
    let (canonical, abstract_like, ctor) = match vm.classes.get(&cl) {
        Some(c) => (
            c.name.clone(),
            c.is_abstract || c.is_interface || c.is_trait || c.is_enum,
            c.get_method(b"__construct").is_some(),
        ),
        None => return Ok(None),
    };
    if abstract_like {
        return Ok(None);
    }
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(canonical.clone(), id);
    vm.apply_class_property_defaults(&mut obj, &cl);
    let ctx_val = match context {
        Some(id) => Value::Resource(id),
        None => Value::Null,
    };
    // add_property_resource()/add_property_null() go through the regular
    // property write: visibility, readonly and property types apply.
    let pdef = vm.classes.get(&cl).and_then(|c| {
        c.properties
            .iter()
            .find(|p| p.name == b"context" && !p.is_static)
            .cloned()
    });
    if let Some(pd) = &pdef {
        let scope: Option<Vec<u8>> = vm.current_class_scope().map(|s| s.to_ascii_lowercase());
        let decl = pd.declaring_class.to_ascii_lowercase();
        let decl_name = vm
            .classes
            .get(&decl)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| canonical.clone());
        let decl_disp = goro_core::value::display_class_name(&decl_name).to_string();
        let accessible = match pd.visibility {
            Visibility::Public => true,
            Visibility::Private => scope.as_deref() == Some(decl.as_slice()),
            Visibility::Protected => match &scope {
                Some(sc) => {
                    *sc == decl || vm.class_extends(sc, &decl) || vm.class_extends(&decl, sc)
                }
                None => false,
            },
        };
        if !accessible {
            let vis = if matches!(pd.visibility, Visibility::Private) {
                "private"
            } else {
                "protected"
            };
            let msg = format!("Cannot access {} property {}::$context", vis, decl_disp);
            return Err(throw(vm, b"Error", &msg));
        }
        if pd.is_readonly && scope.as_deref() != Some(decl.as_slice()) {
            let from = match &scope {
                Some(sc) => format!(
                    "scope {}",
                    vm.classes
                        .get(sc)
                        .map(|c| goro_core::value::display_class_name(&c.name).to_string())
                        .unwrap_or_default()
                ),
                None => "global scope".to_string(),
            };
            let msg = format!(
                "Cannot initialize readonly property {}::$context from {}",
                decl_disp, from
            );
            return Err(throw(vm, b"Error", &msg));
        }
    }
    let prop_type = pdef.and_then(|p| p.property_type);
    if let Some(pt) = prop_type {
        if !vm.value_matches_type(&ctx_val, &pt) {
            let msg = format!(
                "Cannot assign {} to property {}::$context of type {}",
                Vm::value_type_name(&ctx_val),
                goro_core::value::display_class_name(&canonical),
                vm.param_type_name(&pt)
            );
            return Err(throw(vm, b"TypeError", &msg));
        }
    }
    obj.set_property(b"context".to_vec(), ctx_val);
    let val = Value::Object(Rc::new(RefCell::new(obj)));
    if ctor {
        match invoke_method(vm, &val, b"__construct", vec![]) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
        if vm.current_exception.is_some() {
            return Err(exc_err(vm));
        }
    }
    if vm
        .classes
        .get(&cl)
        .map(|c| c.get_method(b"__destruct").is_some())
        .unwrap_or(false)
    {
        vm.track_destructible(val.clone());
    }
    Ok(Some(val))
}

// ---------------------------------------------------------------------------
// Stat buffers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub(crate) struct StatBuf {
    pub dev: i64,
    pub ino: i64,
    pub mode: i64,
    pub nlink: i64,
    pub uid: i64,
    pub gid: i64,
    pub rdev: i64,
    pub size: i64,
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
    pub blksize: i64,
    pub blocks: i64,
}

/// statbuf_from_array()
fn statbuf_from_array(arr: &PhpArray) -> StatBuf {
    let g = |k: &[u8]| -> i64 {
        arr.get(&ArrayKey::String(PhpString::from_bytes(k)))
            .map(|v| v.deref().to_long())
            .unwrap_or(0)
    };
    StatBuf {
        dev: g(b"dev"),
        ino: g(b"ino"),
        mode: g(b"mode"),
        nlink: g(b"nlink"),
        uid: g(b"uid"),
        gid: g(b"gid"),
        rdev: g(b"rdev"),
        size: g(b"size"),
        atime: g(b"atime"),
        mtime: g(b"mtime"),
        ctime: g(b"ctime"),
        blksize: g(b"blksize"),
        blocks: g(b"blocks"),
    }
}

fn stat_to_array(sb: &StatBuf) -> Value {
    let fields: [(&[u8], i64); 13] = [
        (b"dev", sb.dev),
        (b"ino", sb.ino),
        (b"mode", sb.mode),
        (b"nlink", sb.nlink),
        (b"uid", sb.uid),
        (b"gid", sb.gid),
        (b"rdev", sb.rdev),
        (b"size", sb.size),
        (b"atime", sb.atime),
        (b"mtime", sb.mtime),
        (b"ctime", sb.ctime),
        (b"blksize", sb.blksize),
        (b"blocks", sb.blocks),
    ];
    let mut a = new_arr();
    for (i, (_, v)) in fields.iter().enumerate() {
        a.set(ArrayKey::Int(i as i64), Value::Long(*v));
    }
    for (k, v) in fields.iter() {
        a.set(ArrayKey::String(PhpString::from_bytes(k)), Value::Long(*v));
    }
    arr_val(a)
}

fn clear_stat_cache() {
    STAT_CACHE.with(|c| *c.borrow_mut() = None);
    LSTAT_CACHE.with(|c| *c.borrow_mut() = None);
}

// ---------------------------------------------------------------------------
// php_stream_userspace_ops
// ---------------------------------------------------------------------------

fn us_obj(us: &Us) -> (Value, String) {
    let b = us.borrow();
    (b.obj.clone(), lossy(&b.class_name))
}

/// Run `f` with PHP_STREAM_FLAG_NO_FCLOSE set, restoring the previous value.
fn with_no_fclose<T>(us: &Us, f: impl FnOnce() -> T) -> T {
    let orig = us.borrow().flags & FLAG_NO_FCLOSE;
    us.borrow_mut().flags |= FLAG_NO_FCLOSE;
    let r = f();
    let mut b = us.borrow_mut();
    b.flags &= !FLAG_NO_FCLOSE;
    b.flags |= orig;
    r
}

/// php_userstreamop_write()
fn op_write(vm: &mut Vm, us: &Us, buf: &[u8]) -> Result<i64, VmError> {
    if let Some(n) = native_of(us) {
        return Ok(n.borrow_mut().write(vm, buf));
    }
    let (obj, cname) = us_obj(us);
    let count = buf.len() as i64;
    let r = with_no_fclose(us, || call_method(vm, &obj, b"stream_write", vec![s(buf)]))?;
    let ret = match r {
        CallRes::NotImpl => {
            warn(vm, &format!("{}::stream_write is not implemented!", cname));
            return Ok(-1);
        }
        CallRes::Undef => return Ok(-1),
        CallRes::Val(v) => v,
    };
    let mut didwrite = if matches!(ret.deref(), Value::False) {
        -1
    } else {
        ret.deref().to_long()
    };
    if didwrite > 0 && didwrite > count {
        warn(
            vm,
            &format!(
                "{}::stream_write wrote {} bytes more data than requested ({} written, {} max)",
                cname,
                didwrite - count,
                didwrite,
                count
            ),
        );
        didwrite = count;
    }
    Ok(didwrite)
}

/// php_userstreamop_read(): returns the bytes read, or None on error (-1).
fn op_read(vm: &mut Vm, us: &Us, count: usize) -> Result<Option<Vec<u8>>, VmError> {
    if let Some(n) = native_of(us) {
        let mut eof = us.borrow().eof;
        let r = n.borrow_mut().read(vm, count, &mut eof);
        us.borrow_mut().eof = eof;
        return Ok(r);
    }
    let (obj, cname) = us_obj(us);
    with_no_fclose(us, || -> Result<Option<Vec<u8>>, VmError> {
        let ret = match call_method(vm, &obj, b"stream_read", vec![Value::Long(count as i64)])? {
            CallRes::Undef => return Ok(None),
            CallRes::NotImpl => {
                warn(vm, &format!("{}::stream_read is not implemented!", cname));
                return Ok(None);
            }
            CallRes::Val(v) => v.deref(),
        };
        if matches!(ret, Value::False) {
            return Ok(None);
        }
        // try_convert_to_string()
        let mut data: Vec<u8> = match &ret {
            Value::Array(_) => {
                warn_plain(vm, "Array to string conversion");
                b"Array".to_vec()
            }
            Value::Object(_) => match object_to_string(vm, &ret)? {
                Some(sv) => sv,
                None => return Ok(None),
            },
            v => v.to_php_string().as_bytes().to_vec(),
        };
        if data.len() > count {
            warn(
                vm,
                &format!(
                    "{}::stream_read - read {} bytes more data than requested ({} read, {} max) - excess data will be lost",
                    cname,
                    data.len() - count,
                    data.len(),
                    count
                ),
            );
            data.truncate(count);
        }
        match call_method(vm, &obj, b"stream_eof", vec![])? {
            CallRes::NotImpl => {
                warn(
                    vm,
                    &format!("{}::stream_eof is not implemented! Assuming EOF", cname),
                );
                us.borrow_mut().eof = true;
                return Ok(None);
            }
            CallRes::Undef => {
                us.borrow_mut().eof = true;
                return Ok(None);
            }
            CallRes::Val(v) => {
                if v.deref().is_truthy() {
                    us.borrow_mut().eof = true;
                }
            }
        }
        Ok(Some(data))
    })
}

fn warn_plain(vm: &mut Vm, msg: &str) {
    vm.emit_warning(msg);
}

fn has_method_exact(vm: &Vm, obj: &Value, name: &[u8]) -> bool {
    let cl = class_lower_of(obj);
    vm.classes
        .get(&cl)
        .map(|c| c.get_method(name).is_some())
        .unwrap_or(false)
}

/// try_convert_to_string() on an object: __toString, or an Error.
fn object_to_string(vm: &mut Vm, v: &Value) -> Result<Option<Vec<u8>>, VmError> {
    if let Value::Object(o) = v {
        if has_method_exact(vm, v, b"__tostring") {
            return match invoke_method(vm, v, b"__tostring", vec![]) {
                Ok(sv) => {
                    if vm.current_exception.is_some() {
                        Ok(None)
                    } else {
                        Ok(Some(sv.to_php_string().as_bytes().to_vec()))
                    }
                }
                Err(e) => {
                    if vm.current_exception.is_some() {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                }
            };
        }
        let cname = goro_core::value::display_class_name(&o.borrow().class_name);
        let msg = format!("Object of class {} could not be converted to string", cname);
        let exc = vm.create_exception(b"Error", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Ok(None);
    }
    Ok(Some(v.to_php_string().as_bytes().to_vec()))
}

/// php_userstreamop_close()
fn op_close(vm: &mut Vm, us: &Us) -> Result<(), VmError> {
    if let Some(n) = native_of(us) {
        n.borrow_mut().close(vm);
        return Ok(());
    }
    let (obj, _) = us_obj(us);
    let _ = call_method(vm, &obj, b"stream_close", vec![])?;
    Ok(())
}

/// php_userstreamop_flush(): 0 on success, -1 on failure.
fn op_flush(vm: &mut Vm, us: &Us) -> Result<i32, VmError> {
    if let Some(n) = native_of(us) {
        return Ok(n.borrow_mut().flush(vm));
    }
    let (obj, _) = us_obj(us);
    Ok(match call_method(vm, &obj, b"stream_flush", vec![])? {
        CallRes::Val(v) if v.deref().is_truthy() => 0,
        _ => -1,
    })
}

/// php_userstreamop_seek(): returns (ret, new offset)
fn op_seek(vm: &mut Vm, us: &Us, offset: i64, whence: i64) -> Result<(i32, Option<i64>), VmError> {
    if let Some(n) = native_of(us) {
        return Ok(match n.borrow_mut().seek(vm, offset, whence) {
            Some(p) => (0, Some(p)),
            None => (-1, None),
        });
    }
    let (obj, cname) = us_obj(us);
    with_no_fclose(us, || -> Result<(i32, Option<i64>), VmError> {
        let r = call_method(
            vm,
            &obj,
            b"stream_seek",
            vec![Value::Long(offset), Value::Long(whence)],
        )?;
        let ret = match r {
            CallRes::NotImpl => {
                us.borrow_mut().flags |= FLAG_NO_SEEK;
                return Ok((-1, None));
            }
            CallRes::Val(v) if v.deref().is_truthy() => 0,
            _ => -1,
        };
        if ret != 0 {
            return Ok((ret, None));
        }
        match call_method(vm, &obj, b"stream_tell", vec![])? {
            CallRes::Val(v) if matches!(v.deref(), Value::Long(_)) => {
                Ok((0, Some(v.deref().to_long())))
            }
            CallRes::NotImpl => {
                warn(vm, &format!("{}::stream_tell is not implemented!", cname));
                Ok((-1, None))
            }
            _ => Ok((-1, None)),
        }
    })
}

/// php_userstreamop_stat()
fn op_stat(vm: &mut Vm, us: &Us) -> Result<Option<StatBuf>, VmError> {
    if native_of(us).is_some() {
        return Ok(None);
    }
    let (obj, cname) = us_obj(us);
    match call_method(vm, &obj, b"stream_stat", vec![])? {
        CallRes::NotImpl => {
            warn(vm, &format!("{}::stream_stat is not implemented!", cname));
            Ok(None)
        }
        CallRes::Undef => Ok(None),
        CallRes::Val(v) => match v.deref() {
            Value::Array(a) => Ok(Some(statbuf_from_array(&a.borrow()))),
            _ => Ok(None),
        },
    }
}

/// zend_zval_value_name()
fn zval_type_name(v: &Value) -> String {
    match v.deref() {
        Value::Object(o) => {
            goro_core::value::display_class_name(&o.borrow().class_name).to_string()
        }
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        other => Vm::value_type_name(&other),
    }
}

/// user_stream_set_check_liveliness()
fn op_check_liveness(vm: &mut Vm, us: &Us) -> Result<i32, VmError> {
    if native_of(us).is_some() {
        return Ok(OPT_NOTIMPL);
    }
    let (obj, cname) = us_obj(us);
    match call_method(vm, &obj, b"stream_eof", vec![])? {
        CallRes::NotImpl => {
            warn(
                vm,
                &format!("{}::stream_eof is not implemented! Assuming EOF", cname),
            );
            Ok(OPT_ERR)
        }
        CallRes::Undef => Ok(OPT_ERR),
        CallRes::Val(v) => match v.deref() {
            Value::True => Ok(OPT_ERR),
            Value::False => Ok(OPT_OK),
            other => {
                warn(
                    vm,
                    &format!(
                        "{}::stream_eof value must be of type bool, {} given",
                        cname,
                        zval_type_name(&other)
                    ),
                );
                Ok(OPT_ERR)
            }
        },
    }
}

/// user_stream_set_locking()
fn op_lock(vm: &mut Vm, us: &Us, value: i64) -> Result<i32, VmError> {
    if let Some(n) = native_of(us) {
        return Ok(n.borrow_mut().lock(vm, value));
    }
    let (obj, cname) = us_obj(us);
    // Userland sees PHP_LOCK_SH/EX/UN (1/2/3) | PHP_LOCK_NB (4).
    let mut lock = 0;
    if value & 4 != 0 {
        lock |= 4;
    }
    // value holds system LOCK_SH(1)/LOCK_EX(2)/LOCK_UN(8)
    match value & !4 {
        1 => lock |= 1,
        2 => lock |= 2,
        8 => lock |= 3,
        _ => {}
    }
    match call_method(vm, &obj, b"stream_lock", vec![Value::Long(lock)])? {
        CallRes::NotImpl => {
            if value == 0 {
                return Ok(OPT_OK);
            }
            warn(vm, &format!("{}::stream_lock is not implemented!", cname));
            Ok(OPT_ERR)
        }
        CallRes::Undef => Ok(OPT_ERR),
        CallRes::Val(v) => match v.deref() {
            Value::False => Ok(1),
            Value::True => Ok(0),
            _ => Ok(OPT_NOTIMPL),
        },
    }
}

/// user_stream_set_truncation(): `size` None = PHP_STREAM_TRUNCATE_SUPPORTED
fn op_truncate(vm: &mut Vm, us: &Us, size: Option<i64>) -> Result<i32, VmError> {
    if native_of(us).is_some() {
        return Ok(OPT_NOTIMPL);
    }
    let (obj, cname) = us_obj(us);
    let size = match size {
        None => {
            return Ok(if has_method(vm, &obj, b"stream_truncate") {
                OPT_OK
            } else {
                OPT_ERR
            });
        }
        Some(n) => n,
    };
    if size < 0 {
        return Ok(OPT_ERR);
    }
    match call_method(vm, &obj, b"stream_truncate", vec![Value::Long(size)])? {
        CallRes::NotImpl => {
            warn(
                vm,
                &format!("{}::stream_truncate is not implemented!", cname),
            );
            Ok(OPT_ERR)
        }
        CallRes::Undef => Ok(OPT_ERR),
        CallRes::Val(v) => match v.deref() {
            Value::True => Ok(OPT_OK),
            Value::False => Ok(OPT_ERR),
            other => {
                warn(
                    vm,
                    &format!(
                        "{}::stream_truncate value must be of type bool, {} given",
                        cname,
                        zval_type_name(&other)
                    ),
                );
                Ok(OPT_ERR)
            }
        },
    }
}

/// user_stream_set_option()
fn op_set_option(
    vm: &mut Vm,
    us: &Us,
    option: i64,
    value: i64,
    ptr: Value,
) -> Result<i32, VmError> {
    if native_of(us).is_some() {
        return Ok(OPT_NOTIMPL);
    }
    let (obj, cname) = us_obj(us);
    let args = vec![Value::Long(option), Value::Long(value), ptr];
    match call_method(vm, &obj, b"stream_set_option", args)? {
        CallRes::NotImpl => {
            warn(
                vm,
                &format!("{}::stream_set_option is not implemented!", cname),
            );
            Ok(OPT_ERR)
        }
        CallRes::Undef => Ok(OPT_ERR),
        CallRes::Val(v) => Ok(if v.deref().is_truthy() {
            OPT_OK
        } else {
            OPT_ERR
        }),
    }
}

/// _php_stream_set_option() for user streams.
fn stream_set_option(
    vm: &mut Vm,
    us: &Us,
    option: i64,
    value: i64,
    ptr: Option<i64>,
) -> Result<i32, VmError> {
    let ret = match option {
        PHP_STREAM_OPTION_READ_TIMEOUT => {
            op_set_option(vm, us, option, value, Value::Long(ptr.unwrap_or(0)))?
        }
        PHP_STREAM_OPTION_READ_BUFFER | PHP_STREAM_OPTION_WRITE_BUFFER => {
            op_set_option(vm, us, option, value, Value::Long(ptr.unwrap_or(BUFSIZ)))?
        }
        PHP_STREAM_OPTION_BLOCKING => op_set_option(vm, us, option, value, Value::Null)?,
        _ => OPT_NOTIMPL,
    };
    if ret == OPT_NOTIMPL && option == PHP_STREAM_OPTION_READ_BUFFER {
        let mut b = us.borrow_mut();
        if value == 0 {
            b.flags |= FLAG_NO_BUFFER;
        } else {
            b.flags &= !FLAG_NO_BUFFER;
        }
        return Ok(OPT_OK);
    }
    Ok(ret)
}

/// php_userstreamop_cast(): returns the inner stream resource id on success.
fn op_cast(
    vm: &mut Vm,
    us: &Us,
    fid: i64,
    for_select: bool,
    report_errors: bool,
) -> Result<Option<i64>, VmError> {
    if let Some(n) = native_of(us) {
        if report_errors {
            let label = n.borrow().label();
            warn(
                vm,
                &format!(
                    "Cannot represent a stream of type {} as a {}",
                    label,
                    if for_select {
                        "select()able descriptor"
                    } else {
                        "STDIO FILE*"
                    }
                ),
            );
        }
        return Ok(None);
    }
    let (obj, cname) = us_obj(us);
    let arg = Value::Long(if for_select { 3 } else { 0 });
    let r = with_no_fclose(us, || call_method(vm, &obj, b"stream_cast", vec![arg]))?;
    let ret = match r {
        CallRes::NotImpl => {
            if report_errors {
                warn(vm, &format!("{}::stream_cast is not implemented!", cname));
            }
            return Ok(None);
        }
        CallRes::Undef => return Ok(None),
        CallRes::Val(v) => v.deref(),
    };
    if !ret.is_truthy() {
        return Ok(None);
    }
    let inner = match &ret {
        Value::Resource(n)
            if goro_core::value::is_open_resource(*n)
                && (FILE_HANDLES.with(|h| h.borrow().contains_key(n)) || (1..=3).contains(n)) =>
        {
            *n
        }
        _ => {
            if report_errors {
                warn(
                    vm,
                    &format!("{}::stream_cast must return a stream resource", cname),
                );
            }
            return Ok(None);
        }
    };
    if inner == fid {
        if report_errors {
            warn(
                vm,
                &format!("{}::stream_cast must not return itself", cname),
            );
        }
        return Ok(None);
    }
    if let Some(ius) = get_us(inner) {
        // php_stream_cast(intstream, castas, retptr, show_err=1)
        let r = op_cast(vm, &ius, inner, for_select, report_errors)?;
        if r.is_none() && report_errors {
            warn(
                vm,
                &format!(
                    "Cannot represent a stream of type user-space as a {}",
                    if for_select {
                        "select()able descriptor"
                    } else {
                        "STDIO FILE*"
                    }
                ),
            );
        }
        return Ok(r);
    }
    Ok(Some(inner))
}

// ---------------------------------------------------------------------------
// Stream layer (streams.c) for user streams
// ---------------------------------------------------------------------------

fn has_read_filters(fid: i64) -> bool {
    crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_READ)
}

fn has_write_filters(fid: i64) -> bool {
    crate::stream_filters::chain_has_direction(fid, crate::stream_filters::STREAM_FILTER_WRITE)
}

fn compact(b: &mut UserStream) {
    if b.readpos > 0 {
        let rp = b.readpos;
        b.readbuf.drain(..rp);
        b.readpos = 0;
    }
}

/// _php_stream_fill_read_buffer(): Ok(false) == FAILURE
fn fill_read_buffer(vm: &mut Vm, fid: i64, us: &Us, size: usize) -> Result<bool, VmError> {
    if has_read_filters(fid) {
        let chunk = us.borrow().chunk_size;
        let to_read_now = size.min(chunk);
        loop {
            {
                let b = us.borrow();
                if b.eof || b.buffered() >= to_read_now {
                    break;
                }
            }
            let r = op_read(vm, us, chunk)?;
            let empty_buf = us.borrow().buffered() == 0;
            let data = match r {
                None if empty_buf => return Ok(false),
                None => Vec::new(),
                Some(d) => d,
            };
            let justread = data.len();
            let eof = us.borrow().eof;
            let flags = if justread > 0 {
                if eof {
                    crate::stream_filters::PSFS_FLAG_FLUSH_CLOSE
                } else {
                    crate::stream_filters::PSFS_FLAG_NORMAL
                }
            } else if eof {
                crate::stream_filters::PSFS_FLAG_FLUSH_CLOSE
            } else {
                crate::stream_filters::PSFS_FLAG_FLUSH_INC
            };
            let r = crate::stream_filters::run_read_chain(vm, fid, Some(&data), flags);
            match r.status {
                crate::stream_filters::PSFS_PASS_ON => {
                    let mut b = us.borrow_mut();
                    compact(&mut b);
                    b.readbuf.extend_from_slice(&r.out);
                }
                crate::stream_filters::PSFS_FEED_ME => {}
                _ => {
                    let mut b = us.borrow_mut();
                    b.eof = true;
                    b.fatal_error = true;
                    return Ok(false);
                }
            }
            if justread == 0 {
                break;
            }
        }
        return Ok(true);
    }
    let (buffered, chunk) = {
        let b = us.borrow();
        (b.buffered(), b.chunk_size)
    };
    if buffered < size {
        compact(&mut us.borrow_mut());
        match op_read(vm, us, chunk)? {
            None => return Ok(false),
            Some(d) => us.borrow_mut().readbuf.extend_from_slice(&d),
        }
    }
    Ok(true)
}

/// _php_stream_read(): None == -1
// The loop mirrors the C one, which also exits after a single pass for
// non-greedy streams.
#[allow(clippy::never_loop)]
fn stream_read(
    vm: &mut Vm,
    fid: i64,
    us: &Us,
    mut size: usize,
) -> Result<Option<Vec<u8>>, VmError> {
    let mut out: Vec<u8> = Vec::new();
    while size > 0 {
        {
            let mut b = us.borrow_mut();
            let avail = b.buffered();
            if avail > 0 {
                let n = avail.min(size);
                let rp = b.readpos;
                out.extend_from_slice(&b.readbuf[rp..rp + n]);
                b.readpos += n;
                size -= n;
            }
        }
        if size == 0 {
            break;
        }
        let (no_buffer, chunk) = {
            let b = us.borrow();
            (b.flags & FLAG_NO_BUFFER != 0, b.chunk_size)
        };
        let toread;
        if !has_read_filters(fid) && (no_buffer || chunk == 1) {
            match op_read(vm, us, size)? {
                None => {
                    if out.is_empty() {
                        return Ok(None);
                    }
                    break;
                }
                Some(d) => {
                    toread = d.len();
                    out.extend_from_slice(&d);
                }
            }
        } else {
            if !fill_read_buffer(vm, fid, us, size)? {
                if out.is_empty() {
                    return Ok(None);
                }
                break;
            }
            let mut b = us.borrow_mut();
            let n = b.buffered().min(size);
            let rp = b.readpos;
            out.extend_from_slice(&b.readbuf[rp..rp + n]);
            b.readpos += n;
            toread = n;
        }
        if toread == 0 {
            break;
        }
        // Not plain files / memory: never read greedily.
        break;
    }
    if !out.is_empty() {
        us.borrow_mut().position += out.len() as i64;
    }
    Ok(Some(out))
}

/// _php_stream_eof()
fn stream_eof(vm: &mut Vm, us: &Us) -> Result<bool, VmError> {
    if us.borrow().buffered() > 0 {
        return Ok(false);
    }
    if !us.borrow().eof && op_check_liveness(vm, us)? == OPT_ERR {
        us.borrow_mut().eof = true;
    }
    Ok(us.borrow().eof)
}

/// _php_stream_get_line(): maxlen None == grow mode
fn stream_get_line(
    vm: &mut Vm,
    fid: i64,
    us: &Us,
    maxlen: Option<usize>,
) -> Result<Option<Vec<u8>>, VmError> {
    let mut out: Vec<u8> = Vec::new();
    let mut maxlen_left = maxlen.unwrap_or(0);
    if maxlen == Some(0) {
        return Ok(None);
    }
    loop {
        let avail = us.borrow().buffered();
        if avail > 0 {
            let mut b = us.borrow_mut();
            let rp = b.readpos;
            let (cpysz, done) = {
                let slice = &b.readbuf[rp..];
                let mut done = false;
                let mut cpysz = match slice.iter().position(|&c| c == b'\n') {
                    Some(p) => {
                        done = true;
                        p + 1
                    }
                    None => avail,
                };
                if maxlen.is_some() && cpysz >= maxlen_left - 1 {
                    cpysz = maxlen_left - 1;
                    done = true;
                }
                out.extend_from_slice(&slice[..cpysz]);
                (cpysz, done)
            };
            b.position += cpysz as i64;
            b.readpos += cpysz;
            maxlen_left = maxlen_left.saturating_sub(cpysz);
            if done {
                break;
            }
        } else if us.borrow().eof {
            break;
        } else {
            let chunk = us.borrow().chunk_size;
            let toread = match maxlen {
                None => chunk,
                Some(_) => (maxlen_left - 1).min(chunk),
            };
            if !fill_read_buffer(vm, fid, us, toread)? && us.borrow().fatal_error {
                return Ok(None);
            }
            if us.borrow().buffered() == 0 {
                break;
            }
        }
    }
    if out.is_empty() {
        return Ok(None);
    }
    Ok(Some(out))
}

fn search_delim(us: &UserStream, maxlen: usize, skiplen: usize, delim: &[u8]) -> Option<usize> {
    let seek_len = us.buffered().min(maxlen);
    if seek_len <= skiplen {
        return None;
    }
    let hay = &us.readbuf[us.readpos + skiplen..us.readpos + seek_len];
    if delim.len() == 1 {
        hay.iter().position(|&c| c == delim[0]).map(|p| p + skiplen)
    } else {
        hay.windows(delim.len())
            .position(|w| w == delim)
            .map(|p| p + skiplen)
    }
}

/// php_stream_get_record()
fn stream_get_record(
    vm: &mut Vm,
    fid: i64,
    us: &Us,
    maxlen: usize,
    delim: &[u8],
) -> Result<Option<Vec<u8>>, VmError> {
    if maxlen == 0 {
        return Ok(None);
    }
    let has_delim = !delim.is_empty();
    let mut found = if has_delim {
        search_delim(&us.borrow(), maxlen, 0, delim)
    } else {
        None
    };
    let mut buffered_len = us.borrow().buffered();
    while found.is_none() && buffered_len < maxlen {
        let chunk = us.borrow().chunk_size;
        let to_read_now = (maxlen - buffered_len).min(chunk);
        if !fill_read_buffer(vm, fid, us, buffered_len + to_read_now)? && us.borrow().fatal_error {
            return Ok(None);
        }
        let just_read = us.borrow().buffered().saturating_sub(buffered_len);
        if just_read == 0 {
            break;
        }
        if has_delim {
            let skip = if buffered_len >= delim.len() - 1 {
                buffered_len - (delim.len() - 1)
            } else {
                0
            };
            found = search_delim(&us.borrow(), maxlen, skip, delim);
            if found.is_some() {
                break;
            }
        }
        buffered_len += just_read;
    }
    let tent_ret_len;
    {
        let b = us.borrow();
        if let Some(p) = found {
            tent_ret_len = p;
        } else if !has_delim && b.buffered() >= maxlen {
            tent_ret_len = maxlen;
        } else if (b.buffered() < maxlen && !b.eof) || (b.buffered() == 0 && b.eof) {
            return Ok(None);
        } else {
            tent_ret_len = b.buffered().min(maxlen);
        }
    }
    let ret = if tent_ret_len > 0 {
        stream_read(vm, fid, us, tent_ret_len)?.unwrap_or_default()
    } else {
        Vec::new()
    };
    if found.is_some() {
        let mut b = us.borrow_mut();
        b.readpos += delim.len();
        b.position += delim.len() as i64;
    }
    Ok(Some(ret))
}

/// _php_stream_write_buffer()
fn write_buffer(vm: &mut Vm, us: &Us, buf: &[u8]) -> Result<i64, VmError> {
    let (no_seek, need_seek, position) = {
        let b = us.borrow();
        (b.flags & FLAG_NO_SEEK != 0, b.buffered() > 0, b.position)
    };
    if !no_seek && need_seek {
        {
            let mut b = us.borrow_mut();
            b.readbuf.clear();
            b.readpos = 0;
        }
        let (_, newpos) = op_seek(vm, us, position, SEEK_SET)?;
        if let Some(p) = newpos {
            us.borrow_mut().position = p;
        }
    }
    // GH-13071: only userspace streams are written in chunk_size pieces.
    let chunk = if us.borrow().native.is_some() {
        buf.len().max(1)
    } else {
        us.borrow().chunk_size.max(1)
    };
    let mut didwrite: i64 = 0;
    let mut off = 0usize;
    while off < buf.len() {
        let end = (off + chunk).min(buf.len());
        let justwrote = op_write(vm, us, &buf[off..end])?;
        if justwrote <= 0 {
            if didwrite == 0 {
                return Ok(justwrote);
            }
            return Ok(didwrite);
        }
        off += justwrote as usize;
        didwrite += justwrote;
        us.borrow_mut().position += justwrote;
    }
    Ok(didwrite)
}

/// _php_stream_write()
fn stream_write(vm: &mut Vm, fid: i64, us: &Us, buf: &[u8]) -> Result<i64, VmError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let bytes = if has_write_filters(fid) {
        let (consumed, out) = crate::stream_filters::run_write_chain(
            vm,
            fid,
            Some(buf),
            crate::stream_filters::PSFS_FLAG_NORMAL,
        );
        if !out.is_empty() && write_buffer(vm, us, &out)? < 0 {
            -1
        } else {
            consumed as i64
        }
    } else {
        write_buffer(vm, us, buf)?
    };
    if bytes != 0 {
        us.borrow_mut().flags |= FLAG_WAS_WRITTEN;
    }
    Ok(bytes)
}

/// _php_stream_flush()
fn stream_flush(vm: &mut Vm, fid: i64, us: &Us) -> Result<i32, VmError> {
    stream_flush_ex(vm, fid, us, false)
}

fn stream_flush_ex(vm: &mut Vm, fid: i64, us: &Us, closing: bool) -> Result<i32, VmError> {
    if has_write_filters(fid) {
        let flags = if closing {
            crate::stream_filters::PSFS_FLAG_FLUSH_CLOSE
        } else {
            crate::stream_filters::PSFS_FLAG_FLUSH_INC
        };
        let (_, out) = crate::stream_filters::run_write_chain(vm, fid, None, flags);
        if !out.is_empty() {
            write_buffer(vm, us, &out)?;
        }
    }
    us.borrow_mut().flags &= !FLAG_WAS_WRITTEN;
    op_flush(vm, us)
}

/// _php_stream_seek()
fn stream_seek(vm: &mut Vm, fid: i64, us: &Us, offset: i64, whence: i64) -> Result<i32, VmError> {
    {
        let mut b = us.borrow_mut();
        if b.flags & FLAG_NO_BUFFER == 0 {
            let avail = b.buffered() as i64;
            match whence {
                SEEK_CUR if offset > 0 && offset <= avail => {
                    b.readpos += offset as usize;
                    b.position += offset;
                    b.eof = false;
                    b.fatal_error = false;
                    return Ok(0);
                }
                SEEK_SET if offset > b.position && offset <= b.position + avail => {
                    let d = (offset - b.position) as usize;
                    b.readpos += d;
                    b.position = offset;
                    b.eof = false;
                    b.fatal_error = false;
                    return Ok(0);
                }
                _ => {}
            }
        }
    }
    let mut offset = offset;
    let mut whence = whence;
    if us.borrow().flags & FLAG_NO_SEEK == 0 {
        if has_write_filters(fid) {
            stream_flush(vm, fid, us)?;
        }
        match whence {
            SEEK_CUR => {
                let pos = us.borrow().position;
                offset = pos.saturating_add(offset);
                whence = SEEK_SET;
            }
            SEEK_SET => {
                if offset < 0 {
                    return Ok(-1);
                }
            }
            _ => {}
        }
        let (ret, newpos) = op_seek(vm, us, offset, whence)?;
        if let Some(p) = newpos {
            us.borrow_mut().position = p;
        }
        let no_seek = us.borrow().flags & FLAG_NO_SEEK != 0;
        if !no_seek || ret == 0 {
            let mut b = us.borrow_mut();
            if ret == 0 {
                b.eof = false;
                b.fatal_error = false;
            }
            b.readbuf.clear();
            b.readpos = 0;
            return Ok(ret);
        }
        // The stream decided it can't seek after all: emulate below.
        // (offset/whence were rewritten to SEEK_SET above, like PHP.)
    }
    if whence == SEEK_CUR && offset >= 0 {
        let mut left = offset;
        while left > 0 {
            match stream_read(vm, fid, us, (left as usize).min(1024))? {
                Some(d) if !d.is_empty() => left -= d.len() as i64,
                _ => return Ok(-1),
            }
        }
        let mut b = us.borrow_mut();
        b.eof = false;
        b.fatal_error = false;
        return Ok(0);
    }
    warn(vm, "Stream does not support seeking");
    Ok(-1)
}

/// _php_stream_copy_to_mem(): None == NULL
fn copy_to_mem(
    vm: &mut Vm,
    fid: i64,
    us: &Us,
    maxlen: Option<usize>,
) -> Result<Option<Vec<u8>>, VmError> {
    if maxlen == Some(0) {
        return Ok(Some(Vec::new()));
    }
    let mut out = Vec::new();
    if let Some(maxlen) = maxlen {
        if maxlen < 4 * CHUNK_SIZE {
            while out.len() < maxlen && !stream_eof(vm, us)? {
                match stream_read(vm, fid, us, maxlen - out.len())? {
                    Some(d) if !d.is_empty() => out.extend_from_slice(&d),
                    _ => break,
                }
            }
            return Ok(if out.is_empty() { None } else { Some(out) });
        }
    }
    let position = us.borrow().position;
    let mut buflen = match op_stat(vm, us)? {
        Some(sb) if sb.size > 0 => ((sb.size - position).max(0) as usize) + CHUNK_SIZE,
        _ => CHUNK_SIZE,
    };
    if let Some(m) = maxlen {
        if buflen > m {
            buflen = m;
        }
    }
    let min_room = CHUNK_SIZE / 4;
    loop {
        let want = buflen - out.len();
        let d = match stream_read(vm, fid, us, want)? {
            Some(d) if !d.is_empty() => d,
            _ => break,
        };
        out.extend_from_slice(&d);
        if out.len() + min_room >= buflen {
            if maxlen == Some(out.len()) {
                break;
            }
            match maxlen {
                Some(m) if buflen + CHUNK_SIZE > m => buflen = m,
                _ => buflen += CHUNK_SIZE,
            }
        }
    }
    Ok(if out.is_empty() { None } else { Some(out) })
}

/// _php_stream_free() with close semantics.
fn stream_free(vm: &mut Vm, fid: i64, us: &Us) -> Result<(), VmError> {
    if us.borrow().in_free || us.borrow().closed {
        return Ok(());
    }
    us.borrow_mut().in_free = true;
    let is_dir = us.borrow().flags & FLAG_IS_DIR != 0;
    let mut res = Ok(());
    if !is_dir && (us.borrow().flags & FLAG_WAS_WRITTEN != 0 || has_write_filters(fid)) {
        res = stream_flush_ex(vm, fid, us, true).map(|_| ());
    }
    // zend_list_close(): the resource is gone before the close handler runs.
    USER_STREAMS.with(|m| {
        m.borrow_mut().remove(&fid);
    });
    FILE_HANDLES.with(|h| {
        h.borrow_mut().remove(&fid);
    });
    goro_core::value::close_resource(fid);
    if !is_dir {
        crate::stream_filters::remove_stream_filters(vm, fid);
        if res.is_ok() {
            res = op_close(vm, us);
        }
    } else {
        let (obj, _) = us_obj(us);
        if let Err(e) = call_method(vm, &obj, b"dir_closedir", vec![]) {
            res = Err(e);
        }
        CURRENT_DIR_RESOURCE.with(|c| {
            if c.get() == Some(fid) {
                c.set(None);
            }
        });
    }
    {
        let mut b = us.borrow_mut();
        b.closed = true;
        b.in_free = false;
        b.readbuf.clear();
        b.readpos = 0;
        b.obj = Value::Null;
    }
    vm.run_pending_destructors_pub();
    res
}

/// Close all user streams still open (resource list destruction at
/// request shutdown, in reverse order of creation).
pub(crate) fn shutdown_streams(vm: &mut Vm) {
    let mut ids: Vec<i64> = USER_STREAMS.with(|m| m.borrow().keys().copied().collect());
    ids.sort();
    for fid in ids.into_iter().rev() {
        if let Some(us) = get_us(fid) {
            // A fatal error (or exception) raised by a close handler is
            // reported; the remaining resources are still destroyed.
            let already = vm.fatal_emitted;
            vm.fatal_emitted = false;
            match stream_free(vm, fid, &us) {
                Err(e) if vm.current_exception.is_some() || !vm.fatal_emitted => {
                    vm.report_uncaught(&e)
                }
                _ => {}
            }
            vm.fatal_emitted |= already;
            vm.current_exception = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Wrapper ops (user_stream_wops)
// ---------------------------------------------------------------------------

fn alloc_stream(
    obj: Value,
    class_name: Vec<u8>,
    mode: &str,
    path: &[u8],
    is_dir: bool,
    is_url: bool,
) -> (i64, Us) {
    let placeholder = std::fs::File::open("/dev/null").or_else(|_| tempfile::tempfile());
    let file = match placeholder {
        Ok(f) => f,
        Err(_) => tempfile::tempfile().expect("tempfile"),
    };
    let fid = alloc_file_handle_special(file, mode, SpecialStream::None);
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.uri = lossy(path);
            fh.wrapper_type = "user-space".to_string();
            fh.stream_type = if is_dir {
                "dir".to_string()
            } else {
                "user-space".to_string()
            };
            fh.user_wrapper = Some(obj.clone());
            fh.user_wrapper_class = Some(class_name.clone());
        }
    });
    goro_core::value::register_resource(fid, b"stream");
    let us = Rc::new(RefCell::new(UserStream {
        obj,
        class_name,
        readbuf: Vec::new(),
        readpos: 0,
        position: 0,
        eof: false,
        fatal_error: false,
        chunk_size: CHUNK_SIZE,
        flags: if is_dir {
            FLAG_IS_DIR | FLAG_NO_BUFFER
        } else {
            0
        },
        mode: mode.to_string(),
        orig_path: lossy(path),
        in_free: false,
        closed: false,
        is_url,
        native: None,
        wrapper_label: None,
    }));
    USER_STREAMS.with(|m| {
        m.borrow_mut().insert(fid, us.clone());
    });
    (fid, us)
}

/// php_stream_alloc() for a native stream served by this layer.
pub(crate) fn alloc_native_stream(
    native: Rc<RefCell<dyn NativeStream>>,
    mode: &str,
    uri: Option<&[u8]>,
    wrapper_label: Option<&'static str>,
    no_buffer: bool,
) -> (i64, Us) {
    let label = native.borrow().label();
    let (fid, us) = alloc_stream(
        Value::Null,
        Vec::new(),
        mode,
        uri.unwrap_or_default(),
        false,
        false,
    );
    FILE_HANDLES.with(|h| {
        if let Some(fh) = h.borrow_mut().get_mut(&fid) {
            fh.wrapper_type = wrapper_label.unwrap_or_default().to_string();
            fh.stream_type = label.to_string();
            fh.user_wrapper = None;
            fh.user_wrapper_class = None;
        }
    });
    {
        let mut b = us.borrow_mut();
        b.native = Some(native);
        b.wrapper_label = wrapper_label;
        if no_buffer {
            b.flags |= FLAG_NO_BUFFER;
        }
    }
    (fid, us)
}

fn recursion_guard(path: &[u8]) -> bool {
    CURRENT_FILENAME.with(|c| {
        c.borrow()
            .as_deref()
            .map(|p| p.as_bytes() == path)
            .unwrap_or(false)
    })
}

fn set_current_filename(path: Option<&[u8]>) {
    CURRENT_FILENAME.with(|c| *c.borrow_mut() = path.map(lossy));
}

/// user_wrapper_opener(): Ok(None) == NULL stream
fn wrapper_open(
    vm: &mut Vm,
    uw: &UserWrapperRef,
    path: &[u8],
    mode: &str,
    options: i64,
    context: Option<i64>,
) -> Result<Option<(i64, Us, Option<Vec<u8>>)>, VmError> {
    if uw.native {
        return Ok(zlib_stream::gzopen(vm, path, mode, options, context, true)?
            .map(|(fid, us)| (fid, us, None)));
    }
    if recursion_guard(path) {
        log_error(vm, options, "infinite recursion prevented".to_string());
        return Ok(None);
    }
    set_current_filename(Some(path));
    let old_in_include = IN_USER_INCLUDE.with(|c| c.get());
    if !uw.is_url
        && options & STREAM_OPEN_FOR_INCLUDE != 0
        && !ini_bool(vm, b"allow_url_include", false)
    {
        IN_USER_INCLUDE.with(|c| c.set(true));
    }
    let result = (|| -> Result<Option<(i64, Us, Option<Vec<u8>>)>, VmError> {
        let obj = match create_object(vm, uw, context)? {
            Some(o) => o,
            None => return Ok(None),
        };
        let opened = Rc::new(RefCell::new(Value::Null));
        let args = vec![
            s(path),
            s(mode.as_bytes()),
            Value::Long(options),
            Value::Reference(opened.clone()),
        ];
        let cname = match &obj {
            Value::Object(o) => lossy(&o.borrow().class_name),
            _ => String::new(),
        };
        match call_method(vm, &obj, b"stream_open", args)? {
            CallRes::NotImpl => {
                log_error(
                    vm,
                    options,
                    format!("\"{}::stream_open\" is not implemented", cname),
                );
                Ok(None)
            }
            CallRes::Undef => Ok(None),
            CallRes::Val(v) => {
                if v.deref().is_truthy() {
                    let class_name = match &obj {
                        Value::Object(o) => o.borrow().class_name.clone(),
                        _ => Vec::new(),
                    };
                    let (fid, us) = alloc_stream(obj, class_name, mode, path, false, uw.is_url);
                    let op = match opened.borrow().deref() {
                        Value::String(s) => Some(s.as_bytes().to_vec()),
                        _ => None,
                    };
                    Ok(Some((fid, us, op)))
                } else {
                    log_error(
                        vm,
                        options,
                        format!("\"{}::stream_open\" call failed", cname),
                    );
                    Ok(None)
                }
            }
        }
    })();
    set_current_filename(None);
    IN_USER_INCLUDE.with(|c| c.set(old_in_include));
    result
}

/// user_wrapper_opendir()
fn wrapper_opendir(
    vm: &mut Vm,
    uw: &UserWrapperRef,
    path: &[u8],
    options: i64,
    context: Option<i64>,
) -> Result<Option<(i64, Us)>, VmError> {
    if uw.native {
        log_error(vm, options, "not implemented".to_string());
        return Ok(None);
    }
    if recursion_guard(path) {
        log_error(vm, options, "infinite recursion prevented".to_string());
        return Ok(None);
    }
    set_current_filename(Some(path));
    let result = (|| -> Result<Option<(i64, Us)>, VmError> {
        let obj = match create_object(vm, uw, context)? {
            Some(o) => o,
            None => return Ok(None),
        };
        let cname = match &obj {
            Value::Object(o) => lossy(&o.borrow().class_name),
            _ => String::new(),
        };
        match call_method(
            vm,
            &obj,
            b"dir_opendir",
            vec![s(path), Value::Long(options)],
        )? {
            CallRes::NotImpl => {
                log_error(
                    vm,
                    options,
                    format!("\"{}::dir_opendir\" is not implemented", cname),
                );
                Ok(None)
            }
            CallRes::Undef => Ok(None),
            CallRes::Val(v) => {
                if v.deref().is_truthy() {
                    let class_name = match &obj {
                        Value::Object(o) => o.borrow().class_name.clone(),
                        _ => Vec::new(),
                    };
                    Ok(Some(alloc_stream(
                        obj, class_name, "r", path, true, uw.is_url,
                    )))
                } else {
                    log_error(
                        vm,
                        options,
                        format!("\"{}::dir_opendir\" call failed", cname),
                    );
                    Ok(None)
                }
            }
        }
    })();
    set_current_filename(None);
    result
}

/// _php_stream_opendir()
fn opendir_wrapper(
    vm: &mut Vm,
    path: &[u8],
    options: i64,
    context: Option<i64>,
) -> Result<Option<(i64, Us)>, VmError> {
    if path.is_empty() {
        return Ok(None);
    }
    let loc = locate(vm, path, options);
    let stream = match &loc {
        Loc::User(uw) => wrapper_opendir(vm, uw, path, options & !REPORT_ERRORS, context)?,
        _ => None,
    };
    if stream.is_none() && options & REPORT_ERRORS != 0 {
        display_wrapper_errors(
            vm,
            matches!(loc, Loc::User(_)),
            path,
            "Failed to open directory",
        );
    }
    tidy_wrapper_errors();
    Ok(stream)
}

/// php_userstreamop_readdir(): Ok(None) == no entry
fn op_readdir(vm: &mut Vm, us: &Us) -> Result<Option<Vec<u8>>, VmError> {
    let (obj, cname) = us_obj(us);
    let v = match call_method(vm, &obj, b"dir_readdir", vec![])? {
        CallRes::NotImpl => {
            warn(vm, &format!("{}::dir_readdir is not implemented!", cname));
            return Ok(None);
        }
        CallRes::Undef => return Ok(None),
        CallRes::Val(v) => v.deref(),
    };
    match v {
        Value::True | Value::False => Ok(None),
        Value::Object(_) => match object_to_string(vm, &v)? {
            Some(mut sv) => {
                if let Some(p) = sv.iter().position(|&c| c == 0) {
                    sv.truncate(p);
                }
                Ok(Some(sv))
            }
            None => Ok(None),
        },
        Value::Array(_) => {
            warn_plain(vm, "Array to string conversion");
            Ok(Some(b"Array".to_vec()))
        }
        other => {
            let mut sv = other.to_php_string().as_bytes().to_vec();
            if let Some(p) = sv.iter().position(|&c| c == 0) {
                sv.truncate(p);
            }
            sv.truncate(4095);
            Ok(Some(sv))
        }
    }
}

/// php_userstreamop_rewinddir()
fn op_rewinddir(vm: &mut Vm, us: &Us) -> Result<(), VmError> {
    let (obj, _) = us_obj(us);
    let _ = call_method(vm, &obj, b"dir_rewinddir", vec![])?;
    Ok(())
}

/// user_wrapper_stat_url(): None == failure (-1)
fn wrapper_url_stat(
    vm: &mut Vm,
    uw: &UserWrapperRef,
    url: &[u8],
    flags: i64,
) -> Result<Option<StatBuf>, VmError> {
    if uw.native {
        return Ok(None);
    }
    let obj = match create_object(vm, uw, None)? {
        Some(o) => o,
        None => return Ok(None),
    };
    let cname = lossy(&class_name_of(&obj));
    let r = call_method(vm, &obj, b"url_stat", vec![s(url), Value::Long(flags)]);
    drop(obj);
    vm.run_pending_destructors_pub();
    match r? {
        CallRes::NotImpl => {
            warn(vm, &format!("{}::url_stat is not implemented!", cname));
            Ok(None)
        }
        CallRes::Undef => Ok(None),
        CallRes::Val(v) => match v.deref() {
            Value::Array(a) => Ok(Some(statbuf_from_array(&a.borrow()))),
            _ => Ok(None),
        },
    }
}

fn class_name_of(obj: &Value) -> Vec<u8> {
    match obj {
        Value::Object(o) => o.borrow().class_name.clone(),
        _ => Vec::new(),
    }
}

/// Shared shape of user_wrapper_unlink/rename/mkdir/rmdir/metadata.
fn wrapper_bool_op(
    vm: &mut Vm,
    uw: &UserWrapperRef,
    context: Option<i64>,
    method: &str,
    args: Vec<Value>,
) -> Result<bool, VmError> {
    if uw.native {
        // wops->unlink / rename / mkdir / rmdir are NULL
        match method {
            "unlink" => warn(vm, "ZLIB does not allow unlinking"),
            "rename" => warn(vm, "ZLIB wrapper does not support renaming"),
            _ => {}
        }
        return Ok(false);
    }
    let obj = match create_object(vm, uw, context)? {
        Some(o) => o,
        None => return Ok(false),
    };
    let cname = lossy(&class_name_of(&obj));
    let r = call_method(vm, &obj, method.as_bytes(), args);
    drop(obj);
    vm.run_pending_destructors_pub();
    let r = r?;
    match r {
        CallRes::NotImpl => {
            warn(vm, &format!("{}::{} is not implemented!", cname, method));
            Ok(false)
        }
        CallRes::Undef => Ok(false),
        CallRes::Val(v) => Ok(matches!(v.deref(), Value::True)),
    }
}

/// php_stream_stat_path_ex() for any path: user wrappers dispatch to
/// url_stat, other paths use the plain/builtin implementation.
fn stat_path_any(vm: &mut Vm, path: &[u8], flags: i64) -> Result<Option<StatBuf>, VmError> {
    if needs_locate(path) {
        match locate(vm, path, 0) {
            Loc::User(uw) => return wrapper_url_stat(vm, &uw, path, flags),
            Loc::None => return Ok(None),
            Loc::Builtin { .. } => {}
        }
    }
    let local = strip_file_scheme(&lossy(path)).unwrap_or_else(|| lossy(path));
    let md = if flags & PHP_STREAM_URL_STAT_LINK != 0 {
        std::fs::symlink_metadata(&local)
    } else {
        std::fs::metadata(&local)
    };
    match md {
        Ok(m) => {
            use std::os::unix::fs::MetadataExt;
            Ok(Some(StatBuf {
                dev: m.dev() as i64,
                ino: m.ino() as i64,
                mode: m.mode() as i64,
                nlink: m.nlink() as i64,
                uid: m.uid() as i64,
                gid: m.gid() as i64,
                rdev: m.rdev() as i64,
                size: m.size() as i64,
                atime: m.atime(),
                mtime: m.mtime(),
                ctime: m.ctime(),
                blksize: m.blksize() as i64,
                blocks: m.blocks() as i64,
            }))
        }
        Err(_) => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Generic stream helpers (user streams + streams owned by the original
// implementation, which are driven through the original builtins)
// ---------------------------------------------------------------------------

fn call_orig(vm: &mut Vm, name: &'static str, args: &[Value]) -> Result<Value, VmError> {
    let f = ORIG.with(|m| m.borrow().get(name).copied());
    match f {
        Some(f) => f(vm, args),
        None => {
            let lower = name.as_bytes().to_vec();
            match vm.functions.get(&lower).copied() {
                Some(f) => f(vm, args),
                None => Ok(Value::False),
            }
        }
    }
}

/// php_stream_read() on any stream: None == error (-1)
fn any_read(vm: &mut Vm, fid: i64, n: usize) -> Result<Option<Vec<u8>>, VmError> {
    if let Some(us) = get_us(fid) {
        return stream_read(vm, fid, &us, n);
    }
    match call_orig(vm, "fread", &[Value::Resource(fid), Value::Long(n as i64)])? {
        Value::String(s) => Ok(Some(s.as_bytes().to_vec())),
        _ => Ok(None),
    }
}

/// php_stream_write() on any stream
fn any_write(vm: &mut Vm, fid: i64, data: &[u8]) -> Result<i64, VmError> {
    if let Some(us) = get_us(fid) {
        return stream_write(vm, fid, &us, data);
    }
    match call_orig(vm, "fwrite", &[Value::Resource(fid), s(data)])? {
        Value::Long(n) => Ok(n),
        _ => Ok(-1),
    }
}

fn any_close(vm: &mut Vm, fid: i64) -> Result<(), VmError> {
    if let Some(us) = get_us(fid) {
        return stream_free(vm, fid, &us);
    }
    call_orig(vm, "fclose", &[Value::Resource(fid)])?;
    Ok(())
}

fn any_seek(vm: &mut Vm, fid: i64, offset: i64, whence: i64) -> Result<i32, VmError> {
    if let Some(us) = get_us(fid) {
        return stream_seek(vm, fid, &us, offset, whence);
    }
    Ok(call_orig(
        vm,
        "fseek",
        &[
            Value::Resource(fid),
            Value::Long(offset),
            Value::Long(whence),
        ],
    )?
    .to_long() as i32)
}

/// Open any path as a stream (php_stream_open_wrapper_ex with REPORT_ERRORS).
fn any_open(
    vm: &mut Vm,
    path: &[u8],
    mode: &str,
    options: i64,
    context: Option<i64>,
) -> Result<Option<i64>, VmError> {
    if needs_locate(path) {
        match open_located(vm, path, mode, options, context)? {
            Opened::Stream(fid, _) => return Ok(Some(fid)),
            Opened::Failed => return Ok(None),
            Opened::Builtin(p) => {
                let mut a = vec![s(&p), s(mode.as_bytes())];
                if options & USE_PATH != 0 {
                    a.push(Value::True);
                }
                return Ok(fid_of(&call_orig(vm, "fopen", &a)?));
            }
        }
    }
    let mut a = vec![s(path), s(mode.as_bytes())];
    if options & USE_PATH != 0 {
        a.push(Value::True);
    }
    Ok(fid_of(&call_orig(vm, "fopen", &a)?))
}

enum Opened {
    Stream(i64, Us),
    Failed,
    /// Not a user wrapper: use the builtin implementation with this path.
    Builtin(Vec<u8>),
}

/// _php_stream_open_wrapper_ex(): locate the wrapper and open the stream.
fn open_located(
    vm: &mut Vm,
    path: &[u8],
    mode: &str,
    options: i64,
    context: Option<i64>,
) -> Result<Opened, VmError> {
    if path.is_empty() {
        return Err(throw(vm, b"ValueError", "Path must not be empty"));
    }
    match locate(vm, path, options) {
        Loc::Builtin { path } => Ok(Opened::Builtin(path)),
        Loc::None => {
            if options & REPORT_ERRORS != 0 {
                display_wrapper_errors(vm, false, path, "Failed to open stream");
            }
            tidy_wrapper_errors();
            Ok(Opened::Failed)
        }
        Loc::User(uw) => {
            let r = wrapper_open(vm, &uw, path, mode, options & !REPORT_ERRORS, context)?;
            let stream = r.map(|(fid, us, _)| (fid, us));
            if let Some((_, us)) = &stream {
                if mode.contains('a')
                    && us.borrow().flags & FLAG_NO_SEEK == 0
                    && us.borrow().position == 0
                {
                    if let (0, Some(p)) = op_seek(vm, us, 0, SEEK_CUR)? {
                        us.borrow_mut().position = p;
                    }
                }
            }
            if stream.is_none() && options & REPORT_ERRORS != 0 {
                display_wrapper_errors(vm, true, path, "Failed to open stream");
            }
            tidy_wrapper_errors();
            Ok(match stream {
                Some((fid, us)) => Opened::Stream(fid, us),
                None => Opened::Failed,
            })
        }
    }
}

/// _php_stream_copy_to_stream_ex(): (success, bytes)
fn copy_stream(
    vm: &mut Vm,
    src: i64,
    dest: i64,
    maxlen: Option<usize>,
) -> Result<(bool, usize), VmError> {
    if maxlen == Some(0) {
        return Ok((true, 0));
    }
    let mut haveread = 0usize;
    loop {
        let mut readchunk = CHUNK_SIZE;
        if let Some(m) = maxlen {
            if m - haveread < readchunk {
                readchunk = m - haveread;
            }
        }
        let data = match any_read(vm, src, readchunk)? {
            None => return Ok((false, haveread)),
            Some(d) if d.is_empty() => return Ok((true, haveread)),
            Some(d) => d,
        };
        haveread += data.len();
        let mut off = 0;
        while off < data.len() {
            let w = any_write(vm, dest, &data[off..])?;
            if w <= 0 {
                return Ok((false, haveread - (data.len() - off)));
            }
            off += w as usize;
        }
        if maxlen == Some(haveread) {
            break;
        }
        if vm.current_exception.is_some() {
            break;
        }
    }
    Ok((true, haveread))
}

// ---------------------------------------------------------------------------
// Builtin interception
// ---------------------------------------------------------------------------

type Handler = fn(&mut Vm, &[Value]) -> Result<Option<Value>, VmError>;

fn run(vm: &mut Vm, name: &'static str, args: &[Value], h: Handler) -> Result<Value, VmError> {
    FN_STACK.with(|s| s.borrow_mut().push(name));
    let r = h(vm, args);
    FN_STACK.with(|s| {
        s.borrow_mut().pop();
    });
    match r {
        Ok(None) => call_orig(vm, name, args),
        Ok(Some(v)) => {
            if vm.current_exception.is_some() {
                Err(exc_err(vm))
            } else {
                Ok(v)
            }
        }
        Err(e) => Err(e),
    }
}

fn b(v: bool) -> Value {
    if v { Value::True } else { Value::False }
}

fn int_arg(args: &[Value], i: usize) -> Option<i64> {
    match args.get(i).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(v) => Some(v.to_long()),
    }
}

// --- stream functions -------------------------------------------------------

fn h_fread(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let len = int_arg(args, 1).unwrap_or(0);
    if len <= 0 {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($length) must be greater than 0",
                cur_fn()
            ),
        ));
    }
    Ok(Some(match stream_read(vm, fid, &us, len as usize)? {
        Some(d) => s(&d),
        None => Value::False,
    }))
}

fn h_fwrite(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let data = args
        .get(1)
        .map(|v| v.deref().to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let num = match int_arg(args, 2) {
        None => data.len(),
        Some(n) if n <= 0 => 0,
        Some(n) => (n as usize).min(data.len()),
    };
    if num == 0 {
        return Ok(Some(Value::Long(0)));
    }
    let r = stream_write(vm, fid, &us, &data[..num])?;
    Ok(Some(if r < 0 { Value::False } else { Value::Long(r) }))
}

fn h_fgets(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let r = match int_arg(args, 1) {
        None => stream_get_line(vm, fid, &us, None)?,
        Some(n) => {
            if n <= 0 {
                return Err(throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #2 ($length) must be greater than 0",
                        cur_fn()
                    ),
                ));
            }
            stream_get_line(vm, fid, &us, Some(n as usize))?
        }
    };
    Ok(Some(match r {
        Some(d) => s(&d),
        None => Value::False,
    }))
}

fn h_fgetc(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(match stream_read(vm, fid, &us, 1)? {
        Some(d) if !d.is_empty() => s(&d),
        _ => Value::False,
    }))
}

fn h_feof(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(b(stream_eof(vm, &us)?)))
}

fn h_fclose(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let fid = match args.first().and_then(fid_of) {
        Some(f) => f,
        None => return Ok(None),
    };
    let us = match get_us(fid) {
        Some(u) => u,
        None => {
            if goro_core::value::is_registered_resource(fid)
                && !goro_core::value::is_open_resource(fid)
            {
                return Err(throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #1 ($stream) must be an open stream resource",
                        cur_fn()
                    ),
                ));
            }
            return Ok(None);
        }
    };
    if us.borrow().flags & FLAG_NO_FCLOSE != 0 {
        warn(
            vm,
            "cannot close the provided stream, as it must not be manually closed",
        );
        return Ok(Some(Value::False));
    }
    stream_free(vm, fid, &us)?;
    Ok(Some(Value::True))
}

fn h_fflush(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(b(stream_flush(vm, fid, &us)? == 0)))
}

fn h_fseek(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let off = int_arg(args, 1).unwrap_or(0);
    let whence = int_arg(args, 2).unwrap_or(SEEK_SET);
    Ok(Some(Value::Long(
        stream_seek(vm, fid, &us, off, whence)? as i64
    )))
}

fn h_ftell(_vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let p = us.borrow().position;
    Ok(Some(if p == -1 {
        Value::False
    } else {
        Value::Long(p)
    }))
}

fn h_rewind(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(b(stream_seek(vm, fid, &us, 0, SEEK_SET)? != -1)))
}

fn h_fstat(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(match op_stat(vm, &us)? {
        Some(sb) => stat_to_array(&sb),
        None => Value::False,
    }))
}

fn h_ftruncate(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let size = int_arg(args, 1).unwrap_or(0);
    if size < 0 {
        return Err(throw(
            vm,
            b"ValueError",
            "ftruncate(): Argument #2 ($size) must be greater than or equal to 0",
        ));
    }
    if op_truncate(vm, &us, None)? != OPT_OK {
        warn(vm, "Can't truncate this stream!");
        return Ok(Some(Value::False));
    }
    Ok(Some(b(op_truncate(vm, &us, Some(size))? == OPT_OK)))
}

fn h_flock(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let operation = int_arg(args, 1).unwrap_or(0);
    let act = operation & 3;
    if !(1..=3).contains(&act) {
        return Err(throw(
            vm,
            b"ValueError",
            "flock(): Argument #2 ($operation) must be one of LOCK_SH, LOCK_EX, or LOCK_UN",
        ));
    }
    if let Some(Value::Reference(r)) = args.get(2) {
        *r.borrow_mut() = Value::Long(0);
    }
    // flock_values[] = { LOCK_SH, LOCK_EX, LOCK_UN } (system values)
    let sys = [1i64, 2, 8][(act - 1) as usize] | if operation & 4 != 0 { 4 } else { 0 };
    Ok(Some(b(op_lock(vm, &us, sys)? == 0)))
}

fn h_stream_supports_lock(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(b(op_lock(vm, &us, 0)? == 0)))
}

fn h_stream_get_contents(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let maxlen = match int_arg(args, 1) {
        None => None,
        Some(-1) => None,
        Some(n) if n < 0 => {
            return Err(throw(
                vm,
                b"ValueError",
                "stream_get_contents(): Argument #2 ($length) must be greater than or equal to -1",
            ));
        }
        Some(n) => Some(n as usize),
    };
    let desiredpos = int_arg(args, 2).unwrap_or(-1);
    if desiredpos >= 0 {
        let position = us.borrow().position;
        let mut seek_res = 0;
        if position >= 0 && desiredpos > position {
            seek_res = stream_seek(vm, fid, &us, desiredpos - position, SEEK_CUR)?;
        } else if desiredpos < position {
            seek_res = stream_seek(vm, fid, &us, desiredpos, SEEK_SET)?;
        }
        if seek_res != 0 {
            warn(
                vm,
                &format!("Failed to seek to position {} in the stream", desiredpos),
            );
            return Ok(Some(Value::False));
        }
    }
    Ok(Some(match copy_to_mem(vm, fid, &us, maxlen)? {
        Some(d) => s(&d),
        None => s(b""),
    }))
}

fn h_stream_get_line(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let mut maxlen = int_arg(args, 1).unwrap_or(0);
    if maxlen < 0 {
        return Err(throw(
            vm,
            b"ValueError",
            "stream_get_line(): Argument #2 ($length) must be greater than or equal to 0",
        ));
    }
    if maxlen == 0 {
        maxlen = CHUNK_SIZE as i64;
    }
    let delim = arg_str(args, 2).unwrap_or_default();
    Ok(Some(
        match stream_get_record(vm, fid, &us, maxlen as usize, &delim)? {
            Some(d) => s(&d),
            None => Value::False,
        },
    ))
}

fn h_fpassthru(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(Value::Long(passthru(vm, fid, &us)?)))
}

/// _php_stream_passthru()
fn passthru(vm: &mut Vm, fid: i64, us: &Us) -> Result<i64, VmError> {
    let mut count = 0i64;
    loop {
        match stream_read(vm, fid, us, CHUNK_SIZE)? {
            Some(d) if !d.is_empty() => {
                vm.write_output(&d);
                count += d.len() as i64;
            }
            None if count == 0 => return Ok(-1),
            _ => break,
        }
    }
    Ok(count)
}

fn h_stream_get_meta_data(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let fid = match args.first().and_then(fid_of) {
        Some(f) => f,
        None => return Ok(None),
    };
    let us = match get_us(fid) {
        Some(u) => u,
        None => return Ok(None),
    };
    let is_dir = us.borrow().flags & FLAG_IS_DIR != 0;
    let eof = if is_dir {
        us.borrow().eof
    } else {
        stream_eof(vm, &us)?
    };
    let bb = us.borrow();
    let mut a = new_arr();
    let set =
        |a: &mut PhpArray, k: &[u8], v: Value| a.set(ArrayKey::String(PhpString::from_bytes(k)), v);
    set(&mut a, b"timed_out", Value::False);
    set(&mut a, b"blocked", Value::True);
    set(&mut a, b"eof", b(eof));
    if let Some(n) = &bb.native {
        if let Some(l) = bb.wrapper_label {
            set(&mut a, b"wrapper_type", s(l.as_bytes()));
        }
        set(&mut a, b"stream_type", s(n.borrow().label().as_bytes()));
        set(&mut a, b"mode", s(bb.mode.as_bytes()));
        set(&mut a, b"unread_bytes", Value::Long(bb.buffered() as i64));
        set(&mut a, b"seekable", b(bb.flags & FLAG_NO_SEEK == 0));
        if !bb.orig_path.is_empty() {
            set(&mut a, b"uri", s(bb.orig_path.as_bytes()));
        }
        return Ok(Some(arr_val(a)));
    }
    set(&mut a, b"wrapper_data", bb.obj.clone());
    set(&mut a, b"wrapper_type", s(b"user-space"));
    set(
        &mut a,
        b"stream_type",
        s(if is_dir {
            b"user-space-dir".as_slice()
        } else {
            b"user-space".as_slice()
        }),
    );
    set(&mut a, b"mode", s(bb.mode.as_bytes()));
    set(&mut a, b"unread_bytes", Value::Long(bb.buffered() as i64));
    set(&mut a, b"seekable", b(bb.flags & FLAG_NO_SEEK == 0));
    set(&mut a, b"uri", s(bb.orig_path.as_bytes()));
    Ok(Some(arr_val(a)))
}

fn h_stream_set_blocking(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let block = args.get(1).map(|v| v.deref().is_truthy()).unwrap_or(false) as i64;
    Ok(Some(b(stream_set_option(
        vm,
        &us,
        PHP_STREAM_OPTION_BLOCKING,
        block,
        None,
    )? != -1)))
}

fn h_stream_set_timeout(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let mut sec = int_arg(args, 1).unwrap_or(0);
    let usec = match int_arg(args, 2) {
        Some(u) => {
            sec += u / 1_000_000;
            u % 1_000_000
        }
        None => 0,
    };
    Ok(Some(b(stream_set_option(
        vm,
        &us,
        PHP_STREAM_OPTION_READ_TIMEOUT,
        sec,
        Some(usec),
    )? == OPT_OK)))
}

fn set_buffer(vm: &mut Vm, args: &[Value], option: i64) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let buff = int_arg(args, 1).unwrap_or(0);
    let ret = if buff == 0 {
        stream_set_option(vm, &us, option, 0, None)?
    } else {
        stream_set_option(vm, &us, option, 2, Some(buff))?
    };
    Ok(Some(Value::Long(if ret == 0 { 0 } else { -1 })))
}

fn h_stream_set_write_buffer(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    set_buffer(vm, args, PHP_STREAM_OPTION_WRITE_BUFFER)
}

fn h_stream_set_read_buffer(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    set_buffer(vm, args, PHP_STREAM_OPTION_READ_BUFFER)
}

fn h_stream_set_chunk_size(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let csize = int_arg(args, 1).unwrap_or(0);
    if csize <= 0 {
        return Err(throw(
            vm,
            b"ValueError",
            "stream_set_chunk_size(): Argument #2 ($size) must be greater than 0",
        ));
    }
    let mut bm = us.borrow_mut();
    let old = bm.chunk_size.min(i32::MAX as usize) as i64;
    bm.chunk_size = csize as usize;
    Ok(Some(Value::Long(if old > 0 { old } else { -1 })))
}

fn h_fsync(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    if us_arg(args, 0).is_none() {
        return Ok(None);
    }
    warn(vm, &format!("Can't {} this stream!", cur_fn()));
    Ok(Some(Value::False))
}

fn h_stream_isatty(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    match op_cast(vm, &us, fid, true, false)? {
        Some(inner) => Ok(Some(call_orig(
            vm,
            "stream_isatty",
            &[Value::Resource(inner)],
        )?)),
        None => Ok(Some(Value::False)),
    }
}

fn h_stream_copy_to_stream(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let src = match args.first().and_then(fid_of) {
        Some(f) => f,
        None => return Ok(None),
    };
    let dest = match args.get(1).and_then(fid_of) {
        Some(f) => f,
        None => return Ok(None),
    };
    if get_us(src).is_none() && get_us(dest).is_none() {
        return Ok(None);
    }
    let maxlen = match int_arg(args, 2) {
        None | Some(-1) => None,
        Some(n) => Some(n.max(0) as usize),
    };
    let pos = int_arg(args, 3).unwrap_or(0);
    if pos > 0 && any_seek(vm, src, pos, SEEK_SET)? < 0 {
        warn(
            vm,
            &format!("Failed to seek to position {} in the stream", pos),
        );
        return Ok(Some(Value::False));
    }
    let (ok, len) = copy_stream(vm, src, dest, maxlen)?;
    Ok(Some(if ok {
        Value::Long(len as i64)
    } else {
        Value::False
    }))
}

fn h_fprintf(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let formatted = call_orig(vm, "sprintf", &args[1..])?;
    let data = formatted.to_php_string().as_bytes().to_vec();
    stream_write(vm, fid, &us, &data)?;
    Ok(Some(Value::Long(data.len() as i64)))
}

fn h_vfprintf(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let formatted = call_orig(vm, "vsprintf", &args[1..])?;
    let data = formatted.to_php_string().as_bytes().to_vec();
    stream_write(vm, fid, &us, &data)?;
    Ok(Some(Value::Long(data.len() as i64)))
}

fn fd_of(fid: i64) -> Option<i32> {
    match fid {
        1 => Some(0),
        2 => Some(1),
        3 => Some(2),
        _ => FILE_HANDLES.with(|h| {
            use std::os::unix::io::AsRawFd;
            h.borrow().get(&fid).map(|fh| fh.file.as_raw_fd())
        }),
    }
}

fn h_stream_select(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let mut any_user = false;
    for i in 0..3 {
        if let Some(Value::Array(a)) = args.get(i).map(|v| v.deref()) {
            for v in a.borrow().values() {
                if fid_of(v).and_then(get_us).is_some() {
                    any_user = true;
                }
            }
        }
    }
    if !any_user {
        return Ok(None);
    }
    let mut pfds: Vec<libc::pollfd> = Vec::new();
    let mut sets = 0;
    for i in 0..3 {
        let arr = match args.get(i).map(|v| v.deref()) {
            Some(Value::Array(a)) => a,
            _ => continue,
        };
        let vals: Vec<Value> = arr.borrow().values().cloned().collect();
        let mut cnt = 0;
        for v in vals {
            let fid = match fid_of(&v) {
                Some(f) => f,
                None => continue,
            };
            let real = match get_us(fid) {
                Some(us) => op_cast(vm, &us, fid, true, true)?,
                None => Some(fid),
            };
            match real.and_then(fd_of) {
                Some(fd) => {
                    let events = match i {
                        0 => libc::POLLIN,
                        1 => libc::POLLOUT,
                        _ => libc::POLLPRI,
                    };
                    pfds.push(libc::pollfd {
                        fd,
                        events,
                        revents: 0,
                    });
                    cnt += 1;
                }
                None => {
                    warn(
                        vm,
                        "Cannot represent a stream of type user-space as a select()able descriptor",
                    );
                }
            }
        }
        sets += cnt;
    }
    if sets == 0 {
        return Err(throw(vm, b"ValueError", "No stream arrays were passed"));
    }
    let sec = int_arg(args, 3);
    let usec = int_arg(args, 4).unwrap_or(0);
    let timeout = match sec {
        None => -1,
        Some(sv) => (sv * 1000 + usec / 1000) as i32,
    };
    let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout) };
    Ok(Some(if n < 0 {
        Value::False
    } else {
        Value::Long(pfds.iter().filter(|p| p.revents != 0).count() as i64)
    }))
}

fn h_fgetcsv(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let maxlen = match int_arg(args, 1) {
        None | Some(0) => None,
        Some(n) if n < 0 => {
            return Err(throw(
                vm,
                b"ValueError",
                "fgetcsv(): Argument #2 ($length) must be between 0 and 2147483647",
            ));
        }
        Some(n) => Some(n as usize + 1),
    };
    let enclosure = arg_str(args, 3).unwrap_or_else(|| b"\"".to_vec());
    let mut line = match stream_get_line(vm, fid, &us, maxlen)? {
        Some(l) => l,
        None => return Ok(Some(Value::False)),
    };
    // php_fgetcsv() keeps reading lines while inside an enclosure.
    if let Some(&q) = enclosure.first() {
        while line.iter().filter(|&&c| c == q).count() % 2 == 1 {
            match stream_get_line(vm, fid, &us, maxlen)? {
                Some(more) => line.extend_from_slice(&more),
                None => break,
            }
        }
    }
    while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
        line.pop();
    }
    if line.is_empty() {
        // A blank line yields array(null).
        let mut a = new_arr();
        a.push(Value::Null);
        return Ok(Some(arr_val(a)));
    }
    let mut a = vec![s(&line)];
    a.extend(args.iter().skip(2).cloned());
    Ok(Some(call_orig(vm, "str_getcsv", &a)?))
}

/// Run an original builtin that formats into a stream against a
/// php://memory stream and return what it wrote.
fn format_via_memory(
    vm: &mut Vm,
    name: &'static str,
    rest: &[Value],
) -> Result<Option<Vec<u8>>, VmError> {
    let mem = match fid_of(&call_orig(vm, "fopen", &[s(b"php://memory"), s(b"w+")])?) {
        Some(f) => f,
        None => return Ok(None),
    };
    let mut a = vec![Value::Resource(mem)];
    a.extend_from_slice(rest);
    let r = call_orig(vm, name, &a);
    call_orig(vm, "rewind", &[Value::Resource(mem)])?;
    let out = call_orig(vm, "stream_get_contents", &[Value::Resource(mem)])?;
    call_orig(vm, "fclose", &[Value::Resource(mem)])?;
    match r? {
        Value::False => Ok(None),
        _ => Ok(Some(out.to_php_string().as_bytes().to_vec())),
    }
}

fn h_fputcsv(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let line = match format_via_memory(vm, "fputcsv", &args[1..])? {
        Some(l) => l,
        None => return Ok(Some(Value::False)),
    };
    let r = stream_write(vm, fid, &us, &line)?;
    Ok(Some(if r < 0 { Value::False } else { Value::Long(r) }))
}

fn h_fscanf(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match us_arg(args, 0) {
        Some(x) => x,
        None => return Ok(None),
    };
    let line = match stream_get_line(vm, fid, &us, None)? {
        Some(l) => l,
        None => return Ok(Some(Value::False)),
    };
    let mut a = vec![s(&line)];
    a.extend(args.iter().skip(1).cloned());
    Ok(Some(call_orig(vm, "sscanf", &a)?))
}

fn h_stream_is_local(_vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let is_url = match &v {
        Value::Resource(fid) => match get_us(*fid) {
            Some(us) => us.borrow().is_url,
            None => return Ok(None),
        },
        Value::String(sv) => {
            let p = sv.as_bytes();
            match scheme_len(p).and_then(|n| user_wrapper(&p[..n].to_ascii_lowercase())) {
                Some(u) => u.is_url,
                None => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(b(!is_url)))
}

fn h_stream_resolve_include_path(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    if USER_WRAPPERS.with(|w| w.borrow().is_empty()) {
        return Ok(None);
    }
    match resolve_path_user(vm, &p) {
        Some(Ok(r)) => Ok(Some(s(&r))),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

// --- path functions ---------------------------------------------------------

/// Locate the wrapper for a path argument of a filesystem function:
/// Some(uw) for user wrappers; Err(Some(path)) to defer to the original
/// implementation with that path; Err(None) when no wrapper was found.
fn user_path(vm: &mut Vm, path: &[u8], options: i64) -> Result<UserWrapperRef, Option<Vec<u8>>> {
    if !needs_locate(path) {
        return Err(Some(path.to_vec()));
    }
    match locate(vm, path, options) {
        Loc::User(u) => Ok(u),
        Loc::Builtin { path: p } => Err(Some(p)),
        Loc::None => Err(None),
    }
}

fn with_path(args: &[Value], i: usize, p: Vec<u8>) -> Vec<Value> {
    let mut a = args.to_vec();
    if i < a.len() {
        a[i] = s(&p);
    }
    a
}

macro_rules! path_or_orig {
    ($vm:expr, $name:expr, $args:expr, $idx:expr, $opts:expr) => {{
        let p = match arg_str($args, $idx) {
            Some(p) => p,
            None => return Ok(None),
        };
        match user_path($vm, &p, $opts) {
            Ok(u) => (u, p),
            Err(Some(np)) => {
                if np == p {
                    return Ok(None);
                }
                return Ok(Some(call_orig($vm, $name, &with_path($args, $idx, np))?));
            }
            Err(None) => return Ok(Some(Value::False)),
        }
    }};
}

/// USE_PATH: resolve a relative name through include_path entries served
/// by userspace wrappers.
fn use_path_resolve(vm: &mut Vm, p: Vec<u8>, use_path: bool) -> Result<Vec<u8>, VmError> {
    if !use_path || needs_locate(&p) || USER_WRAPPERS.with(|w| w.borrow().is_empty()) {
        return Ok(p);
    }
    match resolve_path_user(vm, &p) {
        Some(Ok(r)) => Ok(r),
        Some(Err(e)) => Err(e),
        None => Ok(p),
    }
}

fn h_fopen(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    let use_path = args.get(2).map(|v| v.deref().is_truthy()).unwrap_or(false);
    let p = use_path_resolve(vm, p, use_path)?;
    if !needs_locate(&p) {
        return Ok(None);
    }
    let mode = arg_str(args, 1)
        .map(|m| lossy(&m))
        .unwrap_or_else(|| "r".into());
    let ctx = context_from_arg(vm, args.get(3), false)?;
    let options = if use_path { USE_PATH } else { 0 } | REPORT_ERRORS;
    match open_located(vm, &p, &mode, options, ctx)? {
        Opened::Stream(fid, _) => Ok(Some(Value::Resource(fid))),
        Opened::Failed => Ok(Some(Value::False)),
        Opened::Builtin(np) => {
            if np == p {
                Ok(None)
            } else {
                Ok(Some(call_orig(vm, "fopen", &with_path(args, 0, np))?))
            }
        }
    }
}

fn h_file_get_contents(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    let use_path = args.get(1).map(|v| v.deref().is_truthy()).unwrap_or(false);
    let p = use_path_resolve(vm, p, use_path)?;
    let filter_inner = p.starts_with(b"php://filter/")
        && p.windows(9)
            .position(|w| w == b"resource=")
            .map(|i| needs_locate(&p[i + 9..]))
            .unwrap_or(false);
    if !needs_locate(&p) && !filter_inner {
        return Ok(None);
    }
    let offset = int_arg(args, 3).unwrap_or(0);
    let maxlen = match int_arg(args, 4) {
        None => None,
        Some(n) if n < 0 => {
            return Err(throw(
                vm,
                b"ValueError",
                "file_get_contents(): Argument #5 ($length) must be greater than or equal to 0",
            ));
        }
        Some(n) => Some(n as usize),
    };
    let ctx = context_from_arg(vm, args.get(2), false)?;
    let options = if use_path { USE_PATH } else { 0 } | REPORT_ERRORS;
    let (fid, us) = if filter_inner {
        // php://filter over a user stream: the filter wrapper opens the
        // inner resource and stacks the filters on it.
        let ctx_val = ctx.map(Value::Resource).unwrap_or(Value::Null);
        let r = call_orig(vm, "fopen", &[s(&p), s(b"rb"), Value::False, ctx_val])?;
        match fid_of(&r).and_then(|f| get_us(f).map(|u| (f, u))) {
            Some(x) => x,
            None => {
                if let Some(f) = fid_of(&r) {
                    let c = call_orig(vm, "stream_get_contents", &[Value::Resource(f)])?;
                    call_orig(vm, "fclose", &[Value::Resource(f)])?;
                    return Ok(Some(c));
                }
                return Ok(Some(Value::False));
            }
        }
    } else {
        match open_located(vm, &p, "rb", options, ctx)? {
            Opened::Stream(fid, us) => (fid, us),
            Opened::Failed => return Ok(Some(Value::False)),
            Opened::Builtin(np) => {
                if np == p {
                    return Ok(None);
                }
                return Ok(Some(call_orig(
                    vm,
                    "file_get_contents",
                    &with_path(args, 0, np),
                )?));
            }
        }
    };
    if offset != 0
        && stream_seek(
            vm,
            fid,
            &us,
            offset,
            if offset > 0 { SEEK_SET } else { SEEK_END },
        )? < 0
    {
        warn(
            vm,
            &format!("Failed to seek to position {} in the stream", offset),
        );
        stream_free(vm, fid, &us)?;
        return Ok(Some(Value::False));
    }
    let contents = copy_to_mem(vm, fid, &us, maxlen)?;
    stream_free(vm, fid, &us)?;
    Ok(Some(s(&contents.unwrap_or_default())))
}

/// zval_get_tmp_string()-like conversion for file_put_contents data.
fn data_to_string(vm: &mut Vm, v: &Value) -> Result<Option<Vec<u8>>, VmError> {
    match v {
        Value::Array(_) => {
            warn_plain(vm, "Array to string conversion");
            Ok(Some(b"Array".to_vec()))
        }
        Value::Object(_) => object_to_string(vm, v),
        other => Ok(Some(other.to_php_string().as_bytes().to_vec())),
    }
}

fn h_file_put_contents(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let data = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
    let src_fid = fid_of(&data);
    let src_is_user = src_fid.and_then(get_us).is_some();
    let ctx_is_stream = matches!(args.get(3).map(|v| v.deref()), Some(Value::Resource(id))
        if goro_core::value::resource_type_of(id).as_deref() != Some(b"stream-context".as_slice()));
    let p = arg_str(args, 0).unwrap_or_default();
    let user = !p.is_empty() && needs_locate(&p);
    if !user && !src_is_user && !ctx_is_stream {
        return Ok(None);
    }
    let flags = int_arg(args, 2).unwrap_or(0);
    let ctx = context_from_arg(vm, args.get(3), flags & 16 != 0)?;
    if !user {
        if !src_is_user {
            return Ok(None);
        }
        // Read the user stream, then let the plain implementation write it.
        let sfid = src_fid.unwrap();
        let mut all = Vec::new();
        loop {
            match any_read(vm, sfid, CHUNK_SIZE)? {
                Some(d) if !d.is_empty() => all.extend_from_slice(&d),
                _ => break,
            }
            if vm.current_exception.is_some() {
                return Ok(Some(Value::False));
            }
        }
        let mut a = args.to_vec();
        a[1] = s(&all);
        return Ok(Some(call_orig(vm, "file_put_contents", &a)?));
    }
    let mut mode = if flags & 8 != 0 {
        "ab"
    } else if flags & 2 != 0 {
        "cb"
    } else {
        "wb"
    };
    let options = if flags & 1 != 0 { USE_PATH } else { 0 } | REPORT_ERRORS;
    let (fid, us) = match open_located(vm, &p, mode, options, ctx)? {
        Opened::Stream(fid, us) => (fid, us),
        Opened::Failed => return Ok(Some(Value::False)),
        Opened::Builtin(np) => {
            return Ok(Some(call_orig(
                vm,
                "file_put_contents",
                &with_path(args, 0, np),
            )?));
        }
    };
    if flags & 2 != 0 && (op_lock(vm, &us, 0)? != 0 || op_lock(vm, &us, 2)? != 0) {
        stream_free(vm, fid, &us)?;
        warn(vm, "Exclusive locks are not supported for this stream");
        return Ok(Some(Value::False));
    }
    if mode.starts_with('c') {
        op_truncate(vm, &us, Some(0))?;
        mode = "cb";
        let _ = mode;
    }
    let mut numbytes: i64 = 0;
    match &data {
        Value::Resource(sfid) => {
            let (ok, len) = copy_stream(vm, *sfid, fid, None)?;
            numbytes = if ok { len as i64 } else { -1 };
        }
        Value::Array(arr) => {
            let vals: Vec<Value> = arr.borrow().values().cloned().collect();
            for v in vals {
                let sv = match data_to_string(vm, &v.deref())? {
                    Some(sv) => sv,
                    None => {
                        numbytes = -1;
                        break;
                    }
                };
                if !sv.is_empty() {
                    numbytes += sv.len() as i64;
                    let w = stream_write(vm, fid, &us, &sv)?;
                    if w != sv.len() as i64 {
                        warn(
                            vm,
                            &format!("Failed to write {} bytes to {}", sv.len(), lossy(&p)),
                        );
                        numbytes = -1;
                        break;
                    }
                }
            }
        }
        Value::Object(_) => match object_to_string(vm, &data)? {
            Some(sv) => {
                numbytes = stream_write(vm, fid, &us, &sv)?;
                if numbytes != -1 && numbytes != sv.len() as i64 {
                    warn(
                        vm,
                        &format!(
                            "Only {} of {} bytes written, possibly out of free disk space",
                            numbytes,
                            sv.len()
                        ),
                    );
                    numbytes = -1;
                }
            }
            None => numbytes = -1,
        },
        other => {
            let sv = other.to_php_string().as_bytes().to_vec();
            if !sv.is_empty() {
                numbytes = stream_write(vm, fid, &us, &sv)?;
                if numbytes != -1 && numbytes != sv.len() as i64 {
                    warn(
                        vm,
                        &format!(
                            "Only {} of {} bytes written, possibly out of free disk space",
                            numbytes,
                            sv.len()
                        ),
                    );
                    numbytes = -1;
                }
            }
        }
    }
    stream_free(vm, fid, &us)?;
    Ok(Some(if numbytes < 0 {
        Value::False
    } else {
        Value::Long(numbytes)
    }))
}

fn h_file(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    if !needs_locate(&p) {
        return Ok(None);
    }
    let flags = int_arg(args, 1).unwrap_or(0);
    if !(0..=(1 | 2 | 4 | 16)).contains(&flags) {
        return Err(throw(
            vm,
            b"ValueError",
            "file(): Argument #2 ($flags) must be a valid flag value",
        ));
    }
    let include_new_line = flags & 2 == 0;
    let skip_blank_lines = flags & 4 != 0;
    let ctx = context_from_arg(vm, args.get(2), flags & 16 != 0)?;
    let options = if flags & 1 != 0 { USE_PATH } else { 0 } | REPORT_ERRORS;
    let (fid, us) = match open_located(vm, &p, "rb", options, ctx)? {
        Opened::Stream(fid, us) => (fid, us),
        Opened::Failed => return Ok(Some(Value::False)),
        Opened::Builtin(np) => {
            if np == p {
                return Ok(None);
            }
            return Ok(Some(call_orig(vm, "file", &with_path(args, 0, np))?));
        }
    };
    let mut out = new_arr();
    if let Some(buf) = copy_to_mem(vm, fid, &us, None)? {
        let e = buf.len();
        let mut lines: Vec<Vec<u8>> = Vec::new();
        let mut st = 0usize;
        if include_new_line {
            let mut i = 0;
            while i < e {
                if buf[i] == b'\n' {
                    lines.push(buf[st..=i].to_vec());
                    st = i + 1;
                }
                i += 1;
            }
        } else {
            let mut i = 0;
            while i < e {
                if buf[i] == b'\n' {
                    let windows_eol = if i > 0 && buf[i - 1] == b'\r' { 1 } else { 0 };
                    let len = i - st;
                    if !(skip_blank_lines && len - windows_eol.min(len) == 0) {
                        lines.push(buf[st..i - windows_eol.min(len)].to_vec());
                    }
                    st = i + 1;
                }
                i += 1;
            }
        }
        if st != e {
            lines.push(buf[st..e].to_vec());
        }
        for l in lines {
            out.push(s(&l));
        }
    }
    stream_free(vm, fid, &us)?;
    Ok(Some(arr_val(out)))
}

fn h_readfile(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    if !needs_locate(&p) {
        return Ok(None);
    }
    let use_path = args.get(1).map(|v| v.deref().is_truthy()).unwrap_or(false);
    let ctx = context_from_arg(vm, args.get(2), false)?;
    let options = if use_path { USE_PATH } else { 0 } | REPORT_ERRORS;
    let (fid, us) = match open_located(vm, &p, "rb", options, ctx)? {
        Opened::Stream(fid, us) => (fid, us),
        Opened::Failed => return Ok(Some(Value::False)),
        Opened::Builtin(np) => {
            if np == p {
                return Ok(None);
            }
            return Ok(Some(call_orig(vm, "readfile", &with_path(args, 0, np))?));
        }
    };
    let n = passthru(vm, fid, &us)?;
    stream_free(vm, fid, &us)?;
    Ok(Some(Value::Long(n)))
}

fn h_copy(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let src = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    let dest = match arg_str(args, 1) {
        Some(p) => p,
        None => return Ok(None),
    };
    let src_user = needs_locate(&src)
        && matches!(
            scheme_len(&src).map(|n| user_wrapper(&src[..n].to_ascii_lowercase())),
            Some(Some(_))
        );
    let dest_user = needs_locate(&dest)
        && matches!(
            scheme_len(&dest).map(|n| user_wrapper(&dest[..n].to_ascii_lowercase())),
            Some(Some(_))
        );
    if !src_user && !dest_user {
        return Ok(None);
    }
    let ctx = context_from_arg(vm, args.get(2), false)?;
    // php_copy_file_ctx()
    let mut safe = false;
    let src_s = stat_path_any(vm, &src, 0)?;
    match &src_s {
        None => safe = true,
        Some(sb) => {
            if sb.mode & 0o170000 == 0o040000 {
                warn(
                    vm,
                    "The first argument to copy() function cannot be a directory",
                );
                return Ok(Some(Value::False));
            }
        }
    }
    if !safe {
        let dest_s = stat_path_any(vm, &dest, PHP_STREAM_URL_STAT_QUIET)?;
        match &dest_s {
            None => safe = true,
            Some(db) => {
                if db.mode & 0o170000 == 0o040000 {
                    warn(
                        vm,
                        "The second argument to copy() function cannot be a directory",
                    );
                    return Ok(Some(Value::False));
                }
                let sb = src_s.unwrap();
                if sb.ino == 0 || db.ino == 0 {
                    if src == dest {
                        return Ok(Some(Value::False));
                    }
                } else if sb.ino == db.ino && sb.dev == db.dev {
                    return Ok(Some(Value::False));
                }
            }
        }
    }
    let _ = safe;
    let sfid = match any_open(vm, &src, "rb", REPORT_ERRORS, ctx)? {
        Some(f) => f,
        None => return Ok(Some(Value::False)),
    };
    let mut ok = false;
    if let Some(dfid) = any_open(vm, &dest, "wb", REPORT_ERRORS, ctx)? {
        ok = copy_stream(vm, sfid, dfid, None)?.0;
        any_close(vm, sfid)?;
        any_close(vm, dfid)?;
    } else {
        any_close(vm, sfid)?;
    }
    Ok(Some(b(ok)))
}

fn h_unlink(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (uw, p) = path_or_orig!(vm, "unlink", args, 0, 0);
    let ctx = context_from_arg(vm, args.get(1), false)?;
    Ok(Some(b(wrapper_bool_op(
        vm,
        &uw,
        ctx,
        "unlink",
        vec![s(&p)],
    )?)))
}

fn h_rename(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let from = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    let to = match arg_str(args, 1) {
        Some(p) => p,
        None => return Ok(None),
    };
    if !needs_locate(&from) && !needs_locate(&to) {
        return Ok(None);
    }
    let lf = locate(vm, &from, 0);
    let uw = match lf {
        Loc::None => {
            warn(vm, "Unable to locate stream wrapper");
            return Ok(Some(Value::False));
        }
        Loc::Builtin { .. } => {
            if needs_locate(&to) {
                if let Loc::User(_) = locate(vm, &to, 0) {
                    warn(vm, "Cannot rename a file across wrapper types");
                    return Ok(Some(Value::False));
                }
            }
            return Ok(None);
        }
        Loc::User(u) => u,
    };
    if uw.native {
        warn(vm, "ZLIB wrapper does not support renaming");
        return Ok(Some(Value::False));
    }
    let same = match locate(vm, &to, 0) {
        Loc::User(u2) => u2.scheme == uw.scheme,
        _ => false,
    };
    if !same {
        warn(vm, "Cannot rename a file across wrapper types");
        return Ok(Some(Value::False));
    }
    let ctx = context_from_arg(vm, args.get(2), false)?;
    Ok(Some(b(wrapper_bool_op(
        vm,
        &uw,
        ctx,
        "rename",
        vec![s(&from), s(&to)],
    )?)))
}

fn h_mkdir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (uw, p) = path_or_orig!(vm, "mkdir", args, 0, 0);
    let mode = int_arg(args, 1).unwrap_or(0o777);
    let recursive = args.get(2).map(|v| v.deref().is_truthy()).unwrap_or(false);
    let ctx = context_from_arg(vm, args.get(3), false)?;
    let options = if recursive { 1 } else { 0 } | REPORT_ERRORS;
    Ok(Some(b(wrapper_bool_op(
        vm,
        &uw,
        ctx,
        "mkdir",
        vec![s(&p), Value::Long(mode), Value::Long(options)],
    )?)))
}

fn h_rmdir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (uw, p) = path_or_orig!(vm, "rmdir", args, 0, 0);
    let ctx = context_from_arg(vm, args.get(1), false)?;
    Ok(Some(b(wrapper_bool_op(
        vm,
        &uw,
        ctx,
        "rmdir",
        vec![s(&p), Value::Long(REPORT_ERRORS)],
    )?)))
}

fn metadata(
    vm: &mut Vm,
    uw: &UserWrapperRef,
    p: &[u8],
    option: i64,
    value: Value,
) -> Result<Option<Value>, VmError> {
    if uw.native {
        // wops->stream_metadata is NULL
        let times_given = matches!(&value, Value::Array(a) if !a.borrow().is_empty());
        if option == PHP_STREAM_META_TOUCH && !times_given {
            return Ok(Some(match open_located(vm, p, "c", REPORT_ERRORS, None)? {
                Opened::Stream(fid, us) => {
                    stream_free(vm, fid, &us)?;
                    Value::True
                }
                _ => Value::False,
            }));
        }
        warn(
            vm,
            &format!("Cannot call {}() for a non-standard stream", cur_fn()),
        );
        return Ok(Some(Value::False));
    }
    Ok(Some(b(wrapper_bool_op(
        vm,
        uw,
        None,
        "stream_metadata",
        vec![s(p), Value::Long(option), value],
    )?)))
}

fn h_touch(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (uw, p) = path_or_orig!(vm, "touch", args, 0, 0);
    let mtime = int_arg(args, 1);
    let atime = int_arg(args, 2);
    let mut a = new_arr();
    match (mtime, atime) {
        (None, None) => {}
        (Some(m), None) => {
            a.set(ArrayKey::Int(0), Value::Long(m));
            a.set(ArrayKey::Int(1), Value::Long(m));
        }
        (None, Some(_)) => {
            return Err(throw(
                vm,
                b"ValueError",
                "touch(): Argument #2 ($mtime) cannot be null when argument #3 ($atime) is an integer",
            ));
        }
        (Some(m), Some(at)) => {
            a.set(ArrayKey::Int(0), Value::Long(m));
            a.set(ArrayKey::Int(1), Value::Long(at));
        }
    }
    metadata(vm, &uw, &p, PHP_STREAM_META_TOUCH, arr_val(a))
}

fn h_chmod(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (uw, p) = path_or_orig!(vm, "chmod", args, 0, 0);
    let mode = int_arg(args, 1).unwrap_or(0);
    metadata(vm, &uw, &p, PHP_STREAM_META_ACCESS, Value::Long(mode))
}

fn chown_like(
    vm: &mut Vm,
    args: &[Value],
    name: &'static str,
    argname: &str,
    opt_name: i64,
    opt_id: i64,
) -> Result<Option<Value>, VmError> {
    let (uw, p) = path_or_orig!(vm, name, args, 0, 0);
    match args.get(1).map(|v| v.deref()) {
        Some(Value::Long(n)) => metadata(vm, &uw, &p, opt_id, Value::Long(n)),
        Some(Value::String(sv)) => metadata(vm, &uw, &p, opt_name, Value::String(sv)),
        other => {
            let t = other
                .map(|v| zval_type_name(&v))
                .unwrap_or_else(|| "null".into());
            let msg = format!(
                "{}(): Argument #2 (${}) must be of type string|int, {} given",
                name, argname, t
            );
            Err(throw(vm, b"TypeError", &msg))
        }
    }
}

fn h_chown(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    chown_like(
        vm,
        args,
        "chown",
        "user",
        PHP_STREAM_META_OWNER_NAME,
        PHP_STREAM_META_OWNER,
    )
}
fn h_lchown(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    chown_like(
        vm,
        args,
        "lchown",
        "user",
        PHP_STREAM_META_OWNER_NAME,
        PHP_STREAM_META_OWNER,
    )
}
fn h_chgrp(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    chown_like(
        vm,
        args,
        "chgrp",
        "group",
        PHP_STREAM_META_GROUP_NAME,
        PHP_STREAM_META_GROUP,
    )
}
fn h_lchgrp(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    chown_like(
        vm,
        args,
        "lchgrp",
        "group",
        PHP_STREAM_META_GROUP_NAME,
        PHP_STREAM_META_GROUP,
    )
}

// --- stat family (php_stat) --------------------------------------------------

const FS_PERMS: i32 = 0;
const FS_INODE: i32 = 1;
const FS_SIZE: i32 = 2;
const FS_OWNER: i32 = 3;
const FS_GROUP: i32 = 4;
const FS_ATIME: i32 = 5;
const FS_MTIME: i32 = 6;
const FS_CTIME: i32 = 7;
const FS_TYPE: i32 = 8;
const FS_IS_W: i32 = 9;
const FS_IS_R: i32 = 10;
const FS_IS_X: i32 = 11;
const FS_IS_FILE: i32 = 12;
const FS_IS_DIR: i32 = 13;
const FS_IS_LINK: i32 = 14;
const FS_EXISTS: i32 = 15;
const FS_LSTAT: i32 = 16;
const FS_STAT: i32 = 17;

fn is_link_op(t: i32) -> bool {
    matches!(t, FS_TYPE | FS_IS_LINK | FS_LSTAT)
}

fn is_exists_check(t: i32) -> bool {
    matches!(
        t,
        FS_EXISTS | FS_IS_W | FS_IS_R | FS_IS_X | FS_IS_FILE | FS_IS_DIR | FS_IS_LINK
    )
}

fn php_stat(
    vm: &mut Vm,
    args: &[Value],
    name: &'static str,
    t: i32,
) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    if !needs_locate(&p) {
        // A plain stat replaces PHP's single-entry stat cache.
        clear_stat_cache();
        return Ok(None);
    }
    let uw = match user_path(vm, &p, 0) {
        Ok(u) => u,
        Err(Some(np)) => {
            clear_stat_cache();
            if np == p {
                return Ok(None);
            }
            return Ok(Some(call_orig(vm, name, &with_path(args, 0, np))?));
        }
        Err(None) => return Ok(Some(Value::False)),
    };
    let mut flags = 0;
    if is_link_op(t) {
        flags |= PHP_STREAM_URL_STAT_LINK;
    }
    if is_exists_check(t) {
        flags |= PHP_STREAM_URL_STAT_QUIET;
    }
    let cached = if flags & PHP_STREAM_URL_STAT_LINK != 0 {
        LSTAT_CACHE.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|(k, _)| *k == p)
                .map(|(_, sb)| *sb)
        })
    } else {
        STAT_CACHE.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|(k, _)| *k == p)
                .map(|(_, sb)| *sb)
        })
    };
    let sb = match cached {
        Some(sb) => sb,
        None => match wrapper_url_stat(vm, &uw, &p, flags | 4)? {
            None => {
                if !is_exists_check(t) {
                    warn(
                        vm,
                        &format!(
                            "{}stat failed for {}",
                            if is_link_op(t) { "L" } else { "" },
                            lossy(&p)
                        ),
                    );
                }
                return Ok(Some(Value::False));
            }
            Some(sb) => {
                if flags & PHP_STREAM_URL_STAT_LINK != 0 {
                    LSTAT_CACHE.with(|c| *c.borrow_mut() = Some((p.clone(), sb)));
                }
                if flags & PHP_STREAM_URL_STAT_LINK == 0 || sb.mode & 0o170000 != 0o120000 {
                    STAT_CACHE.with(|c| *c.borrow_mut() = Some((p.clone(), sb)));
                }
                sb
            }
        },
    };
    let (mut rmask, mut wmask, mut xmask) = (0o004i64, 0o002i64, 0o001i64);
    if (FS_IS_W..=FS_IS_X).contains(&t) {
        let uid = unsafe { libc::getuid() } as i64;
        let gid = unsafe { libc::getgid() } as i64;
        if sb.uid == uid {
            rmask = 0o400;
            wmask = 0o200;
            xmask = 0o100;
        } else if sb.gid == gid {
            rmask = 0o040;
            wmask = 0o020;
            xmask = 0o010;
        } else {
            let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
            if n > 0 {
                let mut gids = vec![0 as libc::gid_t; n as usize];
                let n2 = unsafe { libc::getgroups(n, gids.as_mut_ptr()) };
                if n2 > 0 && gids[..n2 as usize].iter().any(|&g| g as i64 == sb.gid) {
                    rmask = 0o040;
                    wmask = 0o020;
                    xmask = 0o010;
                }
            }
        }
    }
    let fmt = sb.mode & 0o170000;
    Ok(Some(match t {
        FS_PERMS => Value::Long(sb.mode),
        FS_INODE => Value::Long(sb.ino),
        FS_SIZE => Value::Long(sb.size),
        FS_OWNER => Value::Long(sb.uid),
        FS_GROUP => Value::Long(sb.gid),
        FS_ATIME => Value::Long(sb.atime),
        FS_MTIME => Value::Long(sb.mtime),
        FS_CTIME => Value::Long(sb.ctime),
        FS_TYPE => {
            let name: &[u8] = match fmt {
                0o120000 => b"link",
                0o010000 => b"fifo",
                0o020000 => b"char",
                0o040000 => b"dir",
                0o060000 => b"block",
                0o100000 => b"file",
                0o140000 => b"socket",
                _ => {
                    notice(vm, &format!("Unknown file type ({})", fmt));
                    b"unknown"
                }
            };
            s(name)
        }
        FS_IS_W => b(sb.mode & wmask != 0),
        FS_IS_R => b(sb.mode & rmask != 0),
        FS_IS_X => b(sb.mode & xmask != 0),
        FS_IS_FILE => b(fmt == 0o100000),
        FS_IS_DIR => b(fmt == 0o040000),
        FS_IS_LINK => b(fmt == 0o120000),
        FS_EXISTS => Value::True,
        _ => stat_to_array(&sb),
    }))
}

macro_rules! stat_fns {
    ($( $h:ident => ($name:literal, $t:expr) ),* $(,)?) => {
        $( fn $h(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> { php_stat(vm, args, $name, $t) } )*
    };
}

stat_fns! {
    h_fileperms => ("fileperms", FS_PERMS),
    h_fileinode => ("fileinode", FS_INODE),
    h_filesize => ("filesize", FS_SIZE),
    h_fileowner => ("fileowner", FS_OWNER),
    h_filegroup => ("filegroup", FS_GROUP),
    h_fileatime => ("fileatime", FS_ATIME),
    h_filemtime => ("filemtime", FS_MTIME),
    h_filectime => ("filectime", FS_CTIME),
    h_filetype => ("filetype", FS_TYPE),
    h_is_writable => ("is_writable", FS_IS_W),
    h_is_writeable => ("is_writeable", FS_IS_W),
    h_is_readable => ("is_readable", FS_IS_R),
    h_is_executable => ("is_executable", FS_IS_X),
    h_is_file => ("is_file", FS_IS_FILE),
    h_is_dir => ("is_dir", FS_IS_DIR),
    h_is_link => ("is_link", FS_IS_LINK),
    h_file_exists => ("file_exists", FS_EXISTS),
    h_lstat => ("lstat", FS_LSTAT),
    h_stat => ("stat", FS_STAT),
}

fn h_clearstatcache(_vm: &mut Vm, _args: &[Value]) -> Result<Option<Value>, VmError> {
    clear_stat_cache();
    Ok(None)
}

// --- directories ---------------------------------------------------------------

fn do_opendir(
    vm: &mut Vm,
    args: &[Value],
    name: &'static str,
    createobject: bool,
) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    if !needs_locate(&p) {
        return Ok(None);
    }
    let uw_ok = match user_path(vm, &p, REPORT_ERRORS) {
        Ok(_) => true,
        Err(Some(np)) => {
            if np == p {
                return Ok(None);
            }
            return Ok(Some(call_orig(vm, name, &with_path(args, 0, np))?));
        }
        Err(None) => false,
    };
    let ctx = context_from_arg(vm, args.get(1), false)?;
    if !uw_ok {
        display_wrapper_errors(vm, false, &p, "Failed to open directory");
        return Ok(Some(Value::False));
    }
    let (fid, us) = match opendir_wrapper_located(vm, &p, REPORT_ERRORS, ctx)? {
        Some(x) => x,
        None => return Ok(Some(Value::False)),
    };
    us.borrow_mut().flags |= FLAG_NO_FCLOSE;
    CURRENT_DIR_RESOURCE.with(|c| c.set(Some(fid)));
    if createobject {
        let id = vm.alloc_object_id();
        let mut obj = PhpObject::new(b"Directory".to_vec(), id);
        obj.set_property(b"path".to_vec(), s(&p));
        obj.set_property(b"handle".to_vec(), Value::Resource(fid));
        Ok(Some(Value::Object(Rc::new(RefCell::new(obj)))))
    } else {
        Ok(Some(Value::Resource(fid)))
    }
}

/// opendir_wrapper() without re-running locate()'s diagnostics.
fn opendir_wrapper_located(
    vm: &mut Vm,
    path: &[u8],
    options: i64,
    context: Option<i64>,
) -> Result<Option<(i64, Us)>, VmError> {
    let uw = match scheme_len(path)
        .and_then(|n| user_wrapper(&path[..n].to_ascii_lowercase()))
        .or_else(|| user_wrapper(b"file"))
    {
        Some(u) => u,
        None => return opendir_wrapper(vm, path, options, context),
    };
    let stream = wrapper_opendir(vm, &uw, path, options & !REPORT_ERRORS, context)?;
    if stream.is_none() && options & REPORT_ERRORS != 0 {
        display_wrapper_errors(vm, true, path, "Failed to open directory");
    }
    tidy_wrapper_errors();
    Ok(stream)
}

fn h_opendir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    do_opendir(vm, args, "opendir", false)
}

fn h_dir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    do_opendir(vm, args, "dir", true)
}

fn h_scandir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let p = match arg_str(args, 0) {
        Some(p) => p,
        None => return Ok(None),
    };
    if !needs_locate(&p) {
        return Ok(None);
    }
    match user_path(vm, &p, REPORT_ERRORS) {
        Ok(_) => {}
        Err(Some(np)) => {
            if np == p {
                return Ok(None);
            }
            return Ok(Some(call_orig(vm, "scandir", &with_path(args, 0, np))?));
        }
        Err(None) => {
            display_wrapper_errors(vm, false, &p, "Failed to open directory");
            warn(vm, "(errno 2): No such file or directory");
            return Ok(Some(Value::False));
        }
    }
    let order = int_arg(args, 1).unwrap_or(0);
    // scandir() only uses a context when one is passed.
    let ctx = if args
        .get(2)
        .map(|v| !matches!(v.deref(), Value::Null | Value::Undef))
        .unwrap_or(false)
    {
        context_from_arg(vm, args.get(2), false)?
    } else {
        None
    };
    let (fid, us) = match opendir_wrapper_located(vm, &p, REPORT_ERRORS, ctx)? {
        Some(x) => x,
        None => {
            warn(vm, "(errno 2): No such file or directory");
            return Ok(Some(Value::False));
        }
    };
    let mut names: Vec<Vec<u8>> = Vec::new();
    while let Some(n) = op_readdir(vm, &us)? {
        names.push(n);
        if vm.current_exception.is_some() {
            break;
        }
    }
    stream_free(vm, fid, &us)?;
    match order {
        0 => names.sort(),
        1 => {
            names.sort();
            names.reverse();
        }
        _ => {}
    }
    let mut a = new_arr();
    for n in names {
        a.push(s(&n));
    }
    Ok(Some(arr_val(a)))
}

/// The user dir stream named by a readdir()/rewinddir()/closedir() argument.
fn user_dir_arg(vm: &mut Vm, args: &[Value], name: &str) -> Option<(i64, Us)> {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let fid = match &v {
        Value::Null | Value::Undef => {
            let cur = CURRENT_DIR_RESOURCE.with(|c| c.get())?;
            get_us(cur)?;
            vm.emit_deprecated_at(
                &format!("{}(): Passing null is deprecated, instead the last opened directory stream should be provided", name),
                vm.current_line,
            );
            cur
        }
        Value::Resource(n) => *n,
        Value::Object(_) => dir_handle_resource_id(&v)?,
        _ => return None,
    };
    let us = get_us(fid)?;
    if us.borrow().flags & FLAG_IS_DIR == 0 {
        return None;
    }
    Some((fid, us))
}

fn h_readdir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match user_dir_arg(vm, args, "readdir") {
        Some(x) => x,
        None => return Ok(None),
    };
    Ok(Some(match op_readdir(vm, &us)? {
        Some(n) => s(&n),
        None => Value::False,
    }))
}

fn h_rewinddir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (_, us) = match user_dir_arg(vm, args, "rewinddir") {
        Some(x) => x,
        None => return Ok(None),
    };
    op_rewinddir(vm, &us)?;
    let mut bm = us.borrow_mut();
    bm.eof = false;
    bm.position = 0;
    Ok(Some(Value::Null))
}

fn h_closedir(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let (fid, us) = match user_dir_arg(vm, args, "closedir") {
        Some(x) => x,
        None => return Ok(None),
    };
    stream_free(vm, fid, &us)?;
    Ok(Some(Value::Null))
}

// --- wrapper registry ------------------------------------------------------------

fn valid_scheme(p: &[u8]) -> bool {
    !p.is_empty()
        && p.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
}

fn h_stream_wrapper_register(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let protocol = args
        .first()
        .map(|v| v.deref().to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let class = args
        .get(1)
        .map(|v| v.deref().to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let flags = int_arg(args, 2).unwrap_or(0);
    let cl: Vec<u8> = class
        .strip_prefix(b"\\")
        .unwrap_or(&class)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if !vm.classes.contains_key(&cl) {
        vm.try_autoload_class(&class);
    }
    let canonical = match vm.classes.get(&cl) {
        Some(c) => c.name.clone(),
        None => {
            let msg = format!(
                "{}(): Argument #2 ($class) must be a valid class name, {} given",
                cur_fn(),
                lossy(&class)
            );
            return Err(throw(vm, b"TypeError", &msg));
        }
    };
    let lower: Vec<u8> = protocol.iter().map(|b| b.to_ascii_lowercase()).collect();
    let exists = (is_builtin(&lower) && !builtin_disabled(&lower))
        || USER_WRAPPERS.with(|w| w.borrow().contains_key(&lower));
    if !valid_scheme(&protocol) || exists {
        if exists {
            warn(
                vm,
                &format!("Protocol {}:// is already defined.", lossy(&protocol)),
            );
        } else {
            warn(
                vm,
                &format!(
                    "Invalid protocol scheme specified. Unable to register wrapper class {} to {}://",
                    lossy(&canonical),
                    lossy(&protocol)
                ),
            );
        }
        return Ok(Some(Value::False));
    }
    USER_WRAPPERS.with(|w| {
        w.borrow_mut().insert(
            lower,
            UserWrapper {
                class_name: canonical,
                flags,
            },
        );
    });
    Ok(Some(Value::True))
}

fn h_stream_wrapper_unregister(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let protocol = args
        .first()
        .map(|v| v.deref().to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let lower: Vec<u8> = protocol.iter().map(|b| b.to_ascii_lowercase()).collect();
    if USER_WRAPPERS.with(|w| w.borrow_mut().remove(&lower).is_some()) {
        return Ok(Some(Value::True));
    }
    if is_builtin(&lower) && !builtin_disabled(&lower) {
        DISABLED_BUILTINS.with(|d| d.borrow_mut().push(lower));
        return Ok(Some(Value::True));
    }
    warn(
        vm,
        &format!("Unable to unregister protocol {}://", lossy(&protocol)),
    );
    Ok(Some(Value::False))
}

fn h_stream_wrapper_restore(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let protocol = args
        .first()
        .map(|v| v.deref().to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let lower: Vec<u8> = protocol.iter().map(|b| b.to_ascii_lowercase()).collect();
    if !is_builtin(&lower) {
        warn(
            vm,
            &format!("{}:// never existed, nothing to restore", lossy(&protocol)),
        );
        return Ok(Some(Value::False));
    }
    let overridden = USER_WRAPPERS.with(|w| w.borrow().contains_key(&lower));
    if !builtin_disabled(&lower) && !overridden {
        notice(
            vm,
            &format!(
                "{}:// was never changed, nothing to restore",
                lossy(&protocol)
            ),
        );
        return Ok(Some(Value::True));
    }
    USER_WRAPPERS.with(|w| {
        w.borrow_mut().remove(&lower);
    });
    DISABLED_BUILTINS.with(|d| d.borrow_mut().retain(|x| *x != lower));
    Ok(Some(Value::True))
}

fn h_stream_get_wrappers(vm: &mut Vm, args: &[Value]) -> Result<Option<Value>, VmError> {
    let orig = call_orig(vm, "stream_get_wrappers", args)?;
    let mut a = new_arr();
    if let Value::Array(arr) = orig {
        for v in arr.borrow().values() {
            let name = v.to_php_string().as_bytes().to_ascii_lowercase();
            if builtin_disabled(&name) {
                continue;
            }
            a.push(v.clone());
        }
    }
    Ok(Some(arr_val(a)))
}

// ---------------------------------------------------------------------------
// include / require
// ---------------------------------------------------------------------------

/// VM include hook: returns None for paths not served by a user wrapper.
/// Some(Ok(source)) with the file contents, Some(Err(..)) when opening
/// failed (warnings / the require Error have been raised).
pub(crate) fn include_hook(vm: &mut Vm, path: &str, kind: u8) -> Option<Result<Vec<u8>, VmError>> {
    let p = path.as_bytes();
    if !needs_locate(p) {
        return None;
    }
    let n = scheme_len(p)?;
    user_wrapper(&p[..n].to_ascii_lowercase())?;
    let name: &'static str = match kind {
        1 => "include_once",
        2 => "require",
        3 => "require_once",
        _ => "include",
    };
    FN_STACK.with(|st| st.borrow_mut().push(name));
    // Functions called from the INCLUDE_OR_EVAL opline (the wrapper's
    // methods, the error handler) are top-level calls whose backtraces show
    // an include/require frame.
    let frame = (
        name.to_string(),
        vm.current_file.clone(),
        vm.current_line,
        Vec::new(),
        false,
    );
    vm.call_stack.push(frame.clone());
    let r = include_open(vm, p);
    vm.call_stack.pop();
    let res = match r {
        Ok(Some(src)) => Some(Ok(src)),
        Ok(None) => {
            if vm.current_exception.is_some() {
                Some(Err(exc_err(vm)))
            } else {
                let inc = include_path_value(vm);
                if kind >= 2 {
                    let msg = format!(
                        "Failed opening required '{}' (include_path='{}')",
                        path, inc
                    );
                    Some(Err(throw(vm, b"Error", &msg)))
                } else {
                    vm.call_stack.push(frame);
                    vm.emit_warning(&format!(
                        "{}(): Failed opening '{}' for inclusion (include_path='{}')",
                        name, path, inc
                    ));
                    vm.call_stack.pop();
                    Some(Err(VmError {
                        message: String::new(),
                        line: vm.current_line,
                    }))
                }
            }
        }
        Err(e) => Some(Err(e)),
    };
    FN_STACK.with(|st| {
        st.borrow_mut().pop();
    });
    res
}

fn include_path_value(vm: &Vm) -> String {
    vm.constants
        .get(b"include_path".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| ".".into())
}

/// php_resolve_path(), for the parts that involve userspace wrappers:
/// Some(Ok(url)) when `filename` resolves to a user-wrapper URL (either
/// given verbatim or found through an include_path wrapper entry).
fn resolve_path_user(vm: &mut Vm, filename: &[u8]) -> Option<Result<Vec<u8>, VmError>> {
    if let Some(n) = scheme_len(filename) {
        let lower = filename[..n].to_ascii_lowercase();
        if user_wrapper(&lower).is_some() {
            return Some(Ok(filename.to_vec()));
        }
        return None;
    }
    if filename.is_empty()
        || filename[0] == b'/'
        || filename.starts_with(b"./")
        || filename.starts_with(b"../")
    {
        return None;
    }
    let inc = include_path_value(vm).into_bytes();
    if !inc.windows(3).any(|w| w == b"://") {
        return None;
    }
    let mut ptr = 0usize;
    while ptr < inc.len() {
        let rest = &inc[ptr..];
        let mut p = 0;
        while p < rest.len()
            && (rest[p].is_ascii_alphanumeric() || matches!(rest[p], b'+' | b'-' | b'.'))
        {
            p += 1;
        }
        let mut is_stream_wrapper = false;
        let mut search_from = p;
        if p < rest.len()
            && rest[p] == b':'
            && p > 1
            && rest[p + 1..].starts_with(b"//")
            && !(p == 2 && &rest[..2] == b"..")
        {
            is_stream_wrapper = true;
            search_from = p + 3;
        }
        let end = rest[search_from.min(rest.len())..]
            .iter()
            .position(|&c| c == b':')
            .map(|e| e + search_from);
        let entry = match end {
            Some(e) => &rest[..e],
            None => rest,
        };
        let mut trypath = entry.to_vec();
        trypath.push(b'/');
        trypath.extend_from_slice(filename);
        ptr = match end {
            Some(e) => ptr + e + 1,
            None => inc.len(),
        };
        if is_stream_wrapper {
            let n = scheme_len(&trypath);
            let uw = n.and_then(|n| user_wrapper(&trypath[..n].to_ascii_lowercase()));
            if let Some(uw) = uw {
                match wrapper_url_stat(vm, &uw, &trypath, PHP_STREAM_URL_STAT_QUIET) {
                    Ok(Some(_)) => return Some(Ok(trypath)),
                    Ok(None) => {
                        if vm.current_exception.is_some() {
                            return Some(Err(exc_err(vm)));
                        }
                    }
                    Err(e) => return Some(Err(e)),
                }
            }
            continue;
        }
        if std::path::Path::new(&lossy(&trypath)).exists() {
            return None;
        }
    }
    None
}

/// VM include resolver hook.
pub(crate) fn include_resolve(vm: &mut Vm, path: &str) -> Option<Result<String, VmError>> {
    let p = path.as_bytes();
    if USER_WRAPPERS.with(|w| w.borrow().is_empty()) {
        // compress.zlib:// URLs are served by this layer too.
        let zlib = scheme_len(p).is_some_and(|n| {
            p[..n].eq_ignore_ascii_case(b"compress.zlib")
                && user_wrapper(&p[..n].to_ascii_lowercase()).is_some()
        });
        return if zlib {
            Some(Ok(path.to_string()))
        } else {
            None
        };
    }
    FN_STACK.with(|st| st.borrow_mut().push("include"));
    let r = resolve_path_user(vm, p);
    FN_STACK.with(|st| {
        st.borrow_mut().pop();
    });
    r.map(|r| r.map(|v| lossy(&v)))
}

/// php_stream_open_for_zend_ex() + zend_stream_fixup() + close.
fn include_open(vm: &mut Vm, p: &[u8]) -> Result<Option<Vec<u8>>, VmError> {
    let options = USE_PATH | REPORT_ERRORS | STREAM_OPEN_FOR_INCLUDE | STREAM_OPEN_FOR_ZEND_STREAM;
    let (fid, us) = match open_located(vm, p, "rb", options, None)? {
        Opened::Stream(fid, us) => (fid, us),
        _ => return Ok(None),
    };
    // Disable buffering to avoid double buffering between PHP and Zend streams.
    stream_set_option(vm, &us, PHP_STREAM_OPTION_READ_BUFFER, 0, None)?;
    let file_size = if has_read_filters(fid) {
        0
    } else {
        op_stat(vm, &us)?
            .map(|sb| sb.size.max(0) as usize)
            .unwrap_or(0)
    };
    let mut buf = Vec::new();
    let mut failed = false;
    if file_size > 0 {
        loop {
            if buf.len() >= file_size {
                break;
            }
            match stream_read(vm, fid, &us, file_size - buf.len())? {
                Some(d) if !d.is_empty() => buf.extend_from_slice(&d),
                Some(_) => break,
                None => {
                    failed = true;
                    break;
                }
            }
        }
        // drain until read returns 0, like the C loop
        if !failed && buf.len() >= file_size {
            // the C loop stops as soon as the requested size is satisfied
        }
    } else {
        loop {
            match stream_read(vm, fid, &us, 4096)? {
                Some(d) if !d.is_empty() => buf.extend_from_slice(&d),
                Some(_) => break,
                None => {
                    failed = true;
                    break;
                }
            }
            if vm.current_exception.is_some() {
                break;
            }
        }
    }
    stream_free(vm, fid, &us)?;
    if failed || vm.current_exception.is_some() {
        return Ok(None);
    }
    Ok(Some(buf))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// php_module_startup(): "Check for deprecated directives".
fn check_deprecated_directives(vm: &mut Vm) {
    if ini_bool(vm, b"allow_url_include", false) && vm.error_reporting & 8192 != 0 {
        vm.display_error(
            "Deprecated",
            "Directive 'allow_url_include' is deprecated",
            "Unknown",
            0,
        );
    }
}

macro_rules! intercepts {
    ($( $w:ident => ($name:literal, $h:ident) ),* $(,)?) => {
        $(
            fn $w(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                run(vm, $name, args, $h)
            }
        )*
        pub(crate) fn register(vm: &mut Vm) {
            $(
                let lower: Vec<u8> = $name.as_bytes().to_vec();
                if let Some(orig) = vm.functions.get(&lower).copied() {
                    ORIG.with(|m| m.borrow_mut().insert($name, orig));
                }
                vm.register_function($name.as_bytes(), $w);
            )*
            vm.include_stream_hook = Some(include_hook);
            vm.include_resolve_hook = Some(include_resolve);
            vm.resource_shutdown_hooks.push(shutdown_streams);
            vm.startup_hooks.push(check_deprecated_directives);
        }
    };
}

intercepts! {
    w_fread => ("fread", h_fread),
    w_fwrite => ("fwrite", h_fwrite),
    w_fputs => ("fputs", h_fwrite),
    w_fgets => ("fgets", h_fgets),
    w_fgetc => ("fgetc", h_fgetc),
    w_feof => ("feof", h_feof),
    w_fclose => ("fclose", h_fclose),
    w_fflush => ("fflush", h_fflush),
    w_fseek => ("fseek", h_fseek),
    w_ftell => ("ftell", h_ftell),
    w_rewind => ("rewind", h_rewind),
    w_fstat => ("fstat", h_fstat),
    w_ftruncate => ("ftruncate", h_ftruncate),
    w_flock => ("flock", h_flock),
    w_stream_supports_lock => ("stream_supports_lock", h_stream_supports_lock),
    w_stream_get_contents => ("stream_get_contents", h_stream_get_contents),
    w_stream_get_line => ("stream_get_line", h_stream_get_line),
    w_fpassthru => ("fpassthru", h_fpassthru),
    w_stream_get_meta_data => ("stream_get_meta_data", h_stream_get_meta_data),
    w_stream_set_blocking => ("stream_set_blocking", h_stream_set_blocking),
    w_socket_set_blocking => ("socket_set_blocking", h_stream_set_blocking),
    w_stream_set_timeout => ("stream_set_timeout", h_stream_set_timeout),
    w_socket_set_timeout => ("socket_set_timeout", h_stream_set_timeout),
    w_stream_set_write_buffer => ("stream_set_write_buffer", h_stream_set_write_buffer),
    w_set_file_buffer => ("set_file_buffer", h_stream_set_write_buffer),
    w_stream_set_read_buffer => ("stream_set_read_buffer", h_stream_set_read_buffer),
    w_stream_set_chunk_size => ("stream_set_chunk_size", h_stream_set_chunk_size),
    w_fsync => ("fsync", h_fsync),
    w_fdatasync => ("fdatasync", h_fsync),
    w_stream_isatty => ("stream_isatty", h_stream_isatty),
    w_stream_copy_to_stream => ("stream_copy_to_stream", h_stream_copy_to_stream),
    w_fprintf => ("fprintf", h_fprintf),
    w_vfprintf => ("vfprintf", h_vfprintf),
    w_stream_select => ("stream_select", h_stream_select),
    w_fgetcsv => ("fgetcsv", h_fgetcsv),
    w_fputcsv => ("fputcsv", h_fputcsv),
    w_fscanf => ("fscanf", h_fscanf),
    w_stream_is_local => ("stream_is_local", h_stream_is_local),
    w_stream_resolve_include_path => ("stream_resolve_include_path", h_stream_resolve_include_path),
    w_fopen => ("fopen", h_fopen),
    w_file_get_contents => ("file_get_contents", h_file_get_contents),
    w_file_put_contents => ("file_put_contents", h_file_put_contents),
    w_file => ("file", h_file),
    w_readfile => ("readfile", h_readfile),
    w_copy => ("copy", h_copy),
    w_unlink => ("unlink", h_unlink),
    w_rename => ("rename", h_rename),
    w_mkdir => ("mkdir", h_mkdir),
    w_rmdir => ("rmdir", h_rmdir),
    w_touch => ("touch", h_touch),
    w_chmod => ("chmod", h_chmod),
    w_chown => ("chown", h_chown),
    w_lchown => ("lchown", h_lchown),
    w_chgrp => ("chgrp", h_chgrp),
    w_lchgrp => ("lchgrp", h_lchgrp),
    w_fileperms => ("fileperms", h_fileperms),
    w_fileinode => ("fileinode", h_fileinode),
    w_filesize => ("filesize", h_filesize),
    w_fileowner => ("fileowner", h_fileowner),
    w_filegroup => ("filegroup", h_filegroup),
    w_fileatime => ("fileatime", h_fileatime),
    w_filemtime => ("filemtime", h_filemtime),
    w_filectime => ("filectime", h_filectime),
    w_filetype => ("filetype", h_filetype),
    w_is_writable => ("is_writable", h_is_writable),
    w_is_writeable => ("is_writeable", h_is_writeable),
    w_is_readable => ("is_readable", h_is_readable),
    w_is_executable => ("is_executable", h_is_executable),
    w_is_file => ("is_file", h_is_file),
    w_is_dir => ("is_dir", h_is_dir),
    w_is_link => ("is_link", h_is_link),
    w_file_exists => ("file_exists", h_file_exists),
    w_lstat => ("lstat", h_lstat),
    w_stat => ("stat", h_stat),
    w_clearstatcache => ("clearstatcache", h_clearstatcache),
    w_opendir => ("opendir", h_opendir),
    w_dir => ("dir", h_dir),
    w_scandir => ("scandir", h_scandir),
    w_readdir => ("readdir", h_readdir),
    w_rewinddir => ("rewinddir", h_rewinddir),
    w_closedir => ("closedir", h_closedir),
    w_stream_wrapper_register => ("stream_wrapper_register", h_stream_wrapper_register),
    w_stream_register_wrapper => ("stream_register_wrapper", h_stream_wrapper_register),
    w_stream_wrapper_unregister => ("stream_wrapper_unregister", h_stream_wrapper_unregister),
    w_stream_wrapper_restore => ("stream_wrapper_restore", h_stream_wrapper_restore),
    w_stream_get_wrappers => ("stream_get_wrappers", h_stream_get_wrappers),
}

/// compress.zlib:// streams and gzopen() (ext/zlib/zlib_fopen_wrapper.c).
#[path = "zlib_stream.rs"]
pub(crate) mod zlib_stream;

macro_rules! zlib_intercepts {
    ($( $w:ident => ($name:literal, $orig:literal, $h:path) ),* $(,)?) => {
        $(
            fn $w(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                run(vm, $name, args, $h)
            }
        )*
        /// ext/zlib's stream functions: gzopen() & co. and the gz* aliases
        /// of the stream functions (zif_fread & co. in zlib_arginfo.h).
        pub(crate) fn register_zlib(vm: &mut Vm) {
            $(
                if !$orig.is_empty() {
                    if let Some(orig) = ORIG.with(|m| m.borrow().get($orig).copied()) {
                        ORIG.with(|m| m.borrow_mut().insert($name, orig));
                    }
                }
                vm.register_function($name.as_bytes(), $w);
            )*
        }
    };
}

zlib_intercepts! {
    w_gzwrite => ("gzwrite", "fwrite", h_fwrite),
    w_gzputs => ("gzputs", "fwrite", h_fwrite),
    w_gzrewind => ("gzrewind", "rewind", h_rewind),
    w_gzclose => ("gzclose", "fclose", h_fclose),
    w_gzeof => ("gzeof", "feof", h_feof),
    w_gzgetc => ("gzgetc", "fgetc", h_fgetc),
    w_gzpassthru => ("gzpassthru", "fpassthru", h_fpassthru),
    w_gzseek => ("gzseek", "fseek", h_fseek),
    w_gztell => ("gztell", "ftell", h_ftell),
    w_gzread => ("gzread", "fread", h_fread),
    w_gzgets => ("gzgets", "fgets", h_fgets),
    w_gzopen => ("gzopen", "", zlib_stream::h_gzopen),
    w_gzfile => ("gzfile", "", zlib_stream::h_gzfile),
    w_readgzfile => ("readgzfile", "", zlib_stream::h_readgzfile),
}
