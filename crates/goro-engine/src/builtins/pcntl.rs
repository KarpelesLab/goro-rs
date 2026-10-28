//! A minimal part of ext/pcntl and ext/posix (PHP manual: Process
//! Control): fork(), waiting for children, alarm() and kill(), enough for
//! the forking test servers of ext/standard/tests/http/server.inc. The
//! module stays unlisted: extension_loaded('pcntl') is still false.

use goro_runtime::Value;

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "pcntl_min",
    functions: &[
        ("function pcntl_fork(): int", pcntl_fork),
        (
            "function pcntl_waitpid(int $process_id, &$status, int $flags = 0, &$resource_usage = []): int",
            pcntl_waitpid,
        ),
        (
            "function pcntl_wait(&$status, int $flags = 0, &$resource_usage = []): int",
            pcntl_wait,
        ),
        ("function pcntl_alarm(int $seconds): int", pcntl_alarm),
        (
            "function pcntl_wifexited(int $status): bool",
            pcntl_wifexited,
        ),
        (
            "function pcntl_wifstopped(int $status): bool",
            pcntl_wifstopped,
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
            "function posix_kill(int $process_id, int $signal): bool",
            posix_kill,
        ),
    ],
    constants: &[
        ("WNOHANG", ConstVal::Int(libc::WNOHANG as i64)),
        ("WUNTRACED", ConstVal::Int(libc::WUNTRACED as i64)),
        ("WCONTINUED", ConstVal::Int(libc::WCONTINUED as i64)),
        ("SIGHUP", ConstVal::Int(libc::SIGHUP as i64)),
        ("SIGINT", ConstVal::Int(libc::SIGINT as i64)),
        ("SIGQUIT", ConstVal::Int(libc::SIGQUIT as i64)),
        ("SIGKILL", ConstVal::Int(libc::SIGKILL as i64)),
        ("SIGUSR1", ConstVal::Int(libc::SIGUSR1 as i64)),
        ("SIGUSR2", ConstVal::Int(libc::SIGUSR2 as i64)),
        ("SIGALRM", ConstVal::Int(libc::SIGALRM as i64)),
        ("SIGTERM", ConstVal::Int(libc::SIGTERM as i64)),
        ("SIGCHLD", ConstVal::Int(libc::SIGCHLD as i64)),
        ("SIGCONT", ConstVal::Int(libc::SIGCONT as i64)),
        ("SIGSTOP", ConstVal::Int(libc::SIGSTOP as i64)),
    ],
    listed: false,
    ..Module::EMPTY
};

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// pcntl_fork(): output written so far goes out first, so the child
/// does not repeat it (PHP's CLI writes unbuffered).
fn pcntl_fork(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.out.flush();
    // SAFETY: the engine is single-threaded; the child continues with a
    // copy of this process.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let e = errno();
        vm.fwarn(a, &format!("Error {}: {}", e, super::exec::strerror(e)))?;
    }
    Ok(Value::Int(pid as i64))
}

fn wait_common(vm: &mut Vm, a: &Args, pid: i32, status_idx: usize, flags: i64) -> R<Value> {
    let mut st: libc::c_int = 0;
    let r = loop {
        // SAFETY: waitpid with a valid status pointer.
        let r = unsafe { libc::waitpid(pid, &mut st, flags as libc::c_int) };
        if r < 0 && errno() == libc::EINTR {
            continue;
        }
        break r;
    };
    if r < 0 {
        let e = errno();
        vm.fwarn(a, &format!("Error {}: {}", e, super::exec::strerror(e)))?;
    }
    a.set_ref(status_idx, Value::Int(st as i64));
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

fn posix_kill(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pid = a.int(0) as libc::pid_t;
    let sig = a.int(1) as libc::c_int;
    // SAFETY: kill() has no memory effects.
    Ok(Value::Bool(unsafe { libc::kill(pid, sig) } == 0))
}
