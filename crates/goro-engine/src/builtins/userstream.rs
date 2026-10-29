//! User stream wrappers (PHP manual: stream_wrapper_register, "The
//! streamWrapper class"): a PHP class implementing stream_open,
//! stream_read, stream_write, url_stat, dir_opendir... handles the URLs of
//! a registered protocol. Call order and messages from php85 output and
//! the ext/standard stream tests.

use goro_runtime::{ObjRef, PhpArray, PhpRef, PhpStr, ResRef, Value};

use super::file::{Backing, Stream, new_stream_res};
use crate::class::DisplayName;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function stream_wrapper_register(string $protocol, string $class, int $flags = 0): bool",
            register,
        ),
        (
            "function stream_register_wrapper(string $protocol, string $class, int $flags = 0): bool",
            register,
        ),
        (
            "function stream_wrapper_unregister(string $protocol): bool",
            unregister,
        ),
        (
            "function stream_wrapper_restore(string $protocol): bool",
            restore,
        ),
    ],
    constants: &[
        ("STREAM_USE_PATH", crate::registry::ConstVal::Int(1)),
        ("STREAM_REPORT_ERRORS", crate::registry::ConstVal::Int(8)),
        ("STREAM_MUST_SEEK", crate::registry::ConstVal::Int(16)),
        ("STREAM_URL_STAT_LINK", crate::registry::ConstVal::Int(1)),
        ("STREAM_URL_STAT_QUIET", crate::registry::ConstVal::Int(2)),
        ("STREAM_MKDIR_RECURSIVE", crate::registry::ConstVal::Int(1)),
        ("STREAM_IS_URL", crate::registry::ConstVal::Int(1)),
        ("STREAM_OPTION_BLOCKING", crate::registry::ConstVal::Int(1)),
        (
            "STREAM_OPTION_READ_TIMEOUT",
            crate::registry::ConstVal::Int(4),
        ),
        (
            "STREAM_OPTION_READ_BUFFER",
            crate::registry::ConstVal::Int(2),
        ),
        (
            "STREAM_OPTION_WRITE_BUFFER",
            crate::registry::ConstVal::Int(3),
        ),
        ("STREAM_BUFFER_NONE", crate::registry::ConstVal::Int(0)),
        ("STREAM_BUFFER_LINE", crate::registry::ConstVal::Int(1)),
        ("STREAM_BUFFER_FULL", crate::registry::ConstVal::Int(2)),
        ("STREAM_CAST_AS_STREAM", crate::registry::ConstVal::Int(0)),
        ("STREAM_CAST_FOR_SELECT", crate::registry::ConstVal::Int(3)),
        ("STREAM_META_TOUCH", crate::registry::ConstVal::Int(1)),
        ("STREAM_META_OWNER", crate::registry::ConstVal::Int(3)),
        ("STREAM_META_OWNER_NAME", crate::registry::ConstVal::Int(2)),
        ("STREAM_META_GROUP", crate::registry::ConstVal::Int(5)),
        ("STREAM_META_GROUP_NAME", crate::registry::ConstVal::Int(4)),
        ("STREAM_META_ACCESS", crate::registry::ConstVal::Int(6)),
    ],
    listed: false,
    ..Module::EMPTY
};

const BUILTIN: &[&[u8]] = &[
    b"php",
    b"file",
    b"glob",
    b"data",
    b"http",
    b"ftp",
    b"compress.zlib",
    b"phar",
];

fn valid_protocol(p: &[u8]) -> bool {
    !p.is_empty()
        && p.iter()
            .all(|&c| c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.')
}

fn register(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let proto = a.str(0);
    let class = a.str(1);
    let lp = proto.to_ascii_lowercase();
    if !valid_protocol(&proto) {
        vm.fwarn(
            a,
            &format!(
                "Invalid protocol scheme specified. Unable to register wrapper class {} to {}://",
                class.to_string_lossy(),
                proto.to_string_lossy()
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let Some(cls) = vm.find_class(&class, true)? else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #2 ($class) must be a valid class name, {} given",
                class.to_string_lossy()
            ),
        );
    };
    let builtin_active = BUILTIN.contains(&&lp[..]) && !vm.disabled_wrappers.contains(&lp);
    if vm.user_wrappers.contains_key(&lp) || builtin_active {
        vm.fwarn(
            a,
            &format!("Protocol {}:// is already defined", proto.to_string_lossy()),
        )?;
        return Ok(Value::Bool(false));
    }
    URL_WRAPPERS.with(|u| {
        let mut u = u.borrow_mut();
        u.retain(|x| *x != lp);
        // STREAM_IS_URL
        if a.int_or(2, 0) & 1 != 0 {
            u.push(lp.clone());
        }
    });
    vm.user_wrappers.insert(lp, cls.name.clone());
    Ok(Value::Bool(true))
}

fn is_url_wrapper(path: &[u8]) -> bool {
    let sch = scheme(path).unwrap_or_default();
    URL_WRAPPERS.with(|u| u.borrow().contains(&sch))
}

fn url_disabled(vm: &mut Vm, fname: &str, path: &[u8], setting: &str) -> R<()> {
    let p = path.windows(3).position(|w| w == b"://").unwrap_or(0);
    vm.warn(&format!(
        "{fname}(): {}:// wrapper is disabled in the server configuration by {setting}=0",
        String::from_utf8_lossy(&path[..p])
    ))?;
    vm.warn(&format!(
        "{fname}({}): Failed to open stream: no suitable wrapper could be found",
        String::from_utf8_lossy(path)
    ))
}

thread_local! {
    /// Includes reading through user wrappers.
    static INCLUDING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// User wrappers registered with STREAM_IS_URL.
    static URL_WRAPPERS: std::cell::RefCell<Vec<Vec<u8>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn unregister(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let proto = a.str(0);
    let lp = proto.to_ascii_lowercase();
    if vm.user_wrappers.remove(&lp).is_some() {
        return Ok(Value::Bool(true));
    }
    if BUILTIN.contains(&&lp[..]) && !vm.disabled_wrappers.contains(&lp) {
        vm.disabled_wrappers.push(lp);
        return Ok(Value::Bool(true));
    }
    vm.fwarn(
        a,
        &format!(
            "Unable to unregister protocol {}://",
            proto.to_string_lossy()
        ),
    )?;
    Ok(Value::Bool(false))
}

fn restore(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let proto = a.str(0);
    let lp = proto.to_ascii_lowercase();
    if !BUILTIN.contains(&&lp[..]) {
        vm.fwarn(
            a,
            &format!(
                "{}:// never existed, nothing to restore",
                proto.to_string_lossy()
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let was_disabled = vm.disabled_wrappers.contains(&lp);
    let was_user = vm.user_wrappers.remove(&lp).is_some();
    vm.disabled_wrappers.retain(|p| *p != lp);
    if !was_disabled && !was_user {
        vm.fnotice(
            a,
            &format!(
                "{}:// was never changed, nothing to restore",
                proto.to_string_lossy()
            ),
        )?;
    }
    Ok(Value::Bool(true))
}

/// The scheme of a URL (`scheme://...`), lowercased.
pub(crate) fn scheme(path: &[u8]) -> Option<Vec<u8>> {
    let p = path.windows(3).position(|w| w == b"://")?;
    let s = &path[..p];
    valid_protocol(s).then(|| s.to_ascii_lowercase())
}

/// The wrapper class for a URL, if its protocol is a user wrapper.
pub fn wrapper_for(vm: &Vm, path: &[u8]) -> Option<PhpStr> {
    if vm.user_wrappers.is_empty() {
        return None;
    }
    let s = scheme(path)?;
    vm.user_wrappers.get(&s).cloned()
}

/// A disabled built-in wrapper (stream_wrapper_unregister("file")...).
pub fn disabled(vm: &Vm, path: &[u8]) -> bool {
    if vm.disabled_wrappers.is_empty() {
        return false;
    }
    let s = scheme(path).unwrap_or_else(|| b"file".to_vec());
    vm.disabled_wrappers.contains(&s)
}

thread_local! {
    /// The context of the stream function being run, for wrappers opened
    /// below it (php://filter/resource=...).
    static OPEN_CONTEXT: std::cell::RefCell<Option<Value>> =
        const { std::cell::RefCell::new(None) };
}

/// The context argument of the calling stream function, or the default
/// context: wrapper instances see it as `$this->context` (observed on PHP
/// 8.5; test ext/standard/tests/streams/gh15155).
fn call_context(a: &Args) -> Value {
    let name = a.fname().to_ascii_lowercase();
    let idx = match name.as_str() {
        "fopen" | "mkdir" | "file_put_contents" => Some(3),
        "file_get_contents" | "file" | "readfile" | "rename" | "copy" | "get_headers"
        | "scandir" => Some(2),
        "unlink" | "rmdir" | "opendir" | "dir" => Some(1),
        _ => None,
    };
    if let Some(i) = idx {
        let v = a.val(i);
        if super::sockets::with_context(&v, |_| ()).is_some() {
            return v;
        }
    }
    if let Some(v) = OPEN_CONTEXT.with(|c| c.borrow().clone()) {
        return v;
    }
    super::sockets::default_context()
}

/// Runs `f` with `ctx` as the context of wrappers opened inside it.
pub fn with_open_context<T>(ctx: Value, f: impl FnOnce() -> T) -> T {
    let prev = OPEN_CONTEXT.with(|c| c.replace(Some(ctx)));
    let r = f();
    OPEN_CONTEXT.with(|c| *c.borrow_mut() = prev);
    r
}

/// A new wrapper instance (its `context` property set, then constructed).
fn instance(vm: &mut Vm, class: &PhpStr, context: Option<Value>) -> R<ObjRef> {
    let cls = vm.find_class(class, true)?.expect("wrapper class");
    let o = vm.instantiate(&cls)?;
    let ctx = context.unwrap_or(Value::Null);
    vm.set_prop_raw(&o, b"context", ctx);
    if let Some(f) = cls.magic.borrow().construct.clone() {
        vm.call_func_this(&f, Some(o.clone()), Vec::new())?;
    }
    Ok(o)
}

/// Calls `method` on the wrapper; `None` when it doesn't exist.
pub fn call(vm: &mut Vm, o: &ObjRef, method: &[u8], args: Vec<Value>) -> R<Option<Value>> {
    let cls = vm.class_of(o);
    if cls.find_method(method).is_none() && cls.magic.borrow().call.is_none() {
        return Ok(None);
    }
    Ok(Some(vm.call_method(o, method, args)?))
}

fn class_name(vm: &Vm, o: &ObjRef) -> String {
    vm.class_of(o).name.dname()
}

/// fopen() and friends on a user wrapper URL.
pub fn open(
    vm: &mut Vm,
    a: &Args,
    class: &PhpStr,
    path: &[u8],
    mode: &[u8],
    quiet: bool,
) -> R<Option<Stream>> {
    let ctx = call_context(a);
    with_open_context(ctx, || open_named(vm, &a.fname(), class, path, mode, quiet))
}

/// Opens a user wrapper URL for `fname` (messages).
pub fn open_named(
    vm: &mut Vm,
    fname: &str,
    class: &PhpStr,
    path: &[u8],
    mode: &[u8],
    quiet: bool,
) -> R<Option<Stream>> {
    // URL wrappers need allow_url_fopen (test file/include_userstream_003.phpt)
    if is_url_wrapper(path)
        && !vm
            .get_ini(b"allow_url_fopen")
            .is_some_and(|v| super::misc::assert_ini_truthy(&v))
    {
        if !quiet {
            url_disabled(vm, fname, path, "allow_url_fopen")?;
        }
        return Ok(None);
    }
    if is_url_wrapper(path)
        && INCLUDING.with(|c| c.get()) > 0
        && !vm
            .get_ini(b"allow_url_include")
            .is_some_and(|v| super::misc::assert_ini_truthy(&v))
    {
        if !quiet {
            url_disabled(vm, fname, path, "allow_url_include")?;
        }
        return Ok(None);
    }
    let ctx = OPEN_CONTEXT
        .with(|c| c.borrow().clone())
        .unwrap_or_else(super::sockets::default_context);
    let o = instance(vm, class, Some(ctx))?;
    let opened = PhpRef::new(Value::Null);
    let options = if quiet { 0 } else { 8 };
    let r = call(
        vm,
        &o,
        b"stream_open",
        vec![
            Value::Str(PhpStr::new(path)),
            Value::Str(PhpStr::new(mode)),
            Value::Int(options),
            Value::Ref(opened),
        ],
    )?;
    let ok = match r {
        None => {
            if !quiet {
                vm.warn(&format!(
                    "{}(): {}::stream_open is not implemented!",
                    fname,
                    class.dname()
                ))?;
            }
            false
        }
        Some(v) => vm.to_bool(&v),
    };
    if !ok {
        if !quiet {
            vm.warn(&format!(
                "{}({}): Failed to open stream: \"{}::stream_open\" call failed",
                fname,
                String::from_utf8_lossy(path),
                class.dname()
            ))?;
        }
        return Ok(None);
    }
    let mut s = Stream::new(
        Backing::User { obj: o },
        mode,
        path,
        "user-space",
        "user-space",
    );
    s.readable = true;
    s.writable = true;
    Ok(Some(s))
}

/// Reads one chunk from a user stream into its buffer; false at EOF.
pub fn fill(vm: &mut Vm, s: &mut Stream) -> R<bool> {
    let Backing::User { obj } = &s.backing else {
        return Ok(false);
    };
    let o = obj.clone();
    if s.eof {
        return Ok(false);
    }
    let r = call(vm, &o, b"stream_read", vec![Value::Int(8192)])?;
    let data = match r {
        None => {
            vm.warn(&format!(
                "fread(): {}::stream_read is not implemented!",
                class_name(vm, &o)
            ))?;
            s.eof = true;
            return Ok(false);
        }
        Some(Value::Bool(false)) | Some(Value::Null) => PhpStr::empty(),
        Some(v) => vm.to_php_str(&v)?,
    };
    if data.len() > 8192 {
        vm.warn(&format!(
            "fread(): {}::stream_read - read {} bytes more data than requested ({} read, 8192 max) - excess data will be lost",
            class_name(vm, &o),
            data.len() - 8192,
            data.len()
        ))?;
    }
    let n = data.len().min(8192);
    s.pending.extend_from_slice(&data[..n]);
    match call(vm, &o, b"stream_eof", Vec::new())? {
        None => {
            vm.warn(&format!(
                "fread(): {}::stream_eof is not implemented! Assuming EOF",
                class_name(vm, &o)
            ))?;
            s.eof = true;
        }
        Some(v) => {
            if vm.to_bool(&v) {
                s.eof = true;
            }
        }
    }
    Ok(n > 0)
}

pub fn write(vm: &mut Vm, s: &mut Stream, data: &[u8]) -> R<usize> {
    let Backing::User { obj } = &s.backing else {
        return Ok(0);
    };
    let o = obj.clone();
    let r = call(vm, &o, b"stream_write", vec![Value::Str(PhpStr::new(data))])?;
    match r {
        None => {
            vm.warn(&format!(
                "fwrite(): {}::stream_write is not implemented!",
                class_name(vm, &o)
            ))?;
            Ok(0)
        }
        Some(v) => {
            let n = vm.to_int(&v)?;
            if n > data.len() as i64 {
                vm.warn(&format!(
                    "fwrite(): {}::stream_write wrote {} bytes more data than requested ({} written, {} max)",
                    class_name(vm, &o),
                    n - data.len() as i64,
                    n,
                    data.len()
                ))?;
                return Ok(data.len());
            }
            Ok(n.max(0) as usize)
        }
    }
}

pub fn seek(vm: &mut Vm, s: &mut Stream, off: i64, whence: i64) -> R<bool> {
    let Backing::User { obj } = &s.backing else {
        return Ok(false);
    };
    let o = obj.clone();
    // A relative seek counts from the position the caller sees (after
    // what was buffered).
    let (off, whence) = if whence == 1 {
        (s.upos + off, 0)
    } else {
        (off, whence)
    };
    let r = call(
        vm,
        &o,
        b"stream_seek",
        vec![Value::Int(off), Value::Int(whence)],
    )?;
    let Some(r) = r else {
        vm.warn(&format!(
            "fseek(): {}::stream_seek is not implemented!",
            class_name(vm, &o)
        ))?;
        return Ok(false);
    };
    if !vm.to_bool(&r) {
        return Ok(false);
    }
    s.pending.clear();
    s.eof = false;
    if let Some(t) = call(vm, &o, b"stream_tell", Vec::new())? {
        s.upos = vm.to_int(&t)?;
    } else {
        s.upos = off;
    }
    Ok(true)
}

pub fn simple(vm: &mut Vm, s: &Stream, method: &[u8], args: Vec<Value>) -> R<Option<Value>> {
    let Backing::User { obj } = &s.backing else {
        return Ok(None);
    };
    let o = obj.clone();
    call(vm, &o, method, args)
}

/// stream_set_option() of a user stream: None when the stream isn't one;
/// a missing method warns and fails (test file/userstreams_003.phpt).
pub fn set_option(
    vm: &mut Vm,
    s: &Stream,
    fname: &str,
    option: i64,
    value: i64,
    ptr: Value,
) -> R<Option<bool>> {
    let Backing::User { obj } = &s.backing else {
        return Ok(None);
    };
    let o = obj.clone();
    match call(
        vm,
        &o,
        b"stream_set_option",
        vec![Value::Int(option), Value::Int(value), ptr],
    )? {
        Some(v) => Ok(Some(vm.to_bool(&v))),
        None => {
            let c = class_of_stream(vm, s);
            vm.warn(&format!(
                "{fname}(): {c}::stream_set_option is not implemented!"
            ))?;
            Ok(Some(false))
        }
    }
}

/// url_stat() of a user wrapper URL.
pub fn url_stat(vm: &mut Vm, class: &PhpStr, path: &[u8], flags: i64) -> R<Option<PhpArray>> {
    // the last result is cached until clearstatcache() or a change made
    // through a wrapper (test file/userwrapper.phpt)
    if let Some(st) = STAT_CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(p, _)| p.as_slice() == path)
            .map(|(_, st)| st.clone())
    }) {
        return Ok(Some(st));
    }
    let r = url_stat_uncached(vm, class, path, flags)?;
    if let Some(st) = &r {
        STAT_CACHE.with(|c| *c.borrow_mut() = Some((path.to_vec(), st.clone())));
    }
    Ok(r)
}

thread_local! {
    static STAT_CACHE: std::cell::RefCell<Option<(Vec<u8>, PhpArray)>> =
        const { std::cell::RefCell::new(None) };
}

/// Forgets the cached url_stat() result (clearstatcache()).
pub fn clear_stat_cache() {
    STAT_CACHE.with(|c| *c.borrow_mut() = None);
}

fn url_stat_uncached(vm: &mut Vm, class: &PhpStr, path: &[u8], flags: i64) -> R<Option<PhpArray>> {
    let o = instance(vm, class, None)?;
    let r = call(
        vm,
        &o,
        b"url_stat",
        vec![Value::Str(PhpStr::new(path)), Value::Int(flags)],
    )?;
    match r {
        Some(Value::Array(a)) => Ok(Some(a)),
        None => {
            if flags & 2 == 0 {
                vm.warn(&format!("{}::url_stat is not implemented!", class.dname()))?;
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// A path operation (unlink, rename, mkdir, rmdir, stream_metadata) on a
/// user wrapper URL: the method's result as a bool.
/// A path operation (unlink, rename, mkdir, rmdir, metadata): the stat
/// cache is dropped first.
pub fn path_op(vm: &mut Vm, a: &Args, class: &PhpStr, method: &str, args: Vec<Value>) -> R<Value> {
    clear_stat_cache();
    path_op_cached(vm, a, class, method, args)
}

fn path_op_cached(
    vm: &mut Vm,
    a: &Args,
    class: &PhpStr,
    method: &str,
    args: Vec<Value>,
) -> R<Value> {
    let ctx = call_context(a);
    let o = instance(vm, class, Some(ctx))?;
    match call(vm, &o, method.as_bytes(), args)? {
        Some(v) => Ok(Value::Bool(vm.to_bool(&v))),
        None => {
            vm.fwarn(
                a,
                &format!("{}::{} is not implemented!", class.dname(), method),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn class_of_stream(vm: &Vm, s: &Stream) -> String {
    match &s.backing {
        Backing::User { obj } | Backing::UserDir { obj } => class_name(vm, obj),
        _ => String::new(),
    }
}

/// scandir() on a user wrapper URL: all entries.
pub fn list(vm: &mut Vm, a: &Args, class: &PhpStr, path: &[u8]) -> R<Option<Vec<Vec<u8>>>> {
    let Some(r) = opendir(vm, a, class, path)? else {
        return Ok(None);
    };
    let o = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .and_then(|c| match &c.borrow().backing {
                Backing::UserDir { obj } => Some(obj.clone()),
                _ => None,
            })
    };
    let Some(o) = o else { return Ok(None) };
    let mut out = Vec::new();
    loop {
        match call(vm, &o, b"dir_readdir", Vec::new())? {
            Some(Value::Bool(false)) | None | Some(Value::Null) => break,
            Some(v) => out.push(vm.to_php_str(&v)?.to_vec()),
        }
    }
    call(vm, &o, b"dir_closedir", Vec::new())?;
    r.close();
    Ok(Some(out))
}

/// opendir() on a user wrapper URL.
pub fn opendir(vm: &mut Vm, a: &Args, class: &PhpStr, path: &[u8]) -> R<Option<ResRef>> {
    let ctx = call_context(a);
    let o = instance(vm, class, Some(ctx))?;
    let r = call(
        vm,
        &o,
        b"dir_opendir",
        vec![Value::Str(PhpStr::new(path)), Value::Int(0)],
    )?;
    let ok = match r {
        None => {
            vm.fwarn(
                a,
                &format!("{}::dir_opendir is not implemented!", class.dname()),
            )?;
            false
        }
        Some(v) => vm.to_bool(&v),
    };
    if !ok {
        vm.fwarn(
            a,
            &format!(
                "{}: Failed to open directory: \"{}::dir_opendir\" call failed",
                String::from_utf8_lossy(path),
                class.dname()
            ),
        )?;
        return Ok(None);
    }
    let s = Stream::new(Backing::UserDir { obj: o }, b"r", path, "user-space", "dir");
    Ok(Some(new_stream_res(s)))
}

/// The whole contents of a user wrapper URL (include/require).
pub fn read_all(vm: &mut Vm, fname: &str, class: &PhpStr, path: &[u8]) -> R<Option<Vec<u8>>> {
    // URL wrappers can't be included unless allow_url_include is on
    // (PHP manual: allow_url_include; test file/include_userstream_002.phpt)
    // opens made for an include, also from the wrapper's own code (test
    // file/include_userstream_002.phpt)
    INCLUDING.with(|c| c.set(c.get() + 1));
    let opened = open_named(vm, fname, class, path, b"rb", false);
    INCLUDING.with(|c| c.set(c.get() - 1));
    let Some(mut s) = opened? else {
        return Ok(None);
    };
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match s.read_some(vm, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    vm.check_deferred()?;
    if let Backing::User { obj } = &s.backing {
        let o = obj.clone();
        call(vm, &o, b"stream_close", Vec::new())?;
    }
    Ok(Some(out))
}
