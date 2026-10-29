//! ext/pcntl (PHP manual: Process Control): fork, wait, exec, signals
//! (handlers, dispatch, async delivery, masks, synchronous waits),
//! priorities, CPU affinity and namespaces. Messages, argument checks and
//! array layouts follow the ext/pcntl tests and PHP 8.5's observed output.
//!
//! Signals are caught by a C handler that queues them (with their
//! siginfo) in a lock-free ring; they reach PHP handlers through
//! pcntl_signal_dispatch(), ticks, or after each internal function call
//! once pcntl_async_signals(true) is on ([`after_native_call`]).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicUsize, Ordering};

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

macro_rules! c {
    ($($name:literal = $v:expr),* $(,)?) => {
        &[$(($name, ConstVal::Int($v as i64)),)*]
    };
}

pub static MODULE: Module = Module {
    name: "pcntl",
    functions: &[
        ("function pcntl_fork(): int", pcntl_fork),
        (
            "function pcntl_waitpid(int $process_id, &$status, int $flags = 0, &$resource_usage = []): int",
            pcntl_waitpid,
        ),
        (
            "function pcntl_waitid(int $idtype = P_ALL, ?int $id = null, &$info = [], int $flags = WEXITED, &$resource_usage = []): bool",
            pcntl_waitid,
        ),
        (
            "function pcntl_wait(&$status, int $flags = 0, &$resource_usage = []): int",
            pcntl_wait,
        ),
        (
            "function pcntl_signal(int $signal, $handler, bool $restart_syscalls = true): bool",
            pcntl_signal,
        ),
        (
            "function pcntl_signal_get_handler(int $signal): mixed",
            pcntl_signal_get_handler,
        ),
        ("function pcntl_signal_dispatch(): bool", pcntl_signal_dispatch),
        (
            "function pcntl_sigprocmask(int $mode, array $signals, &$old_signals = null): bool",
            pcntl_sigprocmask,
        ),
        (
            "function pcntl_sigwaitinfo(array $signals, &$info = []): int|false",
            pcntl_sigwaitinfo,
        ),
        (
            "function pcntl_sigtimedwait(array $signals, &$info = [], int $seconds = 0, int $nanoseconds = 0): int|false",
            pcntl_sigtimedwait,
        ),
        (
            "function pcntl_wifexited(int $status): bool",
            pcntl_wifexited,
        ),
        (
            "function pcntl_wifstopped(int $status): bool",
            pcntl_wifstopped,
        ),
        (
            "function pcntl_wifcontinued(int $status): bool",
            pcntl_wifcontinued,
        ),
        (
            "function pcntl_wifsignaled(int $status): bool",
            pcntl_wifsignaled,
        ),
        (
            "function pcntl_wexitstatus(int $status): int|false",
            pcntl_wexitstatus,
        ),
        (
            "function pcntl_wtermsig(int $status): int|false",
            pcntl_wtermsig,
        ),
        (
            "function pcntl_wstopsig(int $status): int|false",
            pcntl_wstopsig,
        ),
        (
            "function pcntl_exec(string $path, array $args = [], array $env_vars = []): false",
            pcntl_exec,
        ),
        ("function pcntl_alarm(int $seconds): int", pcntl_alarm),
        ("function pcntl_get_last_error(): int", pcntl_errno),
        ("function pcntl_errno(): int", pcntl_errno),
        (
            "function pcntl_getpriority(?int $process_id = null, int $mode = PRIO_PROCESS): int|false",
            pcntl_getpriority,
        ),
        (
            "function pcntl_setpriority(int $priority, ?int $process_id = null, int $mode = PRIO_PROCESS): bool",
            pcntl_setpriority,
        ),
        (
            "function pcntl_strerror(int $error_code): string",
            pcntl_strerror,
        ),
        (
            "function pcntl_async_signals(?bool $enable = null): bool",
            pcntl_async_signals,
        ),
        ("function pcntl_unshare(int $flags): bool", pcntl_unshare),
        (
            "function pcntl_getcpuaffinity(?int $process_id = null): array|false",
            pcntl_getcpuaffinity,
        ),
        (
            "function pcntl_setcpuaffinity(?int $process_id = null, array $cpu_ids = []): bool",
            pcntl_setcpuaffinity,
        ),
        ("function pcntl_getcpu(): int", pcntl_getcpu),
    ],
    constants: c![
        "WNOHANG" = libc::WNOHANG,
        "WUNTRACED" = libc::WUNTRACED,
        "WCONTINUED" = libc::WCONTINUED,
        "WEXITED" = libc::WEXITED,
        "WSTOPPED" = libc::WSTOPPED,
        "WNOWAIT" = libc::WNOWAIT,
        "P_ALL" = libc::P_ALL,
        "P_PID" = libc::P_PID,
        "P_PGID" = libc::P_PGID,
        "P_PIDFD" = 3,
        "SIG_IGN" = 1,
        "SIG_DFL" = 0,
        "SIG_ERR" = -1,
        "SIGHUP" = libc::SIGHUP,
        "SIGINT" = libc::SIGINT,
        "SIGQUIT" = libc::SIGQUIT,
        "SIGILL" = libc::SIGILL,
        "SIGTRAP" = libc::SIGTRAP,
        "SIGABRT" = libc::SIGABRT,
        "SIGIOT" = libc::SIGIOT,
        "SIGBUS" = libc::SIGBUS,
        "SIGFPE" = libc::SIGFPE,
        "SIGKILL" = libc::SIGKILL,
        "SIGUSR1" = libc::SIGUSR1,
        "SIGSEGV" = libc::SIGSEGV,
        "SIGUSR2" = libc::SIGUSR2,
        "SIGPIPE" = libc::SIGPIPE,
        "SIGALRM" = libc::SIGALRM,
        "SIGTERM" = libc::SIGTERM,
        "SIGSTKFLT" = libc::SIGSTKFLT,
        "SIGCLD" = libc::SIGCHLD,
        "SIGCHLD" = libc::SIGCHLD,
        "SIGCONT" = libc::SIGCONT,
        "SIGSTOP" = libc::SIGSTOP,
        "SIGTSTP" = libc::SIGTSTP,
        "SIGTTIN" = libc::SIGTTIN,
        "SIGTTOU" = libc::SIGTTOU,
        "SIGURG" = libc::SIGURG,
        "SIGXCPU" = libc::SIGXCPU,
        "SIGXFSZ" = libc::SIGXFSZ,
        "SIGVTALRM" = libc::SIGVTALRM,
        "SIGPROF" = libc::SIGPROF,
        "SIGWINCH" = libc::SIGWINCH,
        "SIGPOLL" = libc::SIGPOLL,
        "SIGIO" = libc::SIGIO,
        "SIGPWR" = libc::SIGPWR,
        "SIGSYS" = libc::SIGSYS,
        "SIGBABY" = libc::SIGSYS,
        "SIGRTMIN" = 34,
        "SIGRTMAX" = 64,
        "PRIO_PGRP" = libc::PRIO_PGRP,
        "PRIO_USER" = libc::PRIO_USER,
        "PRIO_PROCESS" = libc::PRIO_PROCESS,
        "SIG_BLOCK" = libc::SIG_BLOCK,
        "SIG_UNBLOCK" = libc::SIG_UNBLOCK,
        "SIG_SETMASK" = libc::SIG_SETMASK,
        "SI_USER" = 0,
        "SI_KERNEL" = 128,
        "SI_QUEUE" = -1,
        "SI_TIMER" = -2,
        "SI_MESGQ" = -3,
        "SI_ASYNCIO" = -4,
        "SI_SIGIO" = -5,
        "SI_TKILL" = -6,
        "CLD_EXITED" = 1,
        "CLD_KILLED" = 2,
        "CLD_DUMPED" = 3,
        "CLD_TRAPPED" = 4,
        "CLD_STOPPED" = 5,
        "CLD_CONTINUED" = 6,
        "TRAP_BRKPT" = 1,
        "TRAP_TRACE" = 2,
        "POLL_IN" = 1,
        "POLL_OUT" = 2,
        "POLL_MSG" = 3,
        "POLL_ERR" = 4,
        "POLL_PRI" = 5,
        "POLL_HUP" = 6,
        "ILL_ILLOPC" = 1,
        "ILL_ILLOPN" = 2,
        "ILL_ILLADR" = 3,
        "ILL_ILLTRP" = 4,
        "ILL_PRVOPC" = 5,
        "ILL_PRVREG" = 6,
        "ILL_COPROC" = 7,
        "ILL_BADSTK" = 8,
        "FPE_INTDIV" = 1,
        "FPE_INTOVF" = 2,
        "FPE_FLTDIV" = 3,
        "FPE_FLTOVF" = 4,
        "FPE_FLTUND" = 5,
        "FPE_FLTRES" = 6,
        "FPE_FLTINV" = 7,
        "FPE_FLTSUB" = 8,
        "SEGV_MAPERR" = 1,
        "SEGV_ACCERR" = 2,
        "BUS_ADRALN" = 1,
        "BUS_ADRERR" = 2,
        "BUS_OBJERR" = 3,
        "CLONE_NEWNS" = libc::CLONE_NEWNS,
        "CLONE_NEWIPC" = libc::CLONE_NEWIPC,
        "CLONE_NEWUTS" = libc::CLONE_NEWUTS,
        "CLONE_NEWNET" = libc::CLONE_NEWNET,
        "CLONE_NEWPID" = libc::CLONE_NEWPID,
        "CLONE_NEWUSER" = libc::CLONE_NEWUSER,
        "CLONE_NEWCGROUP" = libc::CLONE_NEWCGROUP,
        "PCNTL_EINTR" = libc::EINTR,
        "PCNTL_ECHILD" = libc::ECHILD,
        "PCNTL_EINVAL" = libc::EINVAL,
        "PCNTL_EAGAIN" = libc::EAGAIN,
        "PCNTL_ESRCH" = libc::ESRCH,
        "PCNTL_EACCES" = libc::EACCES,
        "PCNTL_EPERM" = libc::EPERM,
        "PCNTL_ENOMEM" = libc::ENOMEM,
        "PCNTL_E2BIG" = libc::E2BIG,
        "PCNTL_EFAULT" = libc::EFAULT,
        "PCNTL_EIO" = libc::EIO,
        "PCNTL_EISDIR" = libc::EISDIR,
        "PCNTL_ELIBBAD" = libc::ELIBBAD,
        "PCNTL_ELOOP" = libc::ELOOP,
        "PCNTL_EMFILE" = libc::EMFILE,
        "PCNTL_ENAMETOOLONG" = libc::ENAMETOOLONG,
        "PCNTL_ENFILE" = libc::ENFILE,
        "PCNTL_ENOENT" = libc::ENOENT,
        "PCNTL_ENOEXEC" = libc::ENOEXEC,
        "PCNTL_ENOTDIR" = libc::ENOTDIR,
        "PCNTL_ETXTBSY" = libc::ETXTBSY,
        "PCNTL_ENOSPC" = libc::ENOSPC,
        "PCNTL_EUSERS" = libc::EUSERS,
    ],
    listed: true,
    ..Module::EMPTY
};

/// Blocks the catchable signals in the calling thread, so that signals
/// sent to the process reach the engine thread (the binary's main thread
/// only waits for it).
pub fn block_signals_in_this_thread() {
    // SAFETY: a filled signal set for pthread_sigmask.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut set);
        for s in [libc::SIGSEGV, libc::SIGBUS, libc::SIGFPE, libc::SIGILL] {
            libc::sigdelset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Highest signal number + 1 (Linux).
const NSIG: i64 = 65;

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

static LAST_ERROR: AtomicI32 = AtomicI32::new(0);

fn set_error(e: i32) {
    LAST_ERROR.store(e, Ordering::Relaxed);
}

fn k(n: &str) -> Key {
    Key::Str(PhpStr::from(n))
}

// ---- the signal queue ----

/// One caught signal and the siginfo fields PHP reports.
struct Slot {
    state: AtomicU32,
    signo: AtomicI32,
    errno: AtomicI32,
    code: AtomicI32,
    pid: AtomicI32,
    uid: AtomicU32,
    status: AtomicI32,
    utime: AtomicI64,
    stime: AtomicI64,
    addr: AtomicUsize,
}

impl Slot {
    const fn new() -> Slot {
        Slot {
            state: AtomicU32::new(0),
            signo: AtomicI32::new(0),
            errno: AtomicI32::new(0),
            code: AtomicI32::new(0),
            pid: AtomicI32::new(0),
            uid: AtomicU32::new(0),
            status: AtomicI32::new(0),
            utime: AtomicI64::new(0),
            stime: AtomicI64::new(0),
            addr: AtomicUsize::new(0),
        }
    }
}

const QLEN: usize = 256;
static QUEUE: [Slot; QLEN] = [const { Slot::new() }; QLEN];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);
/// A signal is queued.
static PENDING: AtomicBool = AtomicBool::new(false);
/// pcntl_async_signals() is on.
static ASYNC: AtomicBool = AtomicBool::new(false);
/// A signal is queued and async delivery is on.
static ASYNC_PENDING: AtomicBool = AtomicBool::new(false);
static DISPATCHING: AtomicBool = AtomicBool::new(false);

/// The siginfo fields of the kinds PHP distinguishes (observed on PHP 8.5:
/// SIGCHLD has status/utime/stime/pid/uid, signals sent by a process
/// pid/uid, the fault signals addr).
#[derive(Default)]
struct Info {
    signo: i32,
    errno: i32,
    code: i32,
    pid: i32,
    uid: u32,
    status: i32,
    utime: i64,
    stime: i64,
    addr: usize,
}

fn info_from(si: &libc::siginfo_t) -> Info {
    // SAFETY: the accessors read the union members of a filled siginfo.
    unsafe {
        let mut i = Info {
            signo: si.si_signo,
            errno: si.si_errno,
            code: si.si_code,
            ..Info::default()
        };
        match si.si_signo {
            libc::SIGCHLD => {
                i.pid = si.si_pid();
                i.uid = si.si_uid();
                i.status = si.si_status();
                i.utime = si.si_utime() as i64;
                i.stime = si.si_stime() as i64;
            }
            libc::SIGILL | libc::SIGFPE | libc::SIGSEGV | libc::SIGBUS => {
                i.addr = si.si_addr() as usize;
            }
            _ => {
                i.pid = si.si_pid();
                i.uid = si.si_uid();
            }
        }
        i
    }
}

fn info_array(i: &Info) -> Value {
    let mut arr = PhpArray::new();
    arr.insert(k("signo"), Value::Int(i.signo as i64));
    arr.insert(k("errno"), Value::Int(i.errno as i64));
    arr.insert(k("code"), Value::Int(i.code as i64));
    match i.signo {
        libc::SIGCHLD => {
            arr.insert(k("status"), Value::Int(i.status as i64));
            arr.insert(k("utime"), Value::Float(i.utime as f64));
            arr.insert(k("stime"), Value::Float(i.stime as f64));
            arr.insert(k("pid"), Value::Int(i.pid as i64));
            arr.insert(k("uid"), Value::Int(i.uid as i64));
        }
        libc::SIGILL | libc::SIGFPE | libc::SIGSEGV | libc::SIGBUS => {
            arr.insert(k("addr"), Value::Int(i.addr as i64));
        }
        _ => {
            arr.insert(k("pid"), Value::Int(i.pid as i64));
            arr.insert(k("uid"), Value::Int(i.uid as i64));
        }
    }
    Value::Array(arr)
}

/// The C signal handler: async-signal-safe (atomics only).
extern "C" fn on_signal(_sig: libc::c_int, si: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
    let saved = errno();
    let idx = TAIL.fetch_add(1, Ordering::AcqRel);
    let slot = &QUEUE[idx % QLEN];
    if slot
        .state
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        // SAFETY: the kernel passes a valid siginfo with SA_SIGINFO.
        let i = info_from(unsafe { &*si });
        slot.signo.store(i.signo, Ordering::Relaxed);
        slot.errno.store(i.errno, Ordering::Relaxed);
        slot.code.store(i.code, Ordering::Relaxed);
        slot.pid.store(i.pid, Ordering::Relaxed);
        slot.uid.store(i.uid, Ordering::Relaxed);
        slot.status.store(i.status, Ordering::Relaxed);
        slot.utime.store(i.utime, Ordering::Relaxed);
        slot.stime.store(i.stime, Ordering::Relaxed);
        slot.addr.store(i.addr, Ordering::Relaxed);
        slot.state.store(2, Ordering::Release);
    } else {
        // Queue full: the occurrence is dropped (its slot is skipped).
        slot.state.store(3, Ordering::Release);
    }
    PENDING.store(true, Ordering::Release);
    if ASYNC.load(Ordering::Relaxed) {
        ASYNC_PENDING.store(true, Ordering::Release);
    }
    // SAFETY: restoring errno for the interrupted code.
    unsafe { *libc::__errno_location() = saved };
}

/// Takes the next queued signal.
fn pop_signal() -> Option<Info> {
    loop {
        let h = HEAD.load(Ordering::Acquire);
        if h >= TAIL.load(Ordering::Acquire) {
            return None;
        }
        let slot = &QUEUE[h % QLEN];
        match slot.state.load(Ordering::Acquire) {
            2 => {
                let i = Info {
                    signo: slot.signo.load(Ordering::Relaxed),
                    errno: slot.errno.load(Ordering::Relaxed),
                    code: slot.code.load(Ordering::Relaxed),
                    pid: slot.pid.load(Ordering::Relaxed),
                    uid: slot.uid.load(Ordering::Relaxed),
                    status: slot.status.load(Ordering::Relaxed),
                    utime: slot.utime.load(Ordering::Relaxed),
                    stime: slot.stime.load(Ordering::Relaxed),
                    addr: slot.addr.load(Ordering::Relaxed),
                };
                slot.state.store(0, Ordering::Release);
                HEAD.store(h + 1, Ordering::Release);
                return Some(i);
            }
            3 => {
                slot.state.store(0, Ordering::Release);
                HEAD.store(h + 1, Ordering::Release);
            }
            // Being written by a handler that interrupted us.
            _ => return None,
        }
    }
}

/// The PHP handlers by signal number.
#[derive(Default)]
struct Handlers {
    map: std::collections::HashMap<i32, Value>,
}

/// Runs the PHP handlers of the queued signals, in arrival order; an
/// exception from a handler stops the dispatch (test
/// pcntl_signal_dispatch_exception.phpt).
pub fn dispatch(vm: &mut Vm) -> R<()> {
    if DISPATCHING.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    PENDING.store(false, Ordering::Release);
    ASYNC_PENDING.store(false, Ordering::Release);
    let mut r = Ok(());
    while let Some(info) = pop_signal() {
        let h = vm.ext::<Handlers>().map.get(&info.signo).cloned();
        let Some(h) = h else { continue };
        if matches!(h, Value::Int(_)) {
            continue;
        }
        let args = vec![Value::Int(info.signo as i64), info_array(&info)];
        if let Err(e) = vm.call_value(&h, args) {
            r = Err(e);
            break;
        }
    }
    DISPATCHING.store(false, Ordering::Release);
    r
}

/// Engine hook after each internal function call: delivers signals when
/// pcntl_async_signals(true) is on (PHP manual: pcntl_async_signals).
#[inline]
pub fn after_native_call(vm: &mut Vm) -> R<()> {
    if ASYNC_PENDING.load(Ordering::Relaxed) {
        return dispatch(vm);
    }
    Ok(())
}

/// Engine hook on `declare(ticks=N)` ticks.
#[inline]
pub fn on_tick(vm: &mut Vm) -> R<()> {
    if PENDING.load(Ordering::Relaxed) {
        return dispatch(vm);
    }
    Ok(())
}

fn pcntl_signal(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let sig = a.int(0);
    if sig < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($signal) must be greater than or equal to 1",
        );
    }
    if sig >= NSIG {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #1 ($signal) must be less than {}", NSIG),
        );
    }
    let h = a.val(1);
    let restart = a.bool_or(2, true);
    let action: libc::sighandler_t = match &h {
        Value::Int(n) => match *n {
            0 => libc::SIG_DFL,
            1 => libc::SIG_IGN,
            _ => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #2 ($handler) must be either SIG_DFL or SIG_IGN when an integer value is given",
                );
            }
        },
        v if vm.is_callable_value(v, false) => on_signal as *const () as libc::sighandler_t,
        v => {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #2 ($handler) must be of type callable|int, {} given",
                    Vm::type_name_given(v)
                ),
            );
        }
    };
    // SAFETY: a zeroed sigaction filled with a valid handler and mask.
    let ok = unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = action;
        sa.sa_flags = libc::SA_SIGINFO | if restart { libc::SA_RESTART } else { 0 };
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(sig as libc::c_int, &sa, std::ptr::null_mut()) == 0
    };
    if !ok {
        // Observed on PHP 8.5 (SIGKILL): a fatal error without prefix.
        return Err(vm.fatal(&format!("Error installing signal handler for {}", sig)));
    }
    vm.ext::<Handlers>().map.insert(sig as i32, h);
    Ok(Value::Bool(true))
}

fn pcntl_signal_get_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let sig = a.int(0);
    if !(1..NSIG).contains(&sig) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #1 ($signal) must be between 1 and {}", NSIG - 1),
        );
    }
    Ok(vm
        .ext::<Handlers>()
        .map
        .get(&(sig as i32))
        .cloned()
        .unwrap_or(Value::Int(0)))
}

fn pcntl_signal_dispatch(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    dispatch(vm)?;
    Ok(Value::Bool(true))
}

fn pcntl_async_signals(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = ASYNC.load(Ordering::Relaxed);
    if let Some(Value::Bool(b)) = a.get(0).map(|v| v.deref()) {
        ASYNC.store(b, Ordering::Relaxed);
        if b && PENDING.load(Ordering::Acquire) {
            ASYNC_PENDING.store(true, Ordering::Release);
        }
    }
    Ok(Value::Bool(old))
}

/// The `$signals` array of the mask and wait functions.
fn signal_set(vm: &mut Vm, a: &Args, i: usize, allow_empty: bool) -> R<libc::sigset_t> {
    let arr = a.array(i);
    let name = format!("Argument #{} ($signals)", i + 1);
    if arr.is_empty() && !allow_empty {
        return vm.fthrow(a, "ValueError", &format!("{} must not be empty", name));
    }
    // SAFETY: sigemptyset initialises the set.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut set) };
    for (_, v) in arr.iter() {
        let v = v.deref();
        let n = match &v {
            Value::Int(n) => *n,
            Value::Str(s) => {
                use goro_runtime::numeric::{Numericness, parse_numeric};
                let p = parse_numeric(s);
                match (p.kind, p.num) {
                    (Numericness::Numeric, goro_runtime::Number::Int(n)) => n,
                    _ => {
                        return vm.fthrow(
                            a,
                            "TypeError",
                            &format!("{} signals must be of type int, string given", name),
                        );
                    }
                }
            }
            other => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "{} signals must be of type int, {} given",
                        name,
                        Vm::type_name_given(other)
                    ),
                );
            }
        };
        if !(1..NSIG).contains(&n) {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("{} signals must be between 1 and {}", name, NSIG - 1),
            );
        }
        unsafe { libc::sigaddset(&mut set, n as libc::c_int) };
    }
    Ok(set)
}

fn pcntl_sigprocmask(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = a.int(0) as libc::c_int;
    if ![libc::SIG_BLOCK, libc::SIG_UNBLOCK, libc::SIG_SETMASK].contains(&mode) || a.int(0) > i32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($mode) must be one of SIG_BLOCK, SIG_UNBLOCK, or SIG_SETMASK",
        );
    }
    let set = signal_set(vm, a, 1, mode == libc::SIG_SETMASK)?;
    let mut old: libc::sigset_t = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigprocmask(mode, &set, &mut old) } != 0 {
        set_error(errno());
        return Ok(Value::Bool(false));
    }
    if a.has(2) {
        let mut list = Vec::new();
        for s in 1..NSIG as libc::c_int {
            if unsafe { libc::sigismember(&old, s) } == 1 {
                list.push(Value::Int(s as i64));
            }
        }
        a.set_ref(2, Value::Array(PhpArray::from_values(list)));
    }
    Ok(Value::Bool(true))
}

fn wait_result(a: &Args, r: libc::c_int, si: &libc::siginfo_t) -> R<Value> {
    if r < 0 {
        let e = errno();
        if e != libc::EAGAIN {
            set_error(e);
        }
        return Ok(Value::Bool(false));
    }
    a.set_ref(1, info_array(&info_from(si)));
    Ok(Value::Int(r as i64))
}

fn pcntl_sigwaitinfo(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let set = signal_set(vm, a, 0, false)?;
    vm.out.flush();
    let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::sigwaitinfo(&set, &mut si) };
    wait_result(a, r, &si)
}

fn pcntl_sigtimedwait(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let set = signal_set(vm, a, 0, false)?;
    let secs = a.int_or(2, 0);
    let ns = a.int_or(3, 0);
    if secs < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($seconds) must be greater than or equal to 0",
        );
    }
    if !(0..1_000_000_000).contains(&ns) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($nanoseconds) must be between 0 and 1e9",
        );
    }
    if secs == 0 && ns == 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "At least one of argument #3 ($seconds) or argument #4 ($nanoseconds) must be greater than 0",
        );
    }
    vm.out.flush();
    let ts = libc::timespec {
        tv_sec: secs as libc::time_t,
        tv_nsec: ns as libc::c_long,
    };
    let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::sigtimedwait(&set, &mut si, &ts) };
    wait_result(a, r, &si)
}

// ---- processes ----

/// pcntl_fork(): output written so far goes out first, so the child
/// does not repeat it (PHP's CLI writes unbuffered).
fn pcntl_fork(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.out.flush();
    // SAFETY: the engine is single-threaded; the child continues with a
    // copy of this process.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let e = errno();
        set_error(e);
        vm.fwarn(a, &format!("Error {}", e))?;
    }
    Ok(Value::Int(pid as i64))
}

/// The resource usage array (getrusage() field order, observed on PHP 8.5).
fn rusage_array(ru: &libc::rusage) -> Value {
    let mut arr = PhpArray::new();
    let mut put = |n: &str, v: i64| {
        arr.insert(k(n), Value::Int(v));
    };
    put("ru_oublock", ru.ru_oublock);
    put("ru_inblock", ru.ru_inblock);
    put("ru_msgsnd", ru.ru_msgsnd);
    put("ru_msgrcv", ru.ru_msgrcv);
    put("ru_maxrss", ru.ru_maxrss);
    put("ru_ixrss", ru.ru_ixrss);
    put("ru_idrss", ru.ru_idrss);
    put("ru_minflt", ru.ru_minflt);
    put("ru_majflt", ru.ru_majflt);
    put("ru_nsignals", ru.ru_nsignals);
    put("ru_nvcsw", ru.ru_nvcsw);
    put("ru_nivcsw", ru.ru_nivcsw);
    put("ru_nswap", ru.ru_nswap);
    put("ru_utime.tv_usec", ru.ru_utime.tv_usec);
    put("ru_utime.tv_sec", ru.ru_utime.tv_sec);
    put("ru_stime.tv_usec", ru.ru_stime.tv_usec);
    put("ru_stime.tv_sec", ru.ru_stime.tv_sec);
    Value::Array(arr)
}

fn wait_common(vm: &mut Vm, a: &Args, pid: i32, status_idx: usize, flags: i64) -> R<Value> {
    vm.out.flush();
    let mut st: libc::c_int = 0;
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    let r = loop {
        // SAFETY: wait4 with valid status and rusage pointers.
        let r = unsafe { libc::wait4(pid, &mut st, flags as libc::c_int, &mut ru) };
        if r < 0 && errno() == libc::EINTR {
            after_native_call(vm)?;
            continue;
        }
        break r;
    };
    if r < 0 {
        set_error(errno());
    }
    a.set_ref(status_idx, Value::Int(st as i64));
    if a.has(status_idx + 2) {
        let v = if r > 0 {
            rusage_array(&ru)
        } else {
            Value::Array(PhpArray::new())
        };
        a.set_ref(status_idx + 2, v);
    }
    Ok(Value::Int(r as i64))
}

fn pcntl_waitpid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = a.int(0) as i32;
    let flags = a.int_or(2, 0);
    wait_common(vm, a, pid, 1, flags)
}

fn pcntl_wait(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(1, 0);
    wait_common(vm, a, -1, 0, flags)
}

fn pcntl_waitid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.out.flush();
    let idtype = a.int_or(0, libc::P_ALL as i64);
    let id = a.opt_int(1).unwrap_or(0);
    let flags = a.int_or(3, libc::WEXITED as i64);
    let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: the waitid system call with its optional rusage pointer.
    let r = unsafe {
        libc::syscall(
            libc::SYS_waitid,
            idtype as libc::c_int,
            id as libc::id_t,
            &mut si as *mut libc::siginfo_t,
            flags as libc::c_int,
            &mut ru as *mut libc::rusage,
        )
    };
    let ok = r == 0;
    if !ok {
        set_error(errno());
    }
    if a.has(2) {
        let v = if ok && si.si_signo != 0 {
            info_array(&info_from(&si))
        } else {
            Value::Array(PhpArray::new())
        };
        a.set_ref(2, v);
    }
    if a.has(4) {
        let v = if ok && si.si_signo != 0 {
            rusage_array(&ru)
        } else {
            Value::Array(PhpArray::new())
        };
        a.set_ref(4, v);
    }
    Ok(Value::Bool(ok))
}

fn pcntl_alarm(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let secs = a.int(0).clamp(0, u32::MAX as i64) as u32;
    // SAFETY: alarm() has no memory effects.
    Ok(Value::Int(unsafe { libc::alarm(secs) } as i64))
}

fn status(a: &Args) -> libc::c_int {
    a.int(0) as libc::c_int
}

fn pcntl_wifexited(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(libc::WIFEXITED(status(a))))
}

fn pcntl_wifstopped(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(libc::WIFSTOPPED(status(a))))
}

fn pcntl_wifcontinued(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(libc::WIFCONTINUED(status(a))))
}

fn pcntl_wifsignaled(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(libc::WIFSIGNALED(status(a))))
}

fn pcntl_wexitstatus(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(libc::WEXITSTATUS(status(a)) as i64))
}

fn pcntl_wtermsig(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(libc::WTERMSIG(status(a)) as i64))
}

fn pcntl_wstopsig(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(libc::WSTOPSIG(status(a)) as i64))
}

/// pcntl_exec(): the messages from tests pcntl_exec_3.phpt and
/// pcntl_exec_strings_with_nul_bytes.phpt.
fn pcntl_exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std::ffi::CString;
    let path = a.str(0);
    let Ok(cpath) = CString::new(path.as_bytes()) else {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($path) must not contain any null bytes",
        );
    };
    let mut argv = vec![cpath.clone()];
    for (_, v) in a.array(1).iter() {
        let s = vm.to_php_str(&v.deref())?;
        let Ok(c) = CString::new(s.as_bytes()) else {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($args) individual argument must not contain null bytes",
            );
        };
        argv.push(c);
    }
    let env = if a.has(2) {
        let mut env = Vec::new();
        for (key, v) in a.array(2).iter() {
            let name = key.to_owned();
            let name: Vec<u8> = match &name {
                Key::Int(i) => i.to_string().into_bytes(),
                Key::Str(s) => s.as_bytes().to_vec(),
            };
            let val = vm.to_php_str(&v.deref())?;
            if name.contains(&0) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($env_vars) name for environment variable must not contain null bytes",
                );
            }
            if val.as_bytes().contains(&0) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($env_vars) value for environment variable must not contain null bytes",
                );
            }
            let mut e = name;
            e.push(b'=');
            e.extend_from_slice(val.as_bytes());
            env.push(CString::new(e).unwrap_or_default());
        }
        Some(env)
    } else {
        None
    };
    vm.out.flush();
    let mut ptrs: Vec<*const libc::c_char> = argv.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    // SAFETY: NUL-terminated argument and environment vectors.
    unsafe {
        match &env {
            Some(env) => {
                let mut eptrs: Vec<*const libc::c_char> = env.iter().map(|c| c.as_ptr()).collect();
                eptrs.push(std::ptr::null());
                libc::execve(cpath.as_ptr(), ptrs.as_ptr(), eptrs.as_ptr());
            }
            None => {
                libc::execv(cpath.as_ptr(), ptrs.as_ptr());
            }
        }
    }
    let e = errno();
    set_error(e);
    vm.fwarn(
        a,
        &format!(
            "Error has occurred: (errno {}) {}",
            e,
            super::exec::strerror(e)
        ),
    )?;
    Ok(Value::Bool(false))
}

fn pcntl_errno(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(LAST_ERROR.load(Ordering::Relaxed) as i64))
}

fn pcntl_strerror(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::from(super::exec::strerror(
        a.int(0) as i32
    ))))
}

// ---- priorities ----

fn check_mode(vm: &mut Vm, a: &Args, i: usize) -> R<libc::c_int> {
    let mode = a.int_or(i, libc::PRIO_PROCESS as i64);
    if ![libc::PRIO_PGRP, libc::PRIO_USER, libc::PRIO_PROCESS]
        .iter()
        .any(|&m| m as i64 == mode)
    {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($mode) must be one of PRIO_PGRP, PRIO_USER, or PRIO_PROCESS",
                i + 1
            ),
        );
    }
    Ok(mode as libc::c_int)
}

/// The warnings of the priority functions (tests
/// pcntl_getpriority_error.phpt, pcntl_setpriority_error_linux.phpt).
fn priority_error(vm: &mut Vm, a: &Args) -> R<Value> {
    let e = errno();
    set_error(e);
    let msg = match e {
        libc::ESRCH => format!(
            "Error {}: No process was located using the given parameters",
            e
        ),
        libc::EINVAL => format!("Error {}: Invalid identifier flag", e),
        libc::EPERM => format!(
            "Error {}: A process was located, but neither its effective nor real user ID matched the effective user ID of the caller",
            e
        ),
        libc::EACCES => format!(
            "Error {}: Only a super user may attempt to increase the process priority",
            e
        ),
        _ => format!("Unknown error {} has occurred", e),
    };
    vm.fwarn(a, &msg)?;
    Ok(Value::Bool(false))
}

fn pcntl_getpriority(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = a.opt_int(0).unwrap_or(0);
    let mode = check_mode(vm, a, 1)?;
    // SAFETY: errno is thread-local; getpriority() may return -1.
    unsafe { *libc::__errno_location() = 0 };
    let r = unsafe { libc::getpriority(mode as libc::__priority_which_t, pid as libc::id_t) };
    if r == -1 && errno() != 0 {
        return priority_error(vm, a);
    }
    Ok(Value::Int(r as i64))
}

fn pcntl_setpriority(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let prio = a.int(0);
    let pid = a.opt_int(1).unwrap_or(0);
    let mode = check_mode(vm, a, 2)?;
    let r = unsafe {
        libc::setpriority(
            mode as libc::__priority_which_t,
            pid as libc::id_t,
            prio as libc::c_int,
        )
    };
    if r != 0 {
        return priority_error(vm, a);
    }
    Ok(Value::Bool(true))
}

// ---- namespaces and CPUs ----

fn pcntl_unshare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int(0);
    if unsafe { libc::unshare(flags as libc::c_int) } == 0 {
        return Ok(Value::Bool(true));
    }
    let e = errno();
    set_error(e);
    let msg = match e {
        libc::EINVAL => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($flags) must be a combination of CLONE_* flags, or at least one flag is unsupported by the kernel ",
            );
        }
        libc::ENOMEM => format!("Error {}: Insufficient memory for unshare", e),
        libc::EPERM => format!("Error {}: No privilege to use these flags", e),
        libc::ENOSPC => format!(
            "Error {}: Reached the maximum nesting limit for one of the specified namespaces",
            e
        ),
        libc::EUSERS => format!(
            "Error {}: Reached the maximum nesting limit for the user namespace",
            e
        ),
        _ => format!("Unknown error {} has occurred", e),
    };
    vm.fwarn(a, &msg)?;
    Ok(Value::Bool(false))
}

fn ncpus() -> i64 {
    unsafe { libc::sysconf(libc::_SC_NPROCESSORS_CONF) }.max(1) as i64
}

fn invalid_process(vm: &mut Vm, a: &Args, pid: i64) -> R<Value> {
    vm.fthrow(
        a,
        "ValueError",
        &format!("Argument #1 ($process_id) invalid process ({})", pid),
    )
}

fn pcntl_getcpuaffinity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = a.opt_int(0).unwrap_or(0);
    if !(0..=i32::MAX as i64).contains(&pid) {
        return invalid_process(vm, a, pid);
    }
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::sched_getaffinity(
            pid as libc::pid_t,
            std::mem::size_of::<libc::cpu_set_t>(),
            &mut set,
        )
    };
    if r != 0 {
        let e = errno();
        set_error(e);
        if e == libc::ESRCH || e == libc::EINVAL {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #1 ($process_id) invalid process ({})", pid),
            );
        }
        return Ok(Value::Bool(false));
    }
    let mut list = Vec::new();
    for c in 0..ncpus().min(libc::CPU_SETSIZE as i64) {
        if unsafe { libc::CPU_ISSET(c as usize, &set) } {
            list.push(Value::Int(c));
        }
    }
    Ok(Value::Array(PhpArray::from_values(list)))
}

/// pcntl_setcpuaffinity(): the checks and messages of test
/// pcntl_cpuaffinity.phpt.
fn pcntl_setcpuaffinity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = a.opt_int(0).unwrap_or(0);
    if !(0..=i32::MAX as i64).contains(&pid) {
        return invalid_process(vm, a, pid);
    }
    let ids = a.array(1);
    if ids.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($cpu_ids) must not be empty",
        );
    }
    let max = ncpus();
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    for (_, v) in ids.iter() {
        let v = v.deref();
        let id = match &v {
            Value::Int(n) => *n,
            Value::Str(s) => {
                use goro_runtime::numeric::{Numericness, parse_numeric};
                let p = parse_numeric(s);
                match (p.kind, p.num) {
                    (Numericness::Numeric, goro_runtime::Number::Int(n)) => n,
                    _ => {
                        return vm.fthrow(
                            a,
                            "ValueError",
                            &format!(
                                "Argument #2 ($cpu_ids) cpu id invalid value ({})",
                                s.to_string_lossy()
                            ),
                        );
                    }
                }
            }
            other => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #2 ($cpu_ids) value must be of type int|string, {} given",
                        Vm::type_name_given(other)
                    ),
                );
            }
        };
        if id < 0 || id >= max {
            return vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #2 ($cpu_ids) cpu id must be between 0 and {} ({})",
                    max, id
                ),
            );
        }
        unsafe { libc::CPU_SET(id as usize, &mut set) };
    }
    let r = unsafe {
        libc::sched_setaffinity(
            pid as libc::pid_t,
            std::mem::size_of::<libc::cpu_set_t>(),
            &set,
        )
    };
    if r != 0 {
        let e = errno();
        set_error(e);
        if e == libc::ESRCH {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #1 ($process_id) invalid process ({})", pid),
            );
        }
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

fn pcntl_getcpu(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(unsafe { libc::sched_getcpu() } as i64))
}
