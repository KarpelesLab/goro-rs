//! php-src's ext/posix (posix.c): thin wrappers over the POSIX.1 system
//! calls, with the extension's last_error.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString};
use std::rc::Rc;

use goro_core::arginfo::T_INT;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// The extension name (`extension_loaded('posix')`).
pub const NAME: &[u8] = b"posix";

type R = Result<Value, VmError>;

thread_local! {
    /// POSIX_G(last_error)
    static LAST_ERROR: Cell<i32> = const { Cell::new(0) };
}

fn set_last_error(e: i32) {
    LAST_ERROR.with(|c| c.set(e));
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn set_errno(e: i32) {
    unsafe {
        *libc::__errno_location() = e;
    }
}

/// POSIX_G(last_error) = errno; RETURN_FALSE
fn fail() -> R {
    set_last_error(errno());
    Ok(Value::False)
}

const INT_MAX: i64 = i32::MAX as i64;
const INT_MIN: i64 = i32::MIN as i64;

const CONSTANTS: &[(&str, i64)] = &[
    ("POSIX_F_OK", libc::F_OK as i64),
    ("POSIX_X_OK", libc::X_OK as i64),
    ("POSIX_W_OK", libc::W_OK as i64),
    ("POSIX_R_OK", libc::R_OK as i64),
    ("POSIX_S_IFREG", libc::S_IFREG as i64),
    ("POSIX_S_IFCHR", libc::S_IFCHR as i64),
    ("POSIX_S_IFBLK", libc::S_IFBLK as i64),
    ("POSIX_S_IFIFO", libc::S_IFIFO as i64),
    ("POSIX_S_IFSOCK", libc::S_IFSOCK as i64),
    ("POSIX_RLIMIT_AS", libc::RLIMIT_AS as i64),
    ("POSIX_RLIMIT_CORE", libc::RLIMIT_CORE as i64),
    ("POSIX_RLIMIT_CPU", libc::RLIMIT_CPU as i64),
    ("POSIX_RLIMIT_DATA", libc::RLIMIT_DATA as i64),
    ("POSIX_RLIMIT_FSIZE", libc::RLIMIT_FSIZE as i64),
    ("POSIX_RLIMIT_LOCKS", libc::RLIMIT_LOCKS as i64),
    ("POSIX_RLIMIT_MEMLOCK", libc::RLIMIT_MEMLOCK as i64),
    ("POSIX_RLIMIT_MSGQUEUE", libc::RLIMIT_MSGQUEUE as i64),
    ("POSIX_RLIMIT_NICE", libc::RLIMIT_NICE as i64),
    ("POSIX_RLIMIT_NOFILE", libc::RLIMIT_NOFILE as i64),
    ("POSIX_RLIMIT_NPROC", libc::RLIMIT_NPROC as i64),
    ("POSIX_RLIMIT_RSS", libc::RLIMIT_RSS as i64),
    ("POSIX_RLIMIT_RTPRIO", libc::RLIMIT_RTPRIO as i64),
    ("POSIX_RLIMIT_RTTIME", libc::RLIMIT_RTTIME as i64),
    ("POSIX_RLIMIT_SIGPENDING", libc::RLIMIT_SIGPENDING as i64),
    ("POSIX_RLIMIT_STACK", libc::RLIMIT_STACK as i64),
    ("POSIX_RLIMIT_INFINITY", libc::RLIM_INFINITY as i64),
    ("POSIX_SC_ARG_MAX", libc::_SC_ARG_MAX as i64),
    ("POSIX_SC_CHILD_MAX", libc::_SC_CHILD_MAX as i64),
    ("POSIX_SC_CLK_TCK", libc::_SC_CLK_TCK as i64),
    ("POSIX_SC_PAGESIZE", libc::_SC_PAGESIZE as i64),
    (
        "POSIX_SC_NPROCESSORS_CONF",
        libc::_SC_NPROCESSORS_CONF as i64,
    ),
    (
        "POSIX_SC_NPROCESSORS_ONLN",
        libc::_SC_NPROCESSORS_ONLN as i64,
    ),
    ("POSIX_PC_LINK_MAX", libc::_PC_LINK_MAX as i64),
    ("POSIX_PC_MAX_CANON", libc::_PC_MAX_CANON as i64),
    ("POSIX_PC_MAX_INPUT", libc::_PC_MAX_INPUT as i64),
    ("POSIX_PC_NAME_MAX", libc::_PC_NAME_MAX as i64),
    ("POSIX_PC_PATH_MAX", libc::_PC_PATH_MAX as i64),
    ("POSIX_PC_PIPE_BUF", libc::_PC_PIPE_BUF as i64),
    (
        "POSIX_PC_CHOWN_RESTRICTED",
        libc::_PC_CHOWN_RESTRICTED as i64,
    ),
    ("POSIX_PC_NO_TRUNC", libc::_PC_NO_TRUNC as i64),
    ("POSIX_PC_ALLOC_SIZE_MIN", libc::_PC_ALLOC_SIZE_MIN as i64),
    ("POSIX_PC_SYMLINK_MAX", libc::_PC_SYMLINK_MAX as i64),
    ("POSIX_SC_OPEN_MAX", libc::_SC_OPEN_MAX as i64),
];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("posix_kill", posix_kill),
    ("posix_getpid", posix_getpid),
    ("posix_getppid", posix_getppid),
    ("posix_getuid", posix_getuid),
    ("posix_setuid", posix_setuid),
    ("posix_geteuid", posix_geteuid),
    ("posix_seteuid", posix_seteuid),
    ("posix_getgid", posix_getgid),
    ("posix_setgid", posix_setgid),
    ("posix_getegid", posix_getegid),
    ("posix_setegid", posix_setegid),
    ("posix_getgroups", posix_getgroups),
    ("posix_getlogin", posix_getlogin),
    ("posix_getpgrp", posix_getpgrp),
    ("posix_setsid", posix_setsid),
    ("posix_setpgid", posix_setpgid),
    ("posix_getpgid", posix_getpgid),
    ("posix_getsid", posix_getsid),
    ("posix_uname", posix_uname),
    ("posix_times", posix_times),
    ("posix_ctermid", posix_ctermid),
    ("posix_ttyname", posix_ttyname),
    ("posix_isatty", posix_isatty),
    ("posix_getcwd", posix_getcwd),
    ("posix_mkfifo", posix_mkfifo),
    ("posix_mknod", posix_mknod),
    ("posix_access", posix_access),
    ("posix_eaccess", posix_eaccess),
    ("posix_getgrnam", posix_getgrnam),
    ("posix_getgrgid", posix_getgrgid),
    ("posix_getpwnam", posix_getpwnam),
    ("posix_getpwuid", posix_getpwuid),
    ("posix_getrlimit", posix_getrlimit),
    ("posix_setrlimit", posix_setrlimit),
    ("posix_get_last_error", posix_get_last_error),
    ("posix_errno", posix_get_last_error),
    ("posix_strerror", posix_strerror),
    ("posix_initgroups", posix_initgroups),
    ("posix_sysconf", posix_sysconf),
    ("posix_pathconf", posix_pathconf),
    ("posix_fpathconf", posix_fpathconf),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    // PHP_MINFO_FUNCTION(posix)
    vm.phpinfo_hooks
        .push(|_| "\nposix\n\nPOSIX support => enabled\n".to_string());
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        );
    }
    for &(name, v) in CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
    }
    vm.module_constants.push((
        NAME.to_vec(),
        CONSTANTS
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect(),
    ));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> R {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Ok(Value::Null)
}

fn arg_prefix(func: &str, n: usize) -> String {
    let name = goro_core::stubs::find_function(func.as_bytes())
        .and_then(|f| f.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("");
    format!("{}(): Argument #{} (${})", func, n, name)
}

fn value_error(vm: &mut Vm, func: &str, n: usize, what: &str) -> R {
    let msg = format!("{} {}", arg_prefix(func, n), what);
    throw(vm, b"ValueError", &msg)
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn long(args: &[Value], i: usize) -> i64 {
    arg(args, i).to_long()
}

fn bytes(args: &[Value], i: usize) -> Vec<u8> {
    arg(args, i).to_php_string().as_bytes().to_vec()
}

fn s(v: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(v))
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn set(a: &mut PhpArray, k: &str, v: Value) {
    a.set(ArrayKey::String(PhpString::from_string(k.to_string())), v);
}

fn cstr(p: *const libc::c_char) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
}

/// A C string of `b` (cut at its first NUL, as C sees it).
fn c_string(b: &[u8]) -> CString {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    CString::new(&b[..end]).unwrap_or_default()
}

/// Z_PARAM_PATH: Some(path), or None after the "null bytes" ValueError.
fn path_arg(vm: &mut Vm, func: &str, args: &[Value], i: usize) -> Option<Vec<u8>> {
    let p = bytes(args, i);
    if p.contains(&0) {
        let _ = value_error(vm, func, i + 1, "must not contain any null bytes");
        return None;
    }
    Some(p)
}

/// PHP_POSIX_CHECK_PID()
fn check_pid(vm: &mut Vm, func: &str, v: i64, n: usize, lower: i64, upper: i64) -> bool {
    if !(lower..=upper).contains(&v) {
        // (POSIX_PID_MIN is INT_MIN, an int printed with ZEND_LONG_FMT: the
        // 64-bit read sees it zero-extended)
        let shown_lower = if lower == i64::from(i32::MIN) {
            i64::from(i32::MIN as u32)
        } else {
            lower
        };
        let _ = value_error(
            vm,
            func,
            n,
            &format!("must be between {} and {}", shown_lower, upper),
        );
        return false;
    }
    true
}

/// zend_parse_arg_long() in weak or strict mode: Ok(None) when the value
/// is not accepted; Err(()) when an exception was thrown (a deprecation
/// turned into one).
fn parse_arg_long(vm: &mut Vm, func: &str, n: usize, v: &Value) -> Result<Option<i64>, ()> {
    let strict = vm.caller_strict_types;
    match v {
        Value::Long(l) => return Ok(Some(*l)),
        Value::Null | Value::Undef => {
            if strict {
                return Ok(None);
            }
            let name = goro_core::stubs::find_function(func.as_bytes())
                .and_then(|f| f.args.get(n - 1))
                .map(|a| a.name)
                .unwrap_or("");
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #{} (${}) of type int is deprecated",
                func, n, name
            ));
            if vm.current_exception.is_some() {
                return Err(());
            }
            return Ok(Some(0));
        }
        Value::Array(_) | Value::Object(_) | Value::Resource(_) | Value::Generator(_) => {
            return Ok(None);
        }
        _ => {}
    }
    let line = vm.current_line;
    match vm.zpp_coerce_scalar(T_INT, v, strict, line) {
        Ok(Some(c)) => {
            if vm.current_exception.is_some() {
                return Err(());
            }
            Ok(Some(c.to_long()))
        }
        Ok(None) => Ok(None),
        Err(()) => Err(()),
    }
}

/// zval_get_long(): objects without a cast handler warn and become 1.
fn zval_get_long(vm: &mut Vm, v: &Value) -> i64 {
    match v {
        Value::Object(o) => {
            if let Some(c) = goro_core::value::native_cast(o, 1) {
                return c.to_long();
            }
            if let Some(n) = vm.object_to_long.and_then(|f| f(v)) {
                return n;
            }
            let cn = goro_core::value::display_class_name(&o.borrow().class_name);
            vm.emit_warning(&format!(
                "Object of class {} could not be converted to int",
                cn
            ));
            1
        }
        other => other.to_long(),
    }
}

enum Fd {
    Ok(i64),
    /// RETURN_FALSE
    False,
    /// RETURN_THROWS
    Throw,
}

/// php_posix_stream_get_fd() for a resource argument.
fn stream_get_fd(vm: &mut Vm, func: &str, id: i64) -> Fd {
    let is_stream = goro_core::value::is_open_resource(id)
        && matches!(
            goro_core::value::resource_type_of(id).as_deref(),
            Some(b"stream") | Some(b"persistent stream")
        );
    let cast = if is_stream || (1..=3).contains(&id) {
        goro_ext_standard::sockets_streams::ext_stream_cast_fd(id)
    } else {
        None
    };
    match cast {
        Some(Ok(fd)) => Fd::Ok(fd as i64),
        Some(Err(ty)) => {
            vm.emit_warning(&format!(
                "{}(): Could not use stream of type '{}'",
                func, ty
            ));
            Fd::False
        }
        None => {
            let _ = throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): supplied resource is not a valid stream resource",
                    func
                ),
            );
            Fd::Throw
        }
    }
}

fn fd_range_warning(vm: &mut Vm, func: &str) {
    vm.emit_warning(&format!(
        "{}(): Argument #1 ($file_descriptor) must be between 0 and {}",
        func,
        i32::MAX
    ));
    set_last_error(libc::EBADF);
}

// ---------------------------------------------------------------------------
// Process identification
// ---------------------------------------------------------------------------

fn posix_kill(vm: &mut Vm, args: &[Value]) -> R {
    let (pid, sig) = (long(args, 0), long(args, 1));
    if !check_pid(vm, "posix_kill", pid, 1, INT_MIN, INT_MAX) {
        return Ok(Value::Null);
    }
    if unsafe { libc::kill(pid as libc::pid_t, sig as libc::c_int) } < 0 {
        return fail();
    }
    Ok(Value::True)
}

fn posix_getpid(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::getpid() } as i64))
}

fn posix_getppid(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::getppid() } as i64))
}

fn posix_getuid(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::getuid() } as i64))
}

fn posix_getgid(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::getgid() } as i64))
}

fn posix_geteuid(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::geteuid() } as i64))
}

fn posix_getegid(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::getegid() } as i64))
}

/// PHP_POSIX_SINGLE_ARG_FUNC()
fn single_arg(args: &[Value], f: unsafe extern "C" fn(u32) -> libc::c_int) -> R {
    let v = long(args, 0);
    if unsafe { f(v as u32) } < 0 {
        return fail();
    }
    Ok(Value::True)
}

fn posix_setuid(_vm: &mut Vm, args: &[Value]) -> R {
    single_arg(args, libc::setuid)
}

fn posix_setgid(_vm: &mut Vm, args: &[Value]) -> R {
    single_arg(args, libc::setgid)
}

fn posix_seteuid(_vm: &mut Vm, args: &[Value]) -> R {
    single_arg(args, libc::seteuid)
}

fn posix_setegid(_vm: &mut Vm, args: &[Value]) -> R {
    single_arg(args, libc::setegid)
}

fn posix_getgroups(_vm: &mut Vm, _args: &[Value]) -> R {
    let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if n < 0 {
        return fail();
    }
    let mut list = vec![0 as libc::gid_t; n as usize];
    let n = unsafe { libc::getgroups(n, list.as_mut_ptr()) };
    if n < 0 {
        return fail();
    }
    let mut ret = PhpArray::new();
    for g in &list[..n as usize] {
        ret.push(Value::Long(*g as i64));
    }
    Ok(arr(ret))
}

fn posix_getlogin(_vm: &mut Vm, _args: &[Value]) -> R {
    let p = unsafe { libc::getlogin() };
    if p.is_null() {
        return fail();
    }
    Ok(s(&cstr(p)))
}

fn posix_getpgrp(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::getpgrp() } as i64))
}

fn posix_setsid(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::setsid() } as i64))
}

fn posix_setpgid(vm: &mut Vm, args: &[Value]) -> R {
    let (pid, pgid) = (long(args, 0), long(args, 1));
    if !check_pid(vm, "posix_setpgid", pid, 1, 0, INT_MAX)
        || !check_pid(vm, "posix_setpgid", pgid, 2, 0, INT_MAX)
    {
        return Ok(Value::Null);
    }
    if unsafe { libc::setpgid(pid as libc::pid_t, pgid as libc::pid_t) } < 0 {
        return fail();
    }
    Ok(Value::True)
}

fn posix_getpgid(_vm: &mut Vm, args: &[Value]) -> R {
    let v = unsafe { libc::getpgid(long(args, 0) as libc::pid_t) };
    if v < 0 {
        return fail();
    }
    Ok(Value::Long(v as i64))
}

fn posix_getsid(vm: &mut Vm, args: &[Value]) -> R {
    let v = long(args, 0);
    if !check_pid(vm, "posix_getsid", v, 1, 0, INT_MAX) {
        return Ok(Value::Null);
    }
    let r = unsafe { libc::getsid(v as libc::pid_t) };
    if r < 0 {
        return fail();
    }
    Ok(Value::Long(r as i64))
}

fn posix_uname(_vm: &mut Vm, _args: &[Value]) -> R {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } < 0 {
        return fail();
    }
    let mut ret = PhpArray::new();
    set(&mut ret, "sysname", s(&cstr(u.sysname.as_ptr())));
    set(&mut ret, "nodename", s(&cstr(u.nodename.as_ptr())));
    set(&mut ret, "release", s(&cstr(u.release.as_ptr())));
    set(&mut ret, "version", s(&cstr(u.version.as_ptr())));
    set(&mut ret, "machine", s(&cstr(u.machine.as_ptr())));
    set(&mut ret, "domainname", s(&cstr(u.domainname.as_ptr())));
    Ok(arr(ret))
}

fn posix_times(_vm: &mut Vm, _args: &[Value]) -> R {
    let mut t: libc::tms = unsafe { std::mem::zeroed() };
    let ticks = unsafe { libc::times(&mut t) };
    if ticks == -1 as libc::clock_t {
        return fail();
    }
    let mut ret = PhpArray::new();
    set(&mut ret, "ticks", Value::Long(ticks as i64));
    set(&mut ret, "utime", Value::Long(t.tms_utime as i64));
    set(&mut ret, "stime", Value::Long(t.tms_stime as i64));
    set(&mut ret, "cutime", Value::Long(t.tms_cutime as i64));
    set(&mut ret, "cstime", Value::Long(t.tms_cstime as i64));
    Ok(arr(ret))
}

unsafe extern "C" {
    fn ctermid(s: *mut libc::c_char) -> *mut libc::c_char;
    fn eaccess(path: *const libc::c_char, mode: libc::c_int) -> libc::c_int;
    fn initgroups(user: *const libc::c_char, group: libc::gid_t) -> libc::c_int;
}

fn posix_ctermid(_vm: &mut Vm, _args: &[Value]) -> R {
    let mut buf = [0 as libc::c_char; 64];
    let p = unsafe { ctermid(buf.as_mut_ptr()) };
    if p.is_null() {
        return fail();
    }
    Ok(s(&cstr(buf.as_ptr())))
}

// ---------------------------------------------------------------------------
// Terminals
// ---------------------------------------------------------------------------

fn posix_ttyname(vm: &mut Vm, args: &[Value]) -> R {
    let f = "posix_ttyname";
    let z = arg(args, 0);
    let fd = if let Value::Resource(id) = z {
        match stream_get_fd(vm, f, id.id()) {
            Fd::Ok(fd) => fd,
            Fd::False => return Ok(Value::False),
            Fd::Throw => return Ok(Value::Null),
        }
    } else {
        let fd = match parse_arg_long(vm, f, 1, &z) {
            Ok(Some(fd)) => fd,
            Ok(None) => {
                vm.emit_warning(&format!(
                    "{}(): Argument #1 ($file_descriptor) must be of type int|resource, {} given",
                    f,
                    Vm::value_type_name(&z)
                ));
                zval_get_long(vm, &z)
            }
            Err(()) => return Ok(Value::Null),
        };
        if !(0..=INT_MAX).contains(&fd) {
            fd_range_warning(vm, f);
            return Ok(Value::False);
        }
        fd
    };
    let p = unsafe { libc::ttyname(fd as libc::c_int) };
    if p.is_null() {
        return fail();
    }
    Ok(s(&cstr(p)))
}

fn posix_isatty(vm: &mut Vm, args: &[Value]) -> R {
    let f = "posix_isatty";
    let z = arg(args, 0);
    let fd = if let Value::Resource(id) = z {
        match stream_get_fd(vm, f, id.id()) {
            Fd::Ok(fd) => fd,
            Fd::False => return Ok(Value::False),
            Fd::Throw => return Ok(Value::Null),
        }
    } else {
        match parse_arg_long(vm, f, 1, &z) {
            Ok(Some(fd)) => fd,
            Ok(None) => {
                vm.emit_warning(&format!(
                    "{}(): Argument #1 ($file_descriptor) must be of type int|resource, {} given",
                    f,
                    Vm::value_type_name(&z)
                ));
                return Ok(Value::False);
            }
            Err(()) => return Ok(Value::Null),
        }
    };
    if !(0..=INT_MAX).contains(&fd) {
        fd_range_warning(vm, f);
        return Ok(Value::False);
    }
    if unsafe { libc::isatty(fd as libc::c_int) } != 0 {
        Ok(Value::True)
    } else {
        fail()
    }
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

fn posix_getcwd(_vm: &mut Vm, _args: &[Value]) -> R {
    let mut buf = vec![0 as libc::c_char; 4096];
    let p = unsafe { libc::getcwd(buf.as_mut_ptr(), buf.len()) };
    if p.is_null() {
        return fail();
    }
    Ok(s(&cstr(buf.as_ptr())))
}

fn posix_mkfifo(vm: &mut Vm, args: &[Value]) -> R {
    let f = "posix_mkfifo";
    let Some(path) = path_arg(vm, f, args, 0) else {
        return Ok(Value::Null);
    };
    let mode = long(args, 1);
    if !vm.check_open_basedir(f, &String::from_utf8_lossy(&path)) {
        return Ok(Value::False);
    }
    let c = c_string(&path);
    if unsafe { libc::mkfifo(c.as_ptr(), mode as libc::mode_t) } < 0 {
        return fail();
    }
    Ok(Value::True)
}

fn posix_mknod(vm: &mut Vm, args: &[Value]) -> R {
    let f = "posix_mknod";
    let Some(path) = path_arg(vm, f, args, 0) else {
        return Ok(Value::Null);
    };
    let mode = long(args, 1);
    let major = long(args, 2);
    let minor = long(args, 3);
    if !vm.check_open_basedir(f, &String::from_utf8_lossy(&path)) {
        return Ok(Value::False);
    }
    let mut dev: libc::dev_t = 0;
    if mode & libc::S_IFCHR as i64 != 0 || mode & libc::S_IFBLK as i64 != 0 {
        if major == 0 {
            return value_error(
                vm,
                f,
                3,
                "cannot be 0 for the POSIX_S_IFCHR and POSIX_S_IFBLK modes",
            );
        }
        dev = libc::makedev(major as u32, minor as u32);
    }
    let c = c_string(&path);
    if unsafe { libc::mknod(c.as_ptr(), mode as libc::mode_t, dev) } < 0 {
        return fail();
    }
    Ok(Value::True)
}

fn access_common(vm: &mut Vm, args: &[Value], eacc: bool) -> R {
    let f = if eacc {
        "posix_eaccess"
    } else {
        "posix_access"
    };
    let Some(filename) = path_arg(vm, f, args, 0) else {
        return Ok(Value::Null);
    };
    let mode = long(args, 1);
    // expand_filepath(): NULL for a path virtual_file_ex() rejects.
    let cwd_len = if filename.starts_with(b"/") {
        0
    } else {
        std::env::current_dir()
            .map(|d| d.as_os_str().len() + 1)
            .unwrap_or(0)
    };
    let path = if filename.len() + cwd_len >= goro_core::basedir::MAXPATHLEN - 1 {
        None
    } else {
        goro_core::basedir::expand_filepath(&String::from_utf8_lossy(&filename))
    };
    let Some(path) = path else {
        if eacc {
            return value_error(vm, f, 1, "must not be empty");
        }
        set_last_error(libc::EIO);
        return Ok(Value::False);
    };
    if !vm.check_open_basedir(f, &path) {
        set_last_error(libc::EPERM);
        return Ok(Value::False);
    }
    let c = c_string(path.as_bytes());
    let r = unsafe {
        if eacc {
            eaccess(c.as_ptr(), mode as libc::c_int)
        } else {
            libc::access(c.as_ptr(), mode as libc::c_int)
        }
    };
    if r != 0 {
        return fail();
    }
    Ok(Value::True)
}

fn posix_access(vm: &mut Vm, args: &[Value]) -> R {
    access_common(vm, args, false)
}

fn posix_eaccess(vm: &mut Vm, args: &[Value]) -> R {
    access_common(vm, args, true)
}

// ---------------------------------------------------------------------------
// User and group databases
// ---------------------------------------------------------------------------

/// php_posix_group_to_array()
fn group_to_array(g: &libc::group) -> Value {
    let mut ret = PhpArray::new();
    set(&mut ret, "name", s(&cstr(g.gr_name)));
    if g.gr_passwd.is_null() {
        set(&mut ret, "passwd", Value::Null);
    } else {
        set(&mut ret, "passwd", s(&cstr(g.gr_passwd)));
    }
    let mut members = PhpArray::new();
    if !g.gr_mem.is_null() {
        let mut i = 0;
        loop {
            let m = unsafe { *g.gr_mem.add(i) };
            if m.is_null() {
                break;
            }
            members.push(s(&cstr(m)));
            i += 1;
        }
    }
    set(&mut ret, "members", arr(members));
    set(&mut ret, "gid", Value::Long(g.gr_gid as i64));
    arr(ret)
}

fn posix_getgrnam(_vm: &mut Vm, args: &[Value]) -> R {
    let name = c_string(&bytes(args, 0));
    set_errno(0);
    let g = unsafe { libc::getgrnam(name.as_ptr()) };
    if g.is_null() {
        return fail();
    }
    Ok(group_to_array(unsafe { &*g }))
}

fn posix_getgrgid(_vm: &mut Vm, args: &[Value]) -> R {
    set_errno(0);
    let g = unsafe { libc::getgrgid(long(args, 0) as libc::gid_t) };
    if g.is_null() {
        return fail();
    }
    Ok(group_to_array(unsafe { &*g }))
}

/// php_posix_passwd_to_array()
fn passwd_to_array(pw: &libc::passwd) -> Value {
    let mut ret = PhpArray::new();
    set(&mut ret, "name", s(&cstr(pw.pw_name)));
    set(&mut ret, "passwd", s(&cstr(pw.pw_passwd)));
    set(&mut ret, "uid", Value::Long(pw.pw_uid as i64));
    set(&mut ret, "gid", Value::Long(pw.pw_gid as i64));
    set(&mut ret, "gecos", s(&cstr(pw.pw_gecos)));
    set(&mut ret, "dir", s(&cstr(pw.pw_dir)));
    set(&mut ret, "shell", s(&cstr(pw.pw_shell)));
    arr(ret)
}

fn posix_getpwnam(_vm: &mut Vm, args: &[Value]) -> R {
    let name = c_string(&bytes(args, 0));
    set_errno(0);
    let pw = unsafe { libc::getpwnam(name.as_ptr()) };
    if pw.is_null() {
        return fail();
    }
    Ok(passwd_to_array(unsafe { &*pw }))
}

fn posix_getpwuid(_vm: &mut Vm, args: &[Value]) -> R {
    set_errno(0);
    let pw = unsafe { libc::getpwuid(long(args, 0) as libc::uid_t) };
    if pw.is_null() {
        return fail();
    }
    Ok(passwd_to_array(unsafe { &*pw }))
}

fn posix_initgroups(_vm: &mut Vm, args: &[Value]) -> R {
    let name = bytes(args, 0);
    if name.is_empty() {
        return Ok(Value::False);
    }
    let c = c_string(&name);
    let r = unsafe { initgroups(c.as_ptr(), long(args, 1) as libc::gid_t) };
    Ok(if r == 0 { Value::True } else { Value::False })
}

// ---------------------------------------------------------------------------
// Resource limits
// ---------------------------------------------------------------------------

const LIMITS: &[(libc::__rlimit_resource_t, &str)] = &[
    (libc::RLIMIT_CORE, "core"),
    (libc::RLIMIT_DATA, "data"),
    (libc::RLIMIT_STACK, "stack"),
    (libc::RLIMIT_AS, "totalmem"),
    (libc::RLIMIT_RSS, "rss"),
    (libc::RLIMIT_NPROC, "maxproc"),
    (libc::RLIMIT_MEMLOCK, "memlock"),
    (libc::RLIMIT_CPU, "cpu"),
    (libc::RLIMIT_FSIZE, "filesize"),
    (libc::RLIMIT_NOFILE, "openfiles"),
];

fn rlim_value(v: libc::rlim_t) -> Value {
    if v == libc::RLIM_INFINITY {
        s(b"unlimited")
    } else {
        Value::Long(v as i64)
    }
}

/// getrlimit(), with goro's own address-space cap hidden (child_proc).
fn script_getrlimit(res: libc::__rlimit_resource_t, rl: &mut libc::rlimit) -> i32 {
    let r = unsafe { libc::getrlimit(res, rl) };
    if r == 0 && res == libc::RLIMIT_AS {
        if let Some((soft, hard)) = goro_core::child_proc::script_rlimit_as() {
            rl.rlim_cur = soft as libc::rlim_t;
            rl.rlim_max = hard as libc::rlim_t;
        }
    }
    r
}

fn posix_getrlimit(_vm: &mut Vm, args: &[Value]) -> R {
    match arg(args, 0) {
        Value::Undef | Value::Null => {
            let mut ret = PhpArray::new();
            for &(res, name) in LIMITS {
                let mut rl: libc::rlimit = unsafe { std::mem::zeroed() };
                if script_getrlimit(res, &mut rl) < 0 {
                    return fail();
                }
                set(&mut ret, &format!("soft {}", name), rlim_value(rl.rlim_cur));
                set(&mut ret, &format!("hard {}", name), rlim_value(rl.rlim_max));
            }
            Ok(arr(ret))
        }
        v => {
            let mut rl: libc::rlimit = unsafe { std::mem::zeroed() };
            let res = v.to_long() as i32 as libc::__rlimit_resource_t;
            if script_getrlimit(res, &mut rl) < 0 {
                return fail();
            }
            let mut ret = PhpArray::new();
            ret.push(rlim_value(rl.rlim_cur));
            ret.push(rlim_value(rl.rlim_max));
            Ok(arr(ret))
        }
    }
}

fn posix_setrlimit(vm: &mut Vm, args: &[Value]) -> R {
    let f = "posix_setrlimit";
    let (res, cur, max) = (long(args, 0), long(args, 1), long(args, 2));
    if cur < -1 {
        return value_error(vm, f, 2, "must be greater or equal to -1");
    }
    if max < -1 {
        return value_error(vm, f, 3, "must be greater or equal to -1");
    }
    if max > -1 && cur > max {
        return value_error(vm, f, 2, &format!("must be lower or equal to {}", max));
    }
    let rl = libc::rlimit {
        rlim_cur: cur as libc::rlim_t,
        rlim_max: max as libc::rlim_t,
    };
    if unsafe { libc::setrlimit(res as i32 as libc::__rlimit_resource_t, &rl) } == -1 {
        return fail();
    }
    if res == libc::RLIMIT_AS as i64 {
        goro_core::child_proc::note_user_rlimit_as();
    }
    Ok(Value::True)
}

// ---------------------------------------------------------------------------
// Errors and configuration
// ---------------------------------------------------------------------------

fn posix_get_last_error(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(LAST_ERROR.with(|c| c.get()) as i64))
}

fn posix_strerror(_vm: &mut Vm, args: &[Value]) -> R {
    let e = long(args, 0) as libc::c_int;
    Ok(s(&cstr(unsafe { libc::strerror(e) })))
}

fn posix_sysconf(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(Value::Long(
        unsafe { libc::sysconf(long(args, 0) as libc::c_int) } as i64,
    ))
}

fn posix_pathconf(vm: &mut Vm, args: &[Value]) -> R {
    let f = "posix_pathconf";
    let Some(path) = path_arg(vm, f, args, 0) else {
        return Ok(Value::Null);
    };
    let name = long(args, 1);
    if path.is_empty() {
        return value_error(vm, f, 1, "must not be empty");
    }
    let ps = String::from_utf8_lossy(&path).to_string();
    if !vm.check_open_basedir(f, &ps) {
        vm.emit_warning(&format!("{}(): Invalid path supplied: {}", f, ps));
        return Ok(Value::False);
    }
    let c = c_string(&path);
    set_errno(0);
    let r = unsafe { libc::pathconf(c.as_ptr(), name as libc::c_int) };
    if r < 0 && errno() != 0 {
        return fail();
    }
    Ok(Value::Long(r as i64))
}

fn posix_fpathconf(vm: &mut Vm, args: &[Value]) -> R {
    let f = "posix_fpathconf";
    let z = arg(args, 0);
    // Z_PARAM_LONG($name) is parsed before the descriptor is looked at.
    let nv = arg(args, 1);
    let name = match parse_arg_long(vm, f, 2, &nv) {
        Ok(Some(n)) => n,
        Ok(None) => {
            let msg = format!(
                "{} must be of type int, {} given",
                arg_prefix(f, 2),
                Vm::value_type_name(&nv)
            );
            return throw(vm, b"TypeError", &msg);
        }
        Err(()) => return Ok(Value::Null),
    };
    let fd = if let Value::Resource(id) = z {
        match stream_get_fd(vm, f, id.id()) {
            Fd::Ok(fd) => fd,
            Fd::False => return Ok(Value::False),
            Fd::Throw => return Ok(Value::Null),
        }
    } else {
        match parse_arg_long(vm, f, 1, &z) {
            Ok(Some(fd)) => fd,
            Ok(None) => {
                let msg = format!(
                    "{} must be of type int|resource, {} given",
                    arg_prefix(f, 1),
                    Vm::value_type_name(&z)
                );
                return throw(vm, b"TypeError", &msg);
            }
            Err(()) => return Ok(Value::Null),
        }
    };
    if !(0..=INT_MAX).contains(&fd) {
        fd_range_warning(vm, f);
        return Ok(Value::False);
    }
    set_errno(0);
    let r = unsafe { libc::fpathconf(fd as libc::c_int, name as libc::c_int) };
    if r < 0 && errno() != 0 {
        return fail();
    }
    Ok(Value::Long(r as i64))
}
