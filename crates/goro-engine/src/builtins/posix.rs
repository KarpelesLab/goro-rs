//! ext/posix (PHP manual: POSIX functions) over the C library's calls.
//! Argument checks, the int|resource file-descriptor parameter, the error
//! number bookkeeping (posix_get_last_error) and the array layouts follow
//! the ext/posix tests and PHP 8.5's observed output.

use std::cell::Cell;
use std::ffi::{CStr, CString};

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "posix",
    functions: &[
        (
            "function posix_kill(int $process_id, int $signal): bool",
            posix_kill,
        ),
        ("function posix_getpid(): int", posix_getpid),
        ("function posix_getppid(): int", posix_getppid),
        ("function posix_getuid(): int", posix_getuid),
        ("function posix_setuid(int $user_id): bool", posix_setuid),
        ("function posix_geteuid(): int", posix_geteuid),
        ("function posix_seteuid(int $user_id): bool", posix_seteuid),
        ("function posix_getgid(): int", posix_getgid),
        ("function posix_setgid(int $group_id): bool", posix_setgid),
        ("function posix_getegid(): int", posix_getegid),
        ("function posix_setegid(int $group_id): bool", posix_setegid),
        ("function posix_getgroups(): array|false", posix_getgroups),
        ("function posix_getlogin(): string|false", posix_getlogin),
        ("function posix_getpgrp(): int", posix_getpgrp),
        ("function posix_setsid(): int", posix_setsid),
        (
            "function posix_setpgid(int $process_id, int $process_group_id): bool",
            posix_setpgid,
        ),
        (
            "function posix_getpgid(int $process_id): int|false",
            posix_getpgid,
        ),
        (
            "function posix_getsid(int $process_id): int|false",
            posix_getsid,
        ),
        ("function posix_uname(): array|false", posix_uname),
        ("function posix_times(): array|false", posix_times),
        ("function posix_ctermid(): string|false", posix_ctermid),
        (
            "function posix_ttyname($file_descriptor): string|false",
            posix_ttyname,
        ),
        (
            "function posix_isatty($file_descriptor): bool",
            posix_isatty,
        ),
        ("function posix_getcwd(): string|false", posix_getcwd),
        (
            "function posix_mkfifo(string $filename, int $permissions): bool",
            posix_mkfifo,
        ),
        (
            "function posix_mknod(string $filename, int $flags, int $major = 0, int $minor = 0): bool",
            posix_mknod,
        ),
        (
            "function posix_access(string $filename, int $flags = 0): bool",
            posix_access,
        ),
        (
            "function posix_eaccess(string $filename, int $flags = 0): bool",
            posix_eaccess,
        ),
        (
            "function posix_getgrnam(string $name): array|false",
            posix_getgrnam,
        ),
        (
            "function posix_getgrgid(int $group_id): array|false",
            posix_getgrgid,
        ),
        (
            "function posix_getpwnam(string $username): array|false",
            posix_getpwnam,
        ),
        (
            "function posix_getpwuid(int $user_id): array|false",
            posix_getpwuid,
        ),
        (
            "function posix_getrlimit(?int $resource = null): array|false",
            posix_getrlimit,
        ),
        (
            "function posix_setrlimit(int $resource, int $soft_limit, int $hard_limit): bool",
            posix_setrlimit,
        ),
        ("function posix_get_last_error(): int", posix_errno),
        ("function posix_errno(): int", posix_errno),
        (
            "function posix_strerror(int $error_code): string",
            posix_strerror,
        ),
        (
            "function posix_initgroups(string $username, int $group_id): bool",
            posix_initgroups,
        ),
        ("function posix_sysconf(int $conf_id): int", posix_sysconf),
        (
            "function posix_pathconf(string $path, int $name): int|false",
            posix_pathconf,
        ),
        (
            "function posix_fpathconf($file_descriptor, int $name): int|false",
            posix_fpathconf,
        ),
    ],
    constants: &[
        ("POSIX_F_OK", ConstVal::Int(libc::F_OK as i64)),
        ("POSIX_X_OK", ConstVal::Int(libc::X_OK as i64)),
        ("POSIX_W_OK", ConstVal::Int(libc::W_OK as i64)),
        ("POSIX_R_OK", ConstVal::Int(libc::R_OK as i64)),
        ("POSIX_S_IFREG", ConstVal::Int(libc::S_IFREG as i64)),
        ("POSIX_S_IFCHR", ConstVal::Int(libc::S_IFCHR as i64)),
        ("POSIX_S_IFBLK", ConstVal::Int(libc::S_IFBLK as i64)),
        ("POSIX_S_IFIFO", ConstVal::Int(libc::S_IFIFO as i64)),
        ("POSIX_S_IFSOCK", ConstVal::Int(libc::S_IFSOCK as i64)),
        ("POSIX_RLIMIT_AS", ConstVal::Int(libc::RLIMIT_AS as i64)),
        ("POSIX_RLIMIT_CORE", ConstVal::Int(libc::RLIMIT_CORE as i64)),
        ("POSIX_RLIMIT_CPU", ConstVal::Int(libc::RLIMIT_CPU as i64)),
        ("POSIX_RLIMIT_DATA", ConstVal::Int(libc::RLIMIT_DATA as i64)),
        (
            "POSIX_RLIMIT_FSIZE",
            ConstVal::Int(libc::RLIMIT_FSIZE as i64),
        ),
        (
            "POSIX_RLIMIT_LOCKS",
            ConstVal::Int(libc::RLIMIT_LOCKS as i64),
        ),
        (
            "POSIX_RLIMIT_MEMLOCK",
            ConstVal::Int(libc::RLIMIT_MEMLOCK as i64),
        ),
        (
            "POSIX_RLIMIT_MSGQUEUE",
            ConstVal::Int(libc::RLIMIT_MSGQUEUE as i64),
        ),
        ("POSIX_RLIMIT_NICE", ConstVal::Int(libc::RLIMIT_NICE as i64)),
        (
            "POSIX_RLIMIT_NOFILE",
            ConstVal::Int(libc::RLIMIT_NOFILE as i64),
        ),
        (
            "POSIX_RLIMIT_NPROC",
            ConstVal::Int(libc::RLIMIT_NPROC as i64),
        ),
        ("POSIX_RLIMIT_RSS", ConstVal::Int(libc::RLIMIT_RSS as i64)),
        (
            "POSIX_RLIMIT_RTPRIO",
            ConstVal::Int(libc::RLIMIT_RTPRIO as i64),
        ),
        (
            "POSIX_RLIMIT_RTTIME",
            ConstVal::Int(libc::RLIMIT_RTTIME as i64),
        ),
        (
            "POSIX_RLIMIT_SIGPENDING",
            ConstVal::Int(libc::RLIMIT_SIGPENDING as i64),
        ),
        (
            "POSIX_RLIMIT_STACK",
            ConstVal::Int(libc::RLIMIT_STACK as i64),
        ),
        ("POSIX_RLIMIT_INFINITY", ConstVal::Int(-1)),
        ("POSIX_SC_ARG_MAX", ConstVal::Int(libc::_SC_ARG_MAX as i64)),
        (
            "POSIX_SC_CHILD_MAX",
            ConstVal::Int(libc::_SC_CHILD_MAX as i64),
        ),
        ("POSIX_SC_CLK_TCK", ConstVal::Int(libc::_SC_CLK_TCK as i64)),
        (
            "POSIX_SC_PAGESIZE",
            ConstVal::Int(libc::_SC_PAGESIZE as i64),
        ),
        (
            "POSIX_SC_NPROCESSORS_CONF",
            ConstVal::Int(libc::_SC_NPROCESSORS_CONF as i64),
        ),
        (
            "POSIX_SC_NPROCESSORS_ONLN",
            ConstVal::Int(libc::_SC_NPROCESSORS_ONLN as i64),
        ),
        (
            "POSIX_PC_LINK_MAX",
            ConstVal::Int(libc::_PC_LINK_MAX as i64),
        ),
        (
            "POSIX_PC_MAX_CANON",
            ConstVal::Int(libc::_PC_MAX_CANON as i64),
        ),
        (
            "POSIX_PC_MAX_INPUT",
            ConstVal::Int(libc::_PC_MAX_INPUT as i64),
        ),
        (
            "POSIX_PC_NAME_MAX",
            ConstVal::Int(libc::_PC_NAME_MAX as i64),
        ),
        (
            "POSIX_PC_PATH_MAX",
            ConstVal::Int(libc::_PC_PATH_MAX as i64),
        ),
        (
            "POSIX_PC_PIPE_BUF",
            ConstVal::Int(libc::_PC_PIPE_BUF as i64),
        ),
        (
            "POSIX_PC_CHOWN_RESTRICTED",
            ConstVal::Int(libc::_PC_CHOWN_RESTRICTED as i64),
        ),
        (
            "POSIX_PC_NO_TRUNC",
            ConstVal::Int(libc::_PC_NO_TRUNC as i64),
        ),
        (
            "POSIX_PC_ALLOC_SIZE_MIN",
            ConstVal::Int(libc::_PC_ALLOC_SIZE_MIN as i64),
        ),
        (
            "POSIX_PC_SYMLINK_MAX",
            ConstVal::Int(libc::_PC_SYMLINK_MAX as i64),
        ),
        (
            "POSIX_SC_OPEN_MAX",
            ConstVal::Int(libc::_SC_OPEN_MAX as i64),
        ),
    ],
    listed: true,
    ..Module::EMPTY
};

thread_local! {
    /// posix_get_last_error(): the errno of the last failed call.
    static LAST_ERROR: Cell<i32> = const { Cell::new(0) };
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn set_error(e: i32) {
    LAST_ERROR.with(|c| c.set(e));
}

/// Records errno and returns false.
fn fail() -> R<Value> {
    set_error(errno());
    Ok(Value::Bool(false))
}

fn bool_result(ok: bool) -> R<Value> {
    if ok { Ok(Value::Bool(true)) } else { fail() }
}

fn s(v: &[u8]) -> Value {
    Value::Str(PhpStr::from(v))
}

fn k(n: &str) -> Key {
    Key::Str(PhpStr::from(n))
}

/// The C string of a path or name argument; `None` when it has a NUL.
fn cstr(b: &[u8]) -> Option<CString> {
    CString::new(b).ok()
}

fn path_arg(vm: &mut Vm, a: &Args, i: usize, name: &str, nonempty: bool) -> R<Option<CString>> {
    let p = a.str(i);
    if nonempty && p.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${}) must not be empty", i + 1, name),
        );
    }
    match cstr(&p) {
        Some(c) => Ok(Some(c)),
        None => vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} (${}) must not contain any null bytes",
                i + 1,
                name
            ),
        ),
    }
}

/// Checks that an argument fits `lo..=hi` (ValueError otherwise).
fn ranged(vm: &mut Vm, a: &Args, i: usize, name: &str, lo: i64, hi: i64) -> R<i64> {
    let v = a.int(i);
    if v < lo || v > hi {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} (${}) must be between {} and {}",
                i + 1,
                name,
                lo,
                hi
            ),
        );
    }
    Ok(v)
}

/// The `$file_descriptor` parameter (int|resource): a stream gives its
/// descriptor; anything else goes through int juggling with PHP's
/// deprecations, and bad values warn (observed on PHP 8.5, tests
/// posix_isatty_manual_zpp.phpt and posix_isatty_value_errors.phpt).
/// `None` means the function returns false.
fn fd_arg(vm: &mut Vm, a: &Args, throw: bool) -> R<Option<i32>> {
    use super::file::Backing;
    let v = a.val(0);
    let n = match &v {
        Value::Resource(_) => {
            let r = super::file::with_stream(vm, a, 0, |_vm, st| {
                Ok(match &st.backing {
                    Backing::Stdin => Ok(0),
                    Backing::Stdout => Ok(1),
                    Backing::Stderr => Ok(2),
                    Backing::File(f) => {
                        use std::os::unix::io::AsRawFd;
                        Ok(f.as_raw_fd())
                    }
                    Backing::Native(n) => n.raw_fd().ok_or(st.stream_type),
                    _ => Err(st.stream_type),
                })
            })?;
            return match r {
                Ok(fd) => Ok(Some(fd)),
                Err(ty) => {
                    vm.fwarn(a, &format!("Could not use stream of type '{}'", ty))?;
                    Ok(None)
                }
            };
        }
        Value::Null => {
            vm.fdeprecated(
                a,
                "Passing null to parameter #1 ($file_descriptor) of type int is deprecated",
            )?;
            0
        }
        Value::Bool(b) => *b as i64,
        Value::Int(i) => *i,
        Value::Float(f) if f.is_finite() && (-9.2e18..9.2e18).contains(f) => {
            if f.fract() != 0.0 {
                vm.deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_runtime::convert::float_repr(*f)
                ))?;
            }
            *f as i64
        }
        Value::Str(st) => {
            use goro_runtime::numeric::{Numericness, parse_numeric};
            let p = parse_numeric(st);
            match (p.kind, p.num) {
                (Numericness::Numeric, goro_runtime::Number::Int(n)) => n,
                (Numericness::Numeric, goro_runtime::Number::Float(f))
                    if f.is_finite() && (-9.2e18..9.2e18).contains(&f) =>
                {
                    if f.fract() != 0.0 {
                        vm.deprecated(&format!(
                            "Implicit conversion from float-string \"{}\" to int loses precision",
                            st.to_string_lossy()
                        ))?;
                    }
                    f as i64
                }
                _ => return fd_type_warning(vm, a, &v, throw),
            }
        }
        _ => return fd_type_warning(vm, a, &v, throw),
    };
    if !(0..=i32::MAX as i64).contains(&n) {
        vm.fwarn(
            a,
            &format!(
                "Argument #1 ($file_descriptor) must be between 0 and {}",
                i32::MAX
            ),
        )?;
        set_error(libc::EBADF);
        return Ok(None);
    }
    Ok(Some(n as i32))
}

/// posix_fpathconf() throws the TypeError the others only warn about
/// (tests posix_fpathconf.phpt, posix_isatty_manual_zpp.phpt).
fn fd_type_warning(vm: &mut Vm, a: &Args, v: &Value, throw: bool) -> R<Option<i32>> {
    let msg = format!(
        "Argument #1 ($file_descriptor) must be of type int|resource, {} given",
        Vm::type_name_given(v)
    );
    if throw {
        return vm.fthrow(a, "TypeError", &msg);
    }
    vm.fwarn(a, &msg)?;
    // posix_ttyname() also tries the int cast of objects without one
    // (test posix_ttyname_manual_zpp.phpt).
    if let Value::Object(o) = v
        && a.fname() == "posix_ttyname"
    {
        let name = vm.class_of(o).name.to_string_lossy();
        if !name.eq_ignore_ascii_case("GMP") {
            vm.warn(&format!(
                "Object of class {} could not be converted to int",
                name
            ))?;
        }
    }
    Ok(None)
}

// ---- processes ----

/// posix_kill(): the pid must fit a pid_t (the message's lower bound is
/// printed as PHP 8.5 prints it, test posix_kill_pidoverflow.phpt).
fn posix_kill(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = a.int(0);
    if pid < i32::MIN as i64 || pid > i32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($process_id) must be between {} and {}",
                (i32::MIN as i64).unsigned_abs(),
                i32::MAX
            ),
        );
    }
    let sig = a.int(1) as libc::c_int;
    // SAFETY: kill() has no memory effects.
    bool_result(unsafe { libc::kill(pid as libc::pid_t, sig) } == 0)
}

fn posix_getpid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getpid() } as i64))
}

fn posix_getppid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getppid() } as i64))
}

fn posix_getuid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getuid() } as i64))
}

fn posix_geteuid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::geteuid() } as i64))
}

fn posix_getgid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getgid() } as i64))
}

fn posix_getegid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getegid() } as i64))
}

fn posix_setuid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    bool_result(unsafe { libc::setuid(a.int(0) as libc::uid_t) } == 0)
}

fn posix_seteuid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    bool_result(unsafe { libc::seteuid(a.int(0) as libc::uid_t) } == 0)
}

fn posix_setgid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    bool_result(unsafe { libc::setgid(a.int(0) as libc::gid_t) } == 0)
}

fn posix_setegid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    bool_result(unsafe { libc::setegid(a.int(0) as libc::gid_t) } == 0)
}

fn posix_getgroups(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // SAFETY: a null buffer with size 0 asks for the count.
    let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if n < 0 {
        return fail();
    }
    let mut buf = vec![0 as libc::gid_t; n as usize + 1];
    let n = unsafe { libc::getgroups(buf.len() as libc::c_int, buf.as_mut_ptr()) };
    if n < 0 {
        return fail();
    }
    let vals: Vec<Value> = buf[..n as usize]
        .iter()
        .map(|&g| Value::Int(g as i64))
        .collect();
    Ok(Value::Array(PhpArray::from_values(vals)))
}

fn posix_getlogin(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // SAFETY: getlogin() returns a static string or null.
    let p = unsafe { libc::getlogin() };
    if p.is_null() {
        return fail();
    }
    Ok(s(unsafe { CStr::from_ptr(p) }.to_bytes()))
}

fn posix_getpgrp(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::getpgrp() } as i64))
}

fn posix_setsid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let r = unsafe { libc::setsid() };
    if r < 0 {
        set_error(errno());
    }
    Ok(Value::Int(r as i64))
}

fn posix_setpgid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = ranged(vm, a, 0, "process_id", 0, i32::MAX as i64)?;
    let pgid = ranged(vm, a, 1, "process_group_id", 0, i32::MAX as i64)?;
    bool_result(unsafe { libc::setpgid(pid as libc::pid_t, pgid as libc::pid_t) } == 0)
}

fn posix_getpgid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = unsafe { libc::getpgid(a.int(0) as libc::pid_t) };
    if r < 0 {
        return fail();
    }
    Ok(Value::Int(r as i64))
}

fn posix_getsid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = ranged(vm, a, 0, "process_id", 0, i32::MAX as i64)?;
    let r = unsafe { libc::getsid(pid as libc::pid_t) };
    if r < 0 {
        return fail();
    }
    Ok(Value::Int(r as i64))
}

fn field(b: &[libc::c_char]) -> Value {
    // SAFETY: utsname fields are NUL-terminated.
    s(unsafe { CStr::from_ptr(b.as_ptr()) }.to_bytes())
}

fn posix_uname(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } < 0 {
        return fail();
    }
    let mut arr = PhpArray::new();
    arr.insert(k("sysname"), field(&u.sysname));
    arr.insert(k("nodename"), field(&u.nodename));
    arr.insert(k("release"), field(&u.release));
    arr.insert(k("version"), field(&u.version));
    arr.insert(k("machine"), field(&u.machine));
    arr.insert(k("domainname"), field(&u.domainname));
    Ok(Value::Array(arr))
}

fn posix_times(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut t: libc::tms = unsafe { std::mem::zeroed() };
    let ticks = unsafe { libc::times(&mut t) };
    if ticks == !0 {
        return fail();
    }
    let mut arr = PhpArray::new();
    arr.insert(k("ticks"), Value::Int(ticks as i64));
    arr.insert(k("utime"), Value::Int(t.tms_utime as i64));
    arr.insert(k("stime"), Value::Int(t.tms_stime as i64));
    arr.insert(k("cutime"), Value::Int(t.tms_cutime as i64));
    arr.insert(k("cstime"), Value::Int(t.tms_cstime as i64));
    Ok(Value::Array(arr))
}

fn posix_ctermid(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut buf = [0 as libc::c_char; 64];
    let p = unsafe { libc::ctermid(buf.as_mut_ptr()) };
    if p.is_null() {
        return fail();
    }
    Ok(field(&buf))
}

fn posix_ttyname(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(fd) = fd_arg(vm, a, false)? else {
        return Ok(Value::Bool(false));
    };
    let mut buf = [0 as libc::c_char; 256];
    let r = unsafe { libc::ttyname_r(fd, buf.as_mut_ptr(), buf.len()) };
    if r != 0 {
        set_error(r);
        return Ok(Value::Bool(false));
    }
    Ok(field(&buf))
}

fn posix_isatty(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(fd) = fd_arg(vm, a, false)? else {
        return Ok(Value::Bool(false));
    };
    bool_result(unsafe { libc::isatty(fd) } == 1)
}

fn posix_getcwd(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    use std::os::unix::ffi::OsStrExt;
    match std::env::current_dir() {
        Ok(p) => Ok(s(p.as_os_str().as_bytes())),
        Err(e) => {
            set_error(e.raw_os_error().unwrap_or(0));
            Ok(Value::Bool(false))
        }
    }
}

// ---- files ----

fn posix_mkfifo(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(p) = path_arg(vm, a, 0, "filename", false)? else {
        return Ok(Value::Bool(false));
    };
    bool_result(unsafe { libc::mkfifo(p.as_ptr(), a.int(1) as libc::mode_t) } == 0)
}

fn posix_mknod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(p) = path_arg(vm, a, 0, "filename", false)? else {
        return Ok(Value::Bool(false));
    };
    let mode = a.int(1);
    let major = a.int_or(2, 0);
    let minor = a.int_or(3, 0);
    let fmt = mode & libc::S_IFMT as i64;
    let mut dev: libc::dev_t = 0;
    if fmt == libc::S_IFCHR as i64 || fmt == libc::S_IFBLK as i64 {
        if major == 0 {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #3 ($major) cannot be 0 for the POSIX_S_IFCHR and POSIX_S_IFBLK modes",
            );
        }
        dev = libc::makedev(major as u32, minor as u32);
    }
    bool_result(unsafe { libc::mknod(p.as_ptr(), mode as libc::mode_t, dev) } == 0)
}

fn posix_access(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(p) = path_arg(vm, a, 0, "filename", false)? else {
        return Ok(Value::Bool(false));
    };
    if p.as_bytes().is_empty() {
        // Observed on PHP 8.5: an empty path fails with EIO.
        set_error(libc::EIO);
        return Ok(Value::Bool(false));
    }
    bool_result(unsafe { libc::access(p.as_ptr(), a.int_or(1, 0) as libc::c_int) } == 0)
}

fn posix_eaccess(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // A path too long to resolve counts as empty (test posix_eaccess.phpt).
    if a.str(0).len() >= libc::PATH_MAX as usize {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) must not be empty");
    }
    let Some(p) = path_arg(vm, a, 0, "filename", true)? else {
        return Ok(Value::Bool(false));
    };
    bool_result(unsafe { libc::eaccess(p.as_ptr(), a.int_or(1, 0) as libc::c_int) } == 0)
}

// ---- users and groups ----

fn cbytes(p: *const libc::c_char) -> Value {
    if p.is_null() {
        return s(b"");
    }
    s(unsafe { CStr::from_ptr(p) }.to_bytes())
}

fn group_array(g: *const libc::group) -> Value {
    // SAFETY: a non-null result of getgrnam()/getgrgid().
    let g = unsafe { &*g };
    let mut members = PhpArray::new();
    let mut m = g.gr_mem;
    if !m.is_null() {
        unsafe {
            while !(*m).is_null() {
                let _ = members.push(cbytes(*m));
                m = m.add(1);
            }
        }
    }
    let mut arr = PhpArray::new();
    arr.insert(k("name"), cbytes(g.gr_name));
    arr.insert(k("passwd"), cbytes(g.gr_passwd));
    arr.insert(k("members"), Value::Array(members));
    arr.insert(k("gid"), Value::Int(g.gr_gid as i64));
    Value::Array(arr)
}

fn passwd_array(p: *const libc::passwd) -> Value {
    // SAFETY: a non-null result of getpwnam()/getpwuid().
    let p = unsafe { &*p };
    let mut arr = PhpArray::new();
    arr.insert(k("name"), cbytes(p.pw_name));
    arr.insert(k("passwd"), cbytes(p.pw_passwd));
    arr.insert(k("uid"), Value::Int(p.pw_uid as i64));
    arr.insert(k("gid"), Value::Int(p.pw_gid as i64));
    arr.insert(k("gecos"), cbytes(p.pw_gecos));
    arr.insert(k("dir"), cbytes(p.pw_dir));
    arr.insert(k("shell"), cbytes(p.pw_shell));
    Value::Array(arr)
}

/// A lookup that returns null: errno (0 for "not found") is recorded.
fn lookup_failed() -> R<Value> {
    set_error(errno());
    Ok(Value::Bool(false))
}

fn clear_errno() {
    // SAFETY: errno is thread-local.
    unsafe { *libc::__errno_location() = 0 };
}

fn posix_getgrnam(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(n) = cstr(&a.str(0)) else {
        return Ok(Value::Bool(false));
    };
    clear_errno();
    let g = unsafe { libc::getgrnam(n.as_ptr()) };
    if g.is_null() {
        return lookup_failed();
    }
    Ok(group_array(g))
}

fn posix_getgrgid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let gid = a.int(0);
    if !(0..=u32::MAX as i64 - 1).contains(&gid) && gid != -1 {
        // Observed: unrepresentable ids find nothing.
        set_error(0);
        return Ok(Value::Bool(false));
    }
    clear_errno();
    let g = unsafe { libc::getgrgid(gid as libc::gid_t) };
    if g.is_null() {
        return lookup_failed();
    }
    Ok(group_array(g))
}

fn posix_getpwnam(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(n) = cstr(&a.str(0)) else {
        return Ok(Value::Bool(false));
    };
    clear_errno();
    let p = unsafe { libc::getpwnam(n.as_ptr()) };
    if p.is_null() {
        return lookup_failed();
    }
    Ok(passwd_array(p))
}

fn posix_getpwuid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let uid = a.int(0);
    if !(0..=u32::MAX as i64 - 1).contains(&uid) && uid != -1 {
        set_error(0);
        return Ok(Value::Bool(false));
    }
    clear_errno();
    let p = unsafe { libc::getpwuid(uid as libc::uid_t) };
    if p.is_null() {
        return lookup_failed();
    }
    Ok(passwd_array(p))
}

fn posix_initgroups(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    if name.is_empty() {
        return Ok(Value::Bool(false));
    }
    let Some(n) = cstr(&name) else {
        return Ok(Value::Bool(false));
    };
    bool_result(unsafe { libc::initgroups(n.as_ptr(), a.int(1) as libc::gid_t) } == 0)
}

// ---- limits ----

/// posix_getrlimit()'s names, in its order (observed on PHP 8.5).
const RLIMITS: [(&str, libc::__rlimit_resource_t); 10] = [
    ("core", libc::RLIMIT_CORE),
    ("data", libc::RLIMIT_DATA),
    ("stack", libc::RLIMIT_STACK),
    ("totalmem", libc::RLIMIT_AS),
    ("rss", libc::RLIMIT_RSS),
    ("maxproc", libc::RLIMIT_NPROC),
    ("memlock", libc::RLIMIT_MEMLOCK),
    ("cpu", libc::RLIMIT_CPU),
    ("filesize", libc::RLIMIT_FSIZE),
    ("openfiles", libc::RLIMIT_NOFILE),
];

fn limit_value(v: libc::rlim_t) -> Value {
    if v == libc::RLIM_INFINITY {
        Value::Str(PhpStr::from("unlimited"))
    } else {
        Value::Int(v as i64)
    }
}

fn getrlimit(res: libc::__rlimit_resource_t) -> Option<libc::rlimit> {
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(res, &mut rl) } < 0 {
        set_error(errno());
        return None;
    }
    Some(rl)
}

fn posix_getrlimit(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(res) = a.opt_int(0) {
        if !(0..=u32::MAX as i64).contains(&res) {
            set_error(libc::EINVAL);
            return Ok(Value::Bool(false));
        }
        let Some(rl) = getrlimit(res as libc::__rlimit_resource_t) else {
            return Ok(Value::Bool(false));
        };
        return Ok(Value::Array(PhpArray::from_values(vec![
            limit_value(rl.rlim_cur),
            limit_value(rl.rlim_max),
        ])));
    }
    let mut arr = PhpArray::new();
    for (name, res) in RLIMITS {
        let Some(rl) = getrlimit(res) else {
            return Ok(Value::Bool(false));
        };
        arr.insert(k(&format!("soft {}", name)), limit_value(rl.rlim_cur));
        arr.insert(k(&format!("hard {}", name)), limit_value(rl.rlim_max));
    }
    Ok(Value::Array(arr))
}

/// posix_setrlimit(): -1 is RLIM_INFINITY; the checks and their messages
/// from test posix_setrlimit.phpt.
fn posix_setrlimit(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let res = a.int(0);
    let soft = a.int(1);
    let hard = a.int(2);
    if soft < -1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($soft_limit) must be greater or equal to -1",
        );
    }
    if hard < -1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($hard_limit) must be greater or equal to -1",
        );
    }
    if hard != -1 && soft > hard {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #2 ($soft_limit) must be lower or equal to {}",
                hard
            ),
        );
    }
    if !(0..=u32::MAX as i64).contains(&res) {
        set_error(libc::EINVAL);
        return Ok(Value::Bool(false));
    }
    let lim = |v: i64| {
        if v == -1 {
            libc::RLIM_INFINITY
        } else {
            v as libc::rlim_t
        }
    };
    let rl = libc::rlimit {
        rlim_cur: lim(soft),
        rlim_max: lim(hard),
    };
    bool_result(unsafe { libc::setrlimit(res as libc::__rlimit_resource_t, &rl) } == 0)
}

// ---- errors ----

fn posix_errno(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(LAST_ERROR.with(|c| c.get()) as i64))
}

fn posix_strerror(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let e = a.int(0) as i32;
    Ok(Value::Str(PhpStr::from(super::exec::strerror(e))))
}

// ---- configuration ----

fn posix_sysconf(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.int(0);
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&n) {
        return Ok(Value::Int(-1));
    }
    Ok(Value::Int(unsafe { libc::sysconf(n as libc::c_int) } as i64))
}

fn posix_pathconf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(p) = path_arg(vm, a, 0, "path", true)? else {
        return Ok(Value::Bool(false));
    };
    let n = a.int(1);
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&n) {
        set_error(libc::EINVAL);
        return Ok(Value::Bool(false));
    }
    clear_errno();
    let r = unsafe { libc::pathconf(p.as_ptr(), n as libc::c_int) };
    if r < 0 {
        return fail();
    }
    Ok(Value::Int(r as i64))
}

fn posix_fpathconf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(fd) = fd_arg(vm, a, true)? else {
        return Ok(Value::Bool(false));
    };
    let n = a.int(1);
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&n) {
        set_error(libc::EINVAL);
        return Ok(Value::Bool(false));
    }
    clear_errno();
    let r = unsafe { libc::fpathconf(fd, n as libc::c_int) };
    if r < 0 {
        return fail();
    }
    Ok(Value::Int(r as i64))
}
