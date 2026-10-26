//! php-src's ext/pcntl (pcntl.c, php_signal.c): process control.
//!
//! Signals are caught by `signal_handler`, which only records them in a
//! fixed queue (async-signal-safe) and, with pcntl_async_signals(true),
//! raises `value::VM_INTERRUPT`: the VM then runs `Vm::interrupt_hook`
//! between instructions, which calls the PHP handlers. Without
//! asynchronous signals they run from pcntl_signal_dispatch() or a tick.

use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::mem::MaybeUninit;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// The extension name (`extension_loaded('pcntl')`).
pub const NAME: &[u8] = b"pcntl";

type R = Result<Value, VmError>;

/// NSIG (glibc): one more than the last real-time signal.
const NSIG: i64 = 65;

fn num_signals() -> i64 {
    NSIG.max(libc::SIGRTMAX() as i64 + 1)
}

// ---------------------------------------------------------------------------
// The signal queue (shared with the signal handler)
// ---------------------------------------------------------------------------

/// PCNTL_G(spares): how many signals can be pending at once.
const QUEUE_CAP: usize = 128;

struct Queue {
    slots: UnsafeCell<[MaybeUninit<(i32, libc::siginfo_t)>; QUEUE_CAP]>,
    len: AtomicUsize,
}

// The queue is only touched by the signal handler (which runs with all
// signals blocked) and by dispatch() with all signals blocked.
unsafe impl Sync for Queue {}

static QUEUE: Queue = Queue {
    slots: UnsafeCell::new([const { MaybeUninit::uninit() }; QUEUE_CAP]),
    len: AtomicUsize::new(0),
};

/// PCNTL_G(pending_signals)
static PENDING: AtomicBool = AtomicBool::new(false);
/// PCNTL_G(async_signals)
static ASYNC: AtomicBool = AtomicBool::new(false);
/// The number of slots in use: PCNTL_G(num_signals) records ("spares")
/// once pcntl_signal() has been called.
static CAPACITY: AtomicUsize = AtomicUsize::new(0);

/// pcntl_signal_handler()
extern "C" fn signal_handler(
    signo: libc::c_int,
    info: *mut libc::siginfo_t,
    _ctx: *mut libc::c_void,
) {
    let n = QUEUE.len.load(Ordering::Relaxed);
    if n >= CAPACITY.load(Ordering::Relaxed).min(QUEUE_CAP) {
        // too many signals to track: this one is forgotten
        return;
    }
    let si = if info.is_null() {
        unsafe { std::mem::zeroed() }
    } else {
        unsafe { *info }
    };
    unsafe {
        (*QUEUE.slots.get())[n].write((signo, si));
    }
    QUEUE.len.store(n + 1, Ordering::Release);
    PENDING.store(true, Ordering::Release);
    if ASYNC.load(Ordering::Relaxed) {
        goro_core::value::VM_INTERRUPT.store(true, Ordering::Release);
    }
}

thread_local! {
    /// PCNTL_G(php_signal_table)
    static HANDLERS: RefCell<HashMap<i64, Value>> = RefCell::new(HashMap::new());
    /// PCNTL_G(processing_signal_queue)
    static PROCESSING: Cell<bool> = const { Cell::new(false) };
    /// PCNTL_G(last_error)
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

fn strerror(e: i32) -> String {
    unsafe { CStr::from_ptr(libc::strerror(e)) }
        .to_string_lossy()
        .into_owned()
}

/// pcntl_signal_dispatch()
pub fn dispatch(vm: &mut Vm) {
    if !PENDING.load(Ordering::Acquire) {
        return;
    }
    let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
    let mut old: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigfillset(&mut mask);
        libc::sigprocmask(libc::SIG_BLOCK, &mask, &mut old);
    }
    let n = QUEUE.len.load(Ordering::Acquire);
    if n == 0 || PROCESSING.with(|p| p.get()) {
        unsafe { libc::sigprocmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) };
        return;
    }
    // Prevent switching fibers when handling signals
    vm.fiber_switch_blocking += 1;
    PROCESSING.with(|p| p.set(true));
    let queue: Vec<(i32, libc::siginfo_t)> = (0..n)
        .map(|i| unsafe { (*QUEUE.slots.get())[i].assume_init() })
        .collect();
    QUEUE.len.store(0, Ordering::Release);
    for (signo, info) in queue {
        let handle = HANDLERS.with(|h| h.borrow().get(&(signo as i64)).cloned());
        if let Some(h) = handle {
            if !matches!(h, Value::Long(_)) {
                let si = siginfo_array(signo, &info);
                let args = [Value::Long(signo as i64), si];
                let _ = vm.call_callback(&h, &args);
                if vm.current_exception.is_some() || vm.exit_requested {
                    break;
                }
            }
        }
    }
    PENDING.store(false, Ordering::Release);
    PROCESSING.with(|p| p.set(false));
    vm.fiber_switch_blocking -= 1;
    unsafe { libc::sigprocmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) };
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

const CONSTANTS: &[(&str, i64)] = &[
    ("WNOHANG", libc::WNOHANG as i64),
    ("WUNTRACED", libc::WUNTRACED as i64),
    ("WCONTINUED", libc::WCONTINUED as i64),
    ("WEXITED", libc::WEXITED as i64),
    ("WSTOPPED", libc::WSTOPPED as i64),
    ("WNOWAIT", libc::WNOWAIT as i64),
    ("P_ALL", libc::P_ALL as i64),
    ("P_PID", libc::P_PID as i64),
    ("P_PGID", libc::P_PGID as i64),
    ("P_PIDFD", libc::P_PIDFD as i64),
    ("SIG_IGN", 1),
    ("SIG_DFL", 0),
    ("SIG_ERR", -1),
    ("SIGHUP", libc::SIGHUP as i64),
    ("SIGINT", libc::SIGINT as i64),
    ("SIGQUIT", libc::SIGQUIT as i64),
    ("SIGILL", libc::SIGILL as i64),
    ("SIGTRAP", libc::SIGTRAP as i64),
    ("SIGABRT", libc::SIGABRT as i64),
    ("SIGIOT", libc::SIGIOT as i64),
    ("SIGBUS", libc::SIGBUS as i64),
    ("SIGFPE", libc::SIGFPE as i64),
    ("SIGKILL", libc::SIGKILL as i64),
    ("SIGUSR1", libc::SIGUSR1 as i64),
    ("SIGSEGV", libc::SIGSEGV as i64),
    ("SIGUSR2", libc::SIGUSR2 as i64),
    ("SIGPIPE", libc::SIGPIPE as i64),
    ("SIGALRM", libc::SIGALRM as i64),
    ("SIGTERM", libc::SIGTERM as i64),
    ("SIGSTKFLT", libc::SIGSTKFLT as i64),
    ("SIGCLD", libc::SIGCHLD as i64),
    ("SIGCHLD", libc::SIGCHLD as i64),
    ("SIGCONT", libc::SIGCONT as i64),
    ("SIGSTOP", libc::SIGSTOP as i64),
    ("SIGTSTP", libc::SIGTSTP as i64),
    ("SIGTTIN", libc::SIGTTIN as i64),
    ("SIGTTOU", libc::SIGTTOU as i64),
    ("SIGURG", libc::SIGURG as i64),
    ("SIGXCPU", libc::SIGXCPU as i64),
    ("SIGXFSZ", libc::SIGXFSZ as i64),
    ("SIGVTALRM", libc::SIGVTALRM as i64),
    ("SIGPROF", libc::SIGPROF as i64),
    ("SIGWINCH", libc::SIGWINCH as i64),
    ("SIGPOLL", libc::SIGPOLL as i64),
    ("SIGIO", libc::SIGIO as i64),
    ("SIGPWR", libc::SIGPWR as i64),
    ("SIGSYS", libc::SIGSYS as i64),
    ("SIGBABY", libc::SIGSYS as i64),
    ("SIGRTMIN", -1),
    ("SIGRTMAX", -1),
    ("PRIO_PGRP", libc::PRIO_PGRP as i64),
    ("PRIO_USER", libc::PRIO_USER as i64),
    ("PRIO_PROCESS", libc::PRIO_PROCESS as i64),
    ("SIG_BLOCK", libc::SIG_BLOCK as i64),
    ("SIG_UNBLOCK", libc::SIG_UNBLOCK as i64),
    ("SIG_SETMASK", libc::SIG_SETMASK as i64),
    ("SI_USER", libc::SI_USER as i64),
    ("SI_KERNEL", 0x80),
    ("SI_QUEUE", libc::SI_QUEUE as i64),
    ("SI_TIMER", libc::SI_TIMER as i64),
    ("SI_MESGQ", libc::SI_MESGQ as i64),
    ("SI_ASYNCIO", libc::SI_ASYNCIO as i64),
    ("SI_SIGIO", libc::SI_SIGIO as i64),
    ("SI_TKILL", libc::SI_TKILL as i64),
    ("CLD_EXITED", libc::CLD_EXITED as i64),
    ("CLD_KILLED", libc::CLD_KILLED as i64),
    ("CLD_DUMPED", libc::CLD_DUMPED as i64),
    ("CLD_TRAPPED", libc::CLD_TRAPPED as i64),
    ("CLD_STOPPED", libc::CLD_STOPPED as i64),
    ("CLD_CONTINUED", libc::CLD_CONTINUED as i64),
    ("TRAP_BRKPT", 1),
    ("TRAP_TRACE", 2),
    ("POLL_IN", 1),
    ("POLL_OUT", 2),
    ("POLL_MSG", 3),
    ("POLL_ERR", 4),
    ("POLL_PRI", 5),
    ("POLL_HUP", 6),
    ("ILL_ILLOPC", 1),
    ("ILL_ILLOPN", 2),
    ("ILL_ILLADR", 3),
    ("ILL_ILLTRP", 4),
    ("ILL_PRVOPC", 5),
    ("ILL_PRVREG", 6),
    ("ILL_COPROC", 7),
    ("ILL_BADSTK", 8),
    ("FPE_INTDIV", 1),
    ("FPE_INTOVF", 2),
    ("FPE_FLTDIV", 3),
    ("FPE_FLTOVF", 4),
    ("FPE_FLTUND", 5),
    ("FPE_FLTRES", 6),
    ("FPE_FLTINV", 7),
    ("FPE_FLTSUB", 8),
    ("SEGV_MAPERR", 1),
    ("SEGV_ACCERR", 2),
    ("BUS_ADRALN", 1),
    ("BUS_ADRERR", 2),
    ("BUS_OBJERR", 3),
    ("CLONE_NEWNS", libc::CLONE_NEWNS as i64),
    ("CLONE_NEWIPC", libc::CLONE_NEWIPC as i64),
    ("CLONE_NEWUTS", libc::CLONE_NEWUTS as i64),
    ("CLONE_NEWNET", libc::CLONE_NEWNET as i64),
    ("CLONE_NEWPID", libc::CLONE_NEWPID as i64),
    ("CLONE_NEWUSER", libc::CLONE_NEWUSER as i64),
    ("CLONE_NEWCGROUP", libc::CLONE_NEWCGROUP as i64),
    ("PCNTL_EINTR", libc::EINTR as i64),
    ("PCNTL_ECHILD", libc::ECHILD as i64),
    ("PCNTL_EINVAL", libc::EINVAL as i64),
    ("PCNTL_EAGAIN", libc::EAGAIN as i64),
    ("PCNTL_ESRCH", libc::ESRCH as i64),
    ("PCNTL_EACCES", libc::EACCES as i64),
    ("PCNTL_EPERM", libc::EPERM as i64),
    ("PCNTL_ENOMEM", libc::ENOMEM as i64),
    ("PCNTL_E2BIG", libc::E2BIG as i64),
    ("PCNTL_EFAULT", libc::EFAULT as i64),
    ("PCNTL_EIO", libc::EIO as i64),
    ("PCNTL_EISDIR", libc::EISDIR as i64),
    ("PCNTL_ELIBBAD", libc::ELIBBAD as i64),
    ("PCNTL_ELOOP", libc::ELOOP as i64),
    ("PCNTL_EMFILE", libc::EMFILE as i64),
    ("PCNTL_ENAMETOOLONG", libc::ENAMETOOLONG as i64),
    ("PCNTL_ENFILE", libc::ENFILE as i64),
    ("PCNTL_ENOENT", libc::ENOENT as i64),
    ("PCNTL_ENOEXEC", libc::ENOEXEC as i64),
    ("PCNTL_ENOTDIR", libc::ENOTDIR as i64),
    ("PCNTL_ETXTBSY", libc::ETXTBSY as i64),
    ("PCNTL_ENOSPC", libc::ENOSPC as i64),
    ("PCNTL_EUSERS", libc::EUSERS as i64),
];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("pcntl_fork", pcntl_fork),
    ("pcntl_waitpid", pcntl_waitpid),
    ("pcntl_waitid", pcntl_waitid),
    ("pcntl_wait", pcntl_wait),
    ("pcntl_signal", pcntl_signal),
    ("pcntl_signal_get_handler", pcntl_signal_get_handler),
    ("pcntl_signal_dispatch", pcntl_signal_dispatch),
    ("pcntl_sigprocmask", pcntl_sigprocmask),
    ("pcntl_sigwaitinfo", pcntl_sigwaitinfo),
    ("pcntl_sigtimedwait", pcntl_sigtimedwait),
    ("pcntl_wifexited", pcntl_wifexited),
    ("pcntl_wifstopped", pcntl_wifstopped),
    ("pcntl_wifcontinued", pcntl_wifcontinued),
    ("pcntl_wifsignaled", pcntl_wifsignaled),
    ("pcntl_wexitstatus", pcntl_wexitstatus),
    ("pcntl_wtermsig", pcntl_wtermsig),
    ("pcntl_wstopsig", pcntl_wstopsig),
    ("pcntl_exec", pcntl_exec),
    ("pcntl_alarm", pcntl_alarm),
    ("pcntl_get_last_error", pcntl_get_last_error),
    ("pcntl_errno", pcntl_get_last_error),
    ("pcntl_getpriority", pcntl_getpriority),
    ("pcntl_setpriority", pcntl_setpriority),
    ("pcntl_strerror", pcntl_strerror),
    ("pcntl_async_signals", pcntl_async_signals),
    ("pcntl_unshare", pcntl_unshare),
    ("pcntl_setns", pcntl_setns),
    ("pcntl_getcpuaffinity", pcntl_getcpuaffinity),
    ("pcntl_setcpuaffinity", pcntl_setcpuaffinity),
    ("pcntl_getcpu", pcntl_getcpu),
];

/// MINIT + RINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    // PHP_MINFO_FUNCTION(pcntl)
    vm.phpinfo_hooks
        .push(|_| "\npcntl\n\npcntl support => enabled\n".to_string());
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
        let v = match name {
            "SIGRTMIN" => libc::SIGRTMIN() as i64,
            "SIGRTMAX" => libc::SIGRTMAX() as i64,
            _ => v,
        };
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
    // zend_interrupt_function and the tick function
    vm.interrupt_hook = Some(dispatch);
    vm.tick_hook = Some(dispatch);
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

fn type_error(vm: &mut Vm, func: &str, n: usize, what: &str) -> R {
    let msg = format!("{} {}", arg_prefix(func, n), what);
    throw(vm, b"TypeError", &msg)
}

fn warn(vm: &mut Vm, func: &str, msg: &str) {
    vm.emit_warning(&format!("{}(): {}", func, msg));
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

fn long_or(args: &[Value], i: usize, d: i64) -> i64 {
    match arg(args, i) {
        Value::Undef => d,
        v => v.to_long(),
    }
}

/// Z_PARAM_LONG_OR_NULL: None for null (or not passed).
fn opt_long(args: &[Value], i: usize) -> Option<i64> {
    match arg(args, i) {
        Value::Undef | Value::Null => None,
        v => Some(v.to_long()),
    }
}

/// The by-reference argument `i`, when passed.
fn ref_arg(args: &[Value], i: usize) -> Option<&Value> {
    match args.get(i) {
        Some(Value::Undef) | None => None,
        Some(v) => Some(v),
    }
}

/// ZEND_TRY_ASSIGN_REF_*()
fn assign_ref(target: &Value, v: Value) {
    if let Value::Reference(r) = target {
        *r.borrow_mut() = v;
    }
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn set(a: &mut PhpArray, k: &str, v: Value) {
    a.set(ArrayKey::String(PhpString::from_string(k.to_string())), v);
}

fn bool_val(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// zval_try_get_long(): None when the value cannot be converted.
fn try_get_long(v: &Value) -> Option<i64> {
    match v.deref() {
        Value::Long(n) => Some(n),
        Value::Null | Value::Undef | Value::False => Some(0),
        Value::True => Some(1),
        Value::Double(d) => {
            if d.is_finite() && (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
                Some(d as i64)
            } else {
                None
            }
        }
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), true) {
            Some((goro_core::value::NumStr::Long(n), false)) => Some(n),
            Some((goro_core::value::NumStr::Double(d), false)) => {
                if d.is_finite() {
                    Some(d as i64)
                } else {
                    None
                }
            }
            _ => None,
        },
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Process creation
// ---------------------------------------------------------------------------

fn pcntl_fork(vm: &mut Vm, _args: &[Value]) -> R {
    // What was written must not be written again by the child.
    vm.flush_cli_output();
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
    }
    let id = unsafe { libc::fork() };
    if id == -1 {
        let e = errno();
        set_last_error(e);
        let msg = match e {
            libc::EAGAIN => format!(
                "Error {}: Reached the maximum limit of number of processes",
                e
            ),
            libc::ENOMEM => format!("Error {}: Insufficient memory", e),
            libc::ENOSYS => format!("Error {}: Unimplemented", e),
            libc::EBADF => format!("Error {}: File descriptor concurrency issue", e),
            _ => format!("Error {}", e),
        };
        warn(vm, "pcntl_fork", &msg);
    } else {
        // Parent and child now share stdout: write as the CLI does.
        vm.cli_write_through = true;
    }
    Ok(Value::Long(id as i64))
}

fn pcntl_exec(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_exec";
    let path = arg(args, 0).to_php_string().as_bytes().to_vec();
    if path.contains(&0) {
        return value_error(vm, f, 1, "must not contain any null bytes");
    }
    let mut argv: Vec<CString> = vec![CString::new(path.clone()).unwrap_or_default()];
    if let Value::Array(a) = arg(args, 1) {
        let items: Vec<Value> = a.borrow().iter().map(|(_, v)| v.clone()).collect();
        for v in items {
            let Ok(s) = vm.value_to_string_checked(&v.deref()) else {
                return Ok(Value::Null);
            };
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            if s.as_bytes().contains(&0) {
                return value_error(vm, f, 2, "individual argument must not contain null bytes");
            }
            argv.push(CString::new(s.as_bytes().to_vec()).unwrap_or_default());
        }
    }
    let mut envp: Option<Vec<CString>> = None;
    if let Value::Array(a) = arg(args, 2) {
        let items: Vec<(ArrayKey, Value)> = a
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut env = Vec::new();
        for (k, v) in items {
            let Ok(val) = vm.value_to_string_checked(&v.deref()) else {
                return Ok(Value::False);
            };
            if vm.current_exception.is_some() {
                return Ok(Value::False);
            }
            if val.as_bytes().contains(&0) {
                let _ = value_error(
                    vm,
                    f,
                    3,
                    "value for environment variable must not contain null bytes",
                );
                return Ok(Value::False);
            }
            let key = match k {
                ArrayKey::Int(n) => n.to_string().into_bytes(),
                ArrayKey::String(s) => {
                    if s.as_bytes().contains(&0) {
                        let _ = value_error(
                            vm,
                            f,
                            3,
                            "name for environment variable must not contain null bytes",
                        );
                        return Ok(Value::False);
                    }
                    s.as_bytes().to_vec()
                }
            };
            let mut pair = key;
            pair.push(b'=');
            pair.extend_from_slice(val.as_bytes());
            env.push(CString::new(pair).unwrap_or_default());
        }
        envp = Some(env);
    }
    vm.flush_cli_output();
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    let mut av: Vec<*const libc::c_char> = argv.iter().map(|c| c.as_ptr()).collect();
    av.push(std::ptr::null());
    let cpath = CString::new(path).unwrap_or_default();
    // The new image does not keep goro's own RLIMIT_AS cap.
    let mut saved_as: libc::rlimit = unsafe { std::mem::zeroed() };
    let have_as = unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut saved_as) } == 0;
    goro_core::child_proc::ChildState::capture().restore_rlimit_as();
    let r = match &envp {
        Some(env) => {
            let mut ep: Vec<*const libc::c_char> = env.iter().map(|c| c.as_ptr()).collect();
            ep.push(std::ptr::null());
            unsafe { libc::execve(cpath.as_ptr(), av.as_ptr(), ep.as_ptr()) }
        }
        None => unsafe { libc::execv(cpath.as_ptr(), av.as_ptr()) },
    };
    if r == -1 {
        let e = errno();
        if have_as {
            unsafe { libc::setrlimit(libc::RLIMIT_AS, &saved_as) };
        }
        set_last_error(e);
        warn(
            vm,
            f,
            &format!("Error has occurred: (errno {}) {}", e, strerror(e)),
        );
    }
    Ok(Value::False)
}

fn pcntl_alarm(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(Value::Long(
        unsafe { libc::alarm(long(args, 0) as libc::c_uint) } as i64,
    ))
}

// ---------------------------------------------------------------------------
// Waiting
// ---------------------------------------------------------------------------

/// PHP_RUSAGE_TO_ARRAY()
fn rusage_to_array(ru: &libc::rusage, target: &Value) {
    let mut a = PhpArray::new();
    let fields: [(&str, i64); 17] = [
        ("ru_oublock", ru.ru_oublock),
        ("ru_inblock", ru.ru_inblock),
        ("ru_msgsnd", ru.ru_msgsnd),
        ("ru_msgrcv", ru.ru_msgrcv),
        ("ru_maxrss", ru.ru_maxrss),
        ("ru_ixrss", ru.ru_ixrss),
        ("ru_idrss", ru.ru_idrss),
        ("ru_minflt", ru.ru_minflt),
        ("ru_majflt", ru.ru_majflt),
        ("ru_nsignals", ru.ru_nsignals),
        ("ru_nvcsw", ru.ru_nvcsw),
        ("ru_nivcsw", ru.ru_nivcsw),
        ("ru_nswap", ru.ru_nswap),
        ("ru_utime.tv_usec", ru.ru_utime.tv_usec),
        ("ru_utime.tv_sec", ru.ru_utime.tv_sec),
        ("ru_stime.tv_usec", ru.ru_stime.tv_usec),
        ("ru_stime.tv_sec", ru.ru_stime.tv_sec),
    ];
    for (k, v) in fields {
        set(&mut a, k, Value::Long(v));
    }
    assign_ref(target, arr(a));
}

fn pcntl_waitpid(_vm: &mut Vm, args: &[Value]) -> R {
    let pid = long(args, 0);
    let z_status = ref_arg(args, 1).cloned().unwrap_or(Value::Undef);
    let options = long_or(args, 2, 0);
    let z_rusage = ref_arg(args, 3).cloned();
    let mut status = z_status.deref().to_long() as libc::c_int;
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    let child = if let Some(r) = &z_rusage {
        assign_ref(r, arr(PhpArray::new()));
        unsafe {
            libc::wait4(
                pid as libc::pid_t,
                &mut status,
                options as libc::c_int,
                &mut ru,
            )
        }
    } else {
        unsafe { libc::waitpid(pid as libc::pid_t, &mut status, options as libc::c_int) }
    };
    if child < 0 {
        set_last_error(errno());
    }
    if child > 0 {
        if let Some(r) = &z_rusage {
            rusage_to_array(&ru, r);
        }
    }
    assign_ref(&z_status, Value::Long(status as i64));
    Ok(Value::Long(child as i64))
}

fn pcntl_wait(_vm: &mut Vm, args: &[Value]) -> R {
    let z_status = ref_arg(args, 0).cloned().unwrap_or(Value::Undef);
    let options = long_or(args, 1, 0);
    let z_rusage = ref_arg(args, 2).cloned();
    let mut status = z_status.deref().to_long() as libc::c_int;
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    let child = if let Some(r) = &z_rusage {
        assign_ref(r, arr(PhpArray::new()));
        unsafe { libc::wait4(-1, &mut status, options as libc::c_int, &mut ru) }
    } else if options != 0 {
        unsafe {
            libc::wait4(
                -1,
                &mut status,
                options as libc::c_int,
                std::ptr::null_mut(),
            )
        }
    } else {
        unsafe { libc::wait(&mut status) }
    };
    if child < 0 {
        set_last_error(errno());
    }
    if child > 0 {
        if let Some(r) = &z_rusage {
            rusage_to_array(&ru, r);
        }
    }
    assign_ref(&z_status, Value::Long(status as i64));
    Ok(Value::Long(child as i64))
}

fn pcntl_waitid(_vm: &mut Vm, args: &[Value]) -> R {
    let idtype = long_or(args, 0, libc::P_ALL as i64);
    let id = opt_long(args, 1).unwrap_or(0);
    let user_siginfo = ref_arg(args, 2).cloned();
    let options = long_or(args, 3, libc::WEXITED as i64);
    let z_rusage = ref_arg(args, 4).cloned();
    set_errno(0);
    let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let status = if let Some(r) = &z_rusage {
        assign_ref(r, arr(PhpArray::new()));
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        let st = unsafe {
            libc::syscall(
                libc::SYS_waitid,
                idtype as libc::c_int,
                id as libc::id_t,
                &mut si as *mut libc::siginfo_t,
                options as libc::c_int,
                &mut ru as *mut libc::rusage,
            )
        } as i32;
        if st == 0 {
            rusage_to_array(&ru, r);
        }
        st
    } else {
        unsafe {
            libc::waitid(
                idtype as libc::idtype_t,
                id as libc::id_t,
                &mut si,
                options as libc::c_int,
            )
        }
    };
    if status == -1 {
        set_last_error(errno());
        return Ok(Value::False);
    }
    if let Some(u) = &user_siginfo {
        assign_ref(u, siginfo_array(libc::SIGCHLD, &si));
    }
    Ok(Value::True)
}

fn status_arg(args: &[Value]) -> libc::c_int {
    long(args, 0) as libc::c_int
}

fn pcntl_wifexited(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(bool_val(libc::WIFEXITED(status_arg(args))))
}

fn pcntl_wifstopped(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(bool_val(libc::WIFSTOPPED(status_arg(args))))
}

fn pcntl_wifcontinued(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(bool_val(libc::WIFCONTINUED(status_arg(args))))
}

fn pcntl_wifsignaled(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(bool_val(libc::WIFSIGNALED(status_arg(args))))
}

fn pcntl_wexitstatus(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(Value::Long(libc::WEXITSTATUS(status_arg(args)) as i64))
}

fn pcntl_wtermsig(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(Value::Long(libc::WTERMSIG(status_arg(args)) as i64))
}

fn pcntl_wstopsig(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(Value::Long(libc::WSTOPSIG(status_arg(args)) as i64))
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

/// php_signal4(): Err(()) for SIG_ERR.
fn install(
    signo: i32,
    handler: usize,
    restart: bool,
    mask_all: bool,
    siginfo: bool,
) -> Result<(), ()> {
    unsafe {
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = handler;
        if mask_all {
            libc::sigfillset(&mut act.sa_mask);
        } else {
            libc::sigemptyset(&mut act.sa_mask);
        }
        act.sa_flags = libc::SA_ONSTACK;
        if siginfo {
            act.sa_flags |= libc::SA_SIGINFO;
        }
        if restart {
            act.sa_flags |= libc::SA_RESTART;
        }
        if libc::sigaction(signo, &act, std::ptr::null_mut()) != 0 {
            return Err(());
        }
    }
    Ok(())
}

/// PHP builds with zend signals route php_signal4() through zend_sigaction(),
/// which turns a failing sigaction() (SIGKILL, SIGSTOP) into
/// `zend_error_noreturn(E_ERROR, ...)` rather than pcntl's "Error assigning
/// signal" warning.
#[cold]
fn sigaction_fatal(vm: &mut Vm, signo: i64) -> VmError {
    let line = vm.current_line;
    vm.emit_fatal_at(
        &format!("Error installing signal handler for {signo}"),
        line,
    )
}

fn pcntl_signal(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_signal";
    let signo = long(args, 0);
    let handle = arg(args, 1);
    let restart_arg = match arg(args, 2) {
        Value::Undef | Value::Null => None,
        v => Some(v.to_bool()),
    };
    if signo < 1 {
        return value_error(vm, f, 1, "must be greater than or equal to 1");
    }
    let ns = num_signals();
    if signo >= ns {
        return value_error(vm, f, 1, &format!("must be less than {}", ns));
    }
    CAPACITY.store(ns as usize, Ordering::Relaxed);
    let restart = match restart_arg {
        Some(r) => r,
        None => signo != libc::SIGALRM as i64,
    };
    if let Value::Long(h) = handle {
        if h != libc::SIG_DFL as i64 && h != libc::SIG_IGN as i64 {
            return value_error(
                vm,
                f,
                2,
                "must be either SIG_DFL or SIG_IGN when an integer value is given",
            );
        }
        if install(signo as i32, h as usize, restart, false, true).is_err() {
            return Err(sigaction_fatal(vm, signo));
        }
        HANDLERS.with(|t| t.borrow_mut().insert(signo, handle));
        return Ok(Value::True);
    }
    let ok = vm.zend_is_callable_ex(&handle, 0).ok;
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    if !ok {
        set_last_error(libc::EINVAL);
        return type_error(
            vm,
            f,
            2,
            &format!(
                "must be of type callable|int, {} given",
                Vm::value_type_name(&handle)
            ),
        );
    }
    let h: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) = signal_handler;
    if install(signo as i32, h as usize, restart, true, true).is_err() {
        return Err(sigaction_fatal(vm, signo));
    }
    HANDLERS.with(|t| t.borrow_mut().insert(signo, handle));
    Ok(Value::True)
}

fn pcntl_signal_get_handler(vm: &mut Vm, args: &[Value]) -> R {
    let signo = long(args, 0);
    let sigmax = (NSIG - 1).max(libc::SIGRTMAX() as i64);
    if signo < 1 || signo > sigmax {
        return value_error(
            vm,
            "pcntl_signal_get_handler",
            1,
            &format!("must be between 1 and {}", sigmax),
        );
    }
    Ok(HANDLERS
        .with(|t| t.borrow().get(&signo).cloned())
        .unwrap_or(Value::Long(libc::SIG_DFL as i64)))
}

fn pcntl_signal_dispatch(vm: &mut Vm, _args: &[Value]) -> R {
    dispatch(vm);
    Ok(Value::True)
}

fn pcntl_async_signals(_vm: &mut Vm, args: &[Value]) -> R {
    let old = ASYNC.load(Ordering::Relaxed);
    match arg(args, 0) {
        Value::Undef | Value::Null => {}
        v => ASYNC.store(v.to_bool(), Ordering::Relaxed),
    }
    Ok(bool_val(old))
}

/// php_pcntl_set_user_signal_infos()
fn signal_set(
    vm: &mut Vm,
    func: &str,
    signals: &Value,
    arg_num: usize,
    allow_empty: bool,
) -> Option<libc::sigset_t> {
    let items: Vec<Value> = match signals {
        Value::Array(a) => a.borrow().iter().map(|(_, v)| v.clone()).collect(),
        _ => Vec::new(),
    };
    if !allow_empty && items.is_empty() {
        let _ = value_error(vm, func, arg_num, "must not be empty");
        return None;
    }
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut set) };
    let ns = num_signals();
    for v in items {
        let v = v.deref();
        let Some(n) = try_get_long(&v) else {
            let _ = type_error(
                vm,
                func,
                arg_num,
                &format!(
                    "signals must be of type int, {} given",
                    Vm::value_type_name(&v)
                ),
            );
            return None;
        };
        if n < 1 || n >= ns {
            let _ = value_error(
                vm,
                func,
                arg_num,
                &format!("signals must be between 1 and {}", ns - 1),
            );
            return None;
        }
        set_errno(0);
        if unsafe { libc::sigaddset(&mut set, n as libc::c_int) } != 0 {
            let e = errno();
            set_last_error(e);
            warn(vm, func, &strerror(e));
            return None;
        }
    }
    Some(set)
}

fn pcntl_sigprocmask(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_sigprocmask";
    let how = long(args, 0);
    if how != libc::SIG_BLOCK as i64
        && how != libc::SIG_UNBLOCK as i64
        && how != libc::SIG_SETMASK as i64
    {
        return value_error(
            vm,
            f,
            1,
            "must be one of SIG_BLOCK, SIG_UNBLOCK, or SIG_SETMASK",
        );
    }
    let Some(set) = signal_set(vm, f, &arg(args, 1), 2, how == libc::SIG_SETMASK as i64) else {
        return Ok(if vm.current_exception.is_some() {
            Value::Null
        } else {
            Value::False
        });
    };
    let mut old: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut old) };
    if unsafe { libc::sigprocmask(how as libc::c_int, &set, &mut old) } != 0 {
        let e = errno();
        set_last_error(e);
        warn(vm, f, &strerror(e));
        return Ok(Value::False);
    }
    if let Some(u) = ref_arg(args, 2) {
        let mut a = PhpArray::new();
        for s in 1..num_signals() {
            if unsafe { libc::sigismember(&old, s as libc::c_int) } == 1 {
                a.push(Value::Long(s));
            }
        }
        assign_ref(u, arr(a));
    }
    Ok(Value::True)
}

fn pcntl_sigwaitinfo(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_sigwaitinfo";
    let Some(set) = signal_set(vm, f, &arg(args, 0), 1, false) else {
        return Ok(if vm.current_exception.is_some() {
            Value::Null
        } else {
            Value::False
        });
    };
    set_errno(0);
    let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let mut signo = unsafe { sigwaitinfo(&set, &mut si) };
    if signo == -1 {
        let e = errno();
        set_last_error(e);
        warn(vm, f, &strerror(e));
        return Ok(Value::False);
    }
    if signo == 0 && si.si_signo != 0 {
        signo = si.si_signo;
    }
    if let Some(u) = ref_arg(args, 1) {
        if signo > 0 {
            assign_ref(u, siginfo_array(signo, &si));
        }
    }
    Ok(Value::Long(signo as i64))
}

fn pcntl_sigtimedwait(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_sigtimedwait";
    let Some(set) = signal_set(vm, f, &arg(args, 0), 1, false) else {
        return Ok(if vm.current_exception.is_some() {
            Value::Null
        } else {
            Value::False
        });
    };
    let sec = long_or(args, 2, 0);
    let nsec = long_or(args, 3, 0);
    if sec < 0 {
        return value_error(vm, f, 3, "must be greater than or equal to 0");
    }
    if !(0..1_000_000_000).contains(&nsec) {
        return value_error(vm, f, 4, "must be between 0 and 1e9");
    }
    if sec == 0 && nsec == 0 {
        return throw(
            vm,
            b"ValueError",
            "pcntl_sigtimedwait(): At least one of argument #3 ($seconds) or argument #4 ($nanoseconds) must be greater than 0",
        );
    }
    set_errno(0);
    let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let ts = libc::timespec {
        tv_sec: sec as libc::time_t,
        tv_nsec: nsec as libc::c_long,
    };
    let mut signo = unsafe { libc::sigtimedwait(&set, &mut si, &ts) };
    if signo == -1 {
        let e = errno();
        if e != libc::EAGAIN {
            set_last_error(e);
            warn(vm, f, &strerror(e));
        }
        return Ok(Value::False);
    }
    if signo == 0 && si.si_signo != 0 {
        signo = si.si_signo;
    }
    if let Some(u) = ref_arg(args, 1) {
        if signo > 0 {
            assign_ref(u, siginfo_array(signo, &si));
        }
    }
    Ok(Value::Long(signo as i64))
}

unsafe extern "C" {
    fn sigwaitinfo(set: *const libc::sigset_t, info: *mut libc::siginfo_t) -> libc::c_int;
}

/// pcntl_siginfo_to_zval() (into a new array)
fn siginfo_array(signo: i32, si: &libc::siginfo_t) -> Value {
    let mut a = PhpArray::new();
    set(&mut a, "signo", Value::Long(si.si_signo as i64));
    set(&mut a, "errno", Value::Long(si.si_errno as i64));
    set(&mut a, "code", Value::Long(si.si_code as i64));
    unsafe {
        match signo {
            libc::SIGCHLD => {
                set(&mut a, "status", Value::Long(si.si_status() as i64));
                set(&mut a, "utime", Value::Double(si.si_utime() as f64));
                set(&mut a, "stime", Value::Double(si.si_stime() as f64));
                set(&mut a, "pid", Value::Long(si.si_pid() as i64));
                set(&mut a, "uid", Value::Long(si.si_uid() as i64));
            }
            libc::SIGUSR1 | libc::SIGUSR2 => {
                set(&mut a, "pid", Value::Long(si.si_pid() as i64));
                set(&mut a, "uid", Value::Long(si.si_uid() as i64));
            }
            libc::SIGILL | libc::SIGFPE | libc::SIGSEGV | libc::SIGBUS => {
                set(
                    &mut a,
                    "addr",
                    Value::Double(si.si_addr() as usize as i64 as f64),
                );
            }
            libc::SIGPOLL => {
                // struct { long si_band; int si_fd; } after the header
                let base = si as *const libc::siginfo_t as *const u8;
                let band = std::ptr::read_unaligned(base.add(16) as *const libc::c_long);
                let fd = std::ptr::read_unaligned(
                    base.add(16 + std::mem::size_of::<libc::c_long>()) as *const libc::c_int,
                );
                set(&mut a, "band", Value::Long(band as i64));
                set(&mut a, "fd", Value::Long(fd as i64));
            }
            _ => {}
        }
        if libc::SIGRTMIN() <= signo && signo <= libc::SIGRTMAX() {
            set(&mut a, "pid", Value::Long(si.si_pid() as i64));
            set(&mut a, "uid", Value::Long(si.si_uid() as i64));
        }
    }
    arr(a)
}

// ---------------------------------------------------------------------------
// Priorities, namespaces, CPUs
// ---------------------------------------------------------------------------

fn pcntl_getpriority(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_getpriority";
    let pid = opt_long(args, 0).unwrap_or(0);
    let who = long_or(args, 1, libc::PRIO_PROCESS as i64);
    set_errno(0);
    let pri = unsafe { libc::getpriority(who as libc::__priority_which_t, pid as libc::id_t) };
    let e = errno();
    if e != 0 {
        set_last_error(e);
        match e {
            libc::ESRCH => warn(
                vm,
                f,
                &format!(
                    "Error {}: No process was located using the given parameters",
                    e
                ),
            ),
            libc::EINVAL => {
                return value_error(
                    vm,
                    f,
                    2,
                    "must be one of PRIO_PGRP, PRIO_USER, or PRIO_PROCESS",
                );
            }
            _ => warn(vm, f, &format!("Unknown error {} has occurred", e)),
        }
        return Ok(Value::False);
    }
    Ok(Value::Long(pri as i64))
}

fn pcntl_setpriority(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_setpriority";
    let pri = long(args, 0);
    let pid = opt_long(args, 1).unwrap_or(0);
    let who = long_or(args, 2, libc::PRIO_PROCESS as i64);
    if unsafe {
        libc::setpriority(
            who as libc::__priority_which_t,
            pid as libc::id_t,
            pri as libc::c_int,
        )
    } != 0
    {
        let e = errno();
        set_last_error(e);
        match e {
            libc::ESRCH => warn(
                vm,
                f,
                &format!(
                    "Error {}: No process was located using the given parameters",
                    e
                ),
            ),
            libc::EINVAL => {
                return value_error(
                    vm,
                    f,
                    3,
                    "must be one of PRIO_PGRP, PRIO_USER, or PRIO_PROCESS",
                );
            }
            libc::EPERM => warn(
                vm,
                f,
                &format!(
                    "Error {}: A process was located, but neither its effective nor real user ID matched the effective user ID of the caller",
                    e
                ),
            ),
            libc::EACCES => warn(
                vm,
                f,
                &format!(
                    "Error {}: Only a super user may attempt to increase the process priority",
                    e
                ),
            ),
            _ => warn(vm, f, &format!("Unknown error {} has occurred", e)),
        }
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn pcntl_get_last_error(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(LAST_ERROR.with(|c| c.get()) as i64))
}

fn pcntl_strerror(_vm: &mut Vm, args: &[Value]) -> R {
    Ok(Value::String(PhpString::from_string(strerror(
        long(args, 0) as i32,
    ))))
}

fn pcntl_unshare(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_unshare";
    let flags = long(args, 0);
    if unsafe { libc::unshare(flags as libc::c_int) } == -1 {
        let e = errno();
        set_last_error(e);
        match e {
            libc::EINVAL => {
                return value_error(
                    vm,
                    f,
                    1,
                    "must be a combination of CLONE_* flags, or at least one flag is unsupported by the kernel",
                );
            }
            libc::ENOMEM => warn(
                vm,
                f,
                &format!("Error {}: Insufficient memory for unshare", e),
            ),
            libc::EPERM => warn(
                vm,
                f,
                &format!("Error {}: No privilege to use these flags", e),
            ),
            libc::ENOSPC => warn(
                vm,
                f,
                &format!(
                    "Error {}: Reached the maximum nesting limit for one of the specified namespaces",
                    e
                ),
            ),
            libc::EUSERS => warn(
                vm,
                f,
                &format!(
                    "Error {}: Reached the maximum nesting limit for the user namespace",
                    e
                ),
            ),
            _ => warn(vm, f, &format!("Unknown error {} has occurred", e)),
        }
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn pcntl_setns(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_setns";
    let pid = opt_long(args, 0).unwrap_or_else(|| unsafe { libc::getpid() } as i64);
    let nstype = long_or(args, 1, libc::CLONE_NEWNET as i64);
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) } as i32;
    if fd == -1 {
        let e = errno();
        set_last_error(e);
        match e {
            libc::EINVAL | libc::ESRCH => {
                return value_error(vm, f, 1, &format!("is not a valid process ({})", pid));
            }
            libc::ENFILE => warn(
                vm,
                f,
                &format!("Error {}: File descriptors per-process limit reached", e),
            ),
            libc::ENODEV => warn(
                vm,
                f,
                &format!("Error {}: Anonymous inode fs unsupported", e),
            ),
            libc::ENOMEM => warn(
                vm,
                f,
                &format!("Error {}: Insufficient memory for pidfd_open", e),
            ),
            _ => warn(vm, f, &format!("Error {}", e)),
        }
        return Ok(Value::False);
    }
    let ret = unsafe { libc::setns(fd, nstype as libc::c_int) };
    let e = errno();
    unsafe { libc::close(fd) };
    if ret == -1 {
        set_last_error(e);
        match e {
            libc::ESRCH => {
                return value_error(vm, f, 1, &format!("process no longer available ({})", pid));
            }
            libc::EINVAL => {
                return value_error(
                    vm,
                    f,
                    2,
                    &format!("is an invalid nstype ({})", nstype as i32),
                );
            }
            libc::EPERM => warn(
                vm,
                f,
                &format!("Error {}: No required capability for this process", e),
            ),
            _ => warn(vm, f, &format!("Error {}", e)),
        }
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn affinity_error(vm: &mut Vm, f: &str, pid: i64, setting: bool) -> R {
    let e = errno();
    set_last_error(e);
    match e {
        libc::ESRCH => return value_error(vm, f, 1, &format!("invalid process ({})", pid)),
        libc::EPERM => warn(vm, f, "Calling process not having the proper privileges"),
        libc::EINVAL => {
            if setting {
                return value_error(
                    vm,
                    f,
                    2,
                    "invalid cpu affinity mask size or unmapped cpu id(s)",
                );
            }
            return throw(vm, b"ValueError", "invalid cpu affinity mask size");
        }
        _ => warn(vm, f, &format!("Error {}", e)),
    }
    Ok(Value::False)
}

fn pcntl_getcpuaffinity(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_getcpuaffinity";
    let pid = opt_long(args, 0).unwrap_or(0);
    let mut mask: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::sched_getaffinity(
            pid as libc::pid_t,
            std::mem::size_of::<libc::cpu_set_t>(),
            &mut mask,
        )
    } != 0
    {
        return affinity_error(vm, f, pid, false);
    }
    let maxcpus = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_CONF) } as u64;
    let mut a = PhpArray::new();
    for i in 0..maxcpus {
        if (i as usize) < libc::CPU_SETSIZE as usize
            && unsafe { libc::CPU_ISSET(i as usize, &mask) }
        {
            a.push(Value::Long(i as i64));
        }
    }
    Ok(arr(a))
}

fn pcntl_setcpuaffinity(vm: &mut Vm, args: &[Value]) -> R {
    let f = "pcntl_setcpuaffinity";
    let pid = opt_long(args, 0).unwrap_or(0);
    let items: Vec<Value> = match arg(args, 1) {
        Value::Array(a) => a.borrow().iter().map(|(_, v)| v.clone()).collect(),
        _ => Vec::new(),
    };
    if items.is_empty() {
        return value_error(vm, f, 2, "must not be empty");
    }
    let maxcpus = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_CONF) } as i64;
    let mut mask: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { libc::CPU_ZERO(&mut mask) };
    for v in items {
        let v = v.deref();
        let cpu = match &v {
            Value::Long(n) => *n,
            Value::String(s) => match handle_numeric(s.as_bytes()) {
                Some(n) => n,
                None => {
                    return value_error(
                        vm,
                        f,
                        2,
                        &format!("cpu id invalid value ({})", s.to_string_lossy()),
                    );
                }
            },
            other => {
                return type_error(
                    vm,
                    f,
                    2,
                    &format!(
                        "value must be of type int|string, {} given",
                        Vm::value_type_name(other)
                    ),
                );
            }
        };
        if cpu < 0 || cpu >= maxcpus {
            return value_error(
                vm,
                f,
                2,
                &format!("cpu id must be between 0 and {} ({})", maxcpus, cpu),
            );
        }
        unsafe { libc::CPU_SET(cpu as usize, &mut mask) };
    }
    if unsafe {
        libc::sched_setaffinity(
            pid as libc::pid_t,
            std::mem::size_of::<libc::cpu_set_t>(),
            &mask,
        )
    } != 0
    {
        return affinity_error(vm, f, pid, true);
    }
    Ok(Value::True)
}

/// ZEND_HANDLE_NUMERIC_STR(): a canonical decimal integer string.
fn handle_numeric(s: &[u8]) -> Option<i64> {
    let digits = s.strip_prefix(b"-").unwrap_or(s);
    if digits.is_empty()
        || !digits.iter().all(|c| c.is_ascii_digit())
        || (digits[0] == b'0' && (digits.len() > 1 || s.len() != digits.len()))
    {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse::<i64>().ok()
}

fn pcntl_getcpu(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Long(unsafe { libc::sched_getcpu() } as i64))
}
