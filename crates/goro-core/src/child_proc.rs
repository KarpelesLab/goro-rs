//! The process state that commands run by exec(), shell_exec(), system(),
//! passthru(), popen(), proc_open() and mail() inherit.
//!
//! PHP spawns them with posix_spawn(), so a child inherits the ignored
//! signals: SIGPIPE is ignored by the CLI (php_cli.c), so e.g. `yes | head -1`
//! sees EPIPE instead of being killed. The signal mask is not passed on (the
//! children of PHP 8.5 start with an empty one).
//! The goro binary also lowers its own RLIMIT_AS as a safety net; that cap
//! is goro's, not the script's, and must not reach the children.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(unix)]
unsafe extern "C" {
    static mut environ: *const *const libc::c_char;
}

static AS_RECORDED: AtomicBool = AtomicBool::new(false);
static AS_SOFT: AtomicU64 = AtomicU64::new(0);
static AS_HARD: AtomicU64 = AtomicU64::new(0);
/// The script changed RLIMIT_AS itself (posix_setrlimit()): children then
/// inherit the current limit, as they would under PHP.
static AS_USER_SET: AtomicBool = AtomicBool::new(false);

/// Remember RLIMIT_AS as it was before the binary lowered it.
pub fn record_startup_rlimit_as(soft: u64, hard: u64) {
    AS_SOFT.store(soft, Ordering::Relaxed);
    AS_HARD.store(hard, Ordering::Relaxed);
    AS_RECORDED.store(true, Ordering::Release);
}

/// posix_setrlimit(POSIX_RLIMIT_AS, ...) succeeded.
pub fn note_user_rlimit_as() {
    AS_USER_SET.store(true, Ordering::Relaxed);
}

/// RLIMIT_AS as the script sees it (posix_getrlimit()): the limit goro
/// started with, unless the script set one itself.
pub fn script_rlimit_as() -> Option<(u64, u64)> {
    if AS_RECORDED.load(Ordering::Acquire) && !AS_USER_SET.load(Ordering::Relaxed) {
        Some((
            AS_SOFT.load(Ordering::Relaxed),
            AS_HARD.load(Ordering::Relaxed),
        ))
    } else {
        None
    }
}

/// What the child must be given back, captured in the parent before forking.
#[cfg(unix)]
#[derive(Clone, Copy)]
pub struct ChildState {
    sigpipe_ignored: bool,
    rlimit_as: Option<libc::rlimit>,
}

#[cfg(unix)]
impl ChildState {
    pub fn capture() -> ChildState {
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            let sigpipe_ignored = libc::sigaction(libc::SIGPIPE, std::ptr::null(), &mut sa) == 0
                && sa.sa_sigaction == libc::SIG_IGN;
            let rlimit_as =
                if AS_RECORDED.load(Ordering::Acquire) && !AS_USER_SET.load(Ordering::Relaxed) {
                    Some(libc::rlimit {
                        rlim_cur: AS_SOFT.load(Ordering::Relaxed) as libc::rlim_t,
                        rlim_max: AS_HARD.load(Ordering::Relaxed) as libc::rlim_t,
                    })
                } else {
                    None
                };
            ChildState {
                sigpipe_ignored,
                rlimit_as,
            }
        }
    }

    /// In the forked child, before exec: async-signal-safe calls only.
    ///
    /// # Safety
    /// Only to be called between fork() and exec().
    pub unsafe fn apply_in_child(&self) {
        unsafe {
            self.restore_rlimit_as();
            libc::signal(
                libc::SIGPIPE,
                if self.sigpipe_ignored {
                    libc::SIG_IGN
                } else {
                    libc::SIG_DFL
                },
            );
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigprocmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
        }
    }

    /// apply_in_child() without resetting the signal mask (proc_open():
    /// the child inherits the parent's mask, as from php-src).
    ///
    /// # Safety
    /// Only to be called between fork() and exec().
    pub unsafe fn apply_in_child_keep_mask(&self) {
        unsafe {
            self.restore_rlimit_as();
            libc::signal(
                libc::SIGPIPE,
                if self.sigpipe_ignored {
                    libc::SIG_IGN
                } else {
                    libc::SIG_DFL
                },
            );
        }
    }

    /// Give back the RLIMIT_AS goro started with (pcntl_exec(): the new
    /// image keeps the mask and dispositions as they are).
    pub fn restore_rlimit_as(&self) {
        if let Some(r) = &self.rlimit_as {
            unsafe { libc::setrlimit(libc::RLIMIT_AS, r) };
        }
    }

    /// Hook the state into a std Command (which resets SIGPIPE and the mask
    /// in the child before running pre_exec closures).
    pub fn apply_to(self, cmd: &mut std::process::Command) {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(move || {
                self.apply_in_child();
                Ok(())
            });
        }
    }
}

/// Prepare a std Command spawned on behalf of the script.
#[cfg(unix)]
pub fn prepare_command(cmd: &mut std::process::Command) -> &mut std::process::Command {
    ChildState::capture().apply_to(cmd);
    cmd
}

#[cfg(not(unix))]
pub fn prepare_command(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd
}

/// What a child is set up with before its exec (see spawn_child()).
#[cfg(unix)]
pub struct ChildSpec<'a> {
    pub errpipe: [i32; 2],
    /// (index, child end, parent end, has a parent end)
    pub child_fds: &'a [(i32, i32, i32, bool)],
    pub child_state: ChildState,
    pub cwd: *const libc::c_char,
    pub env: *const *const libc::c_char,
    pub prog: *const libc::c_char,
    pub argv: *const *const libc::c_char,
    /// execvp() (a PATH search) rather than execv().
    pub search_path: bool,
    /// The child keeps the parent's signal mask (proc_open()) rather than
    /// an empty one (the other functions running commands).
    pub keep_mask: bool,
    /// A child end already at its index loses its close-on-exec flag (as
    /// with std's Command) instead of being left as it is.
    pub same_fd_inherited: bool,
    /// The signal mask the child gets (filled in by spawn_child(): the
    /// parent's; see `keep_mask`).
    pub mask: libc::sigset_t,
}

/// Start a child as posix_spawn() does: a clone sharing
/// the parent's memory (CLONE_VM | CLONE_VFORK: the parent waits until it
/// called exec or exited), so that the address space is not copied.
/// Returns its pid, or -1 (errno set).
///
/// # Safety
/// The pointers in `spec` must stay valid until the call returns.
#[cfg(unix)]
#[inline(never)]
pub unsafe fn spawn_child(mut spec: ChildSpec<'_>) -> libc::pid_t {
    const STACK: usize = 128 * 1024;
    unsafe {
        // No signal handler of the parent may run on the child's side of
        // the shared memory: everything is blocked until the child reset
        // the caught signals to their default action.
        let mut all: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut all);
        libc::pthread_sigmask(libc::SIG_SETMASK, &all, &mut spec.mask);
        let saved_environ = environ;
        let mut stack = vec![0u8; STACK];
        let top = (stack.as_mut_ptr().add(STACK) as usize & !15) as *mut libc::c_void;
        let pid = libc::clone(
            child_main,
            top,
            libc::CLONE_VM | libc::CLONE_VFORK | libc::SIGCHLD,
            &mut spec as *mut ChildSpec<'_> as *mut libc::c_void,
        );
        let e = *libc::__errno_location();
        // (the child switched it for its exec)
        environ = saved_environ;
        libc::pthread_sigmask(libc::SIG_SETMASK, &spec.mask, std::ptr::null_mut());
        drop(stack);
        if pid < 0 {
            *libc::__errno_location() = e;
        }
        pid
    }
}

#[cfg(unix)]
/// The child of spawn_child(): async-signal-safe calls only (it runs on
/// the parent's memory).
extern "C" fn child_main(arg: *mut libc::c_void) -> libc::c_int {
    unsafe {
        let spec = &*(arg as *const ChildSpec<'_>);
        let fail = |e: i32| -> ! {
            if spec.errpipe[1] >= 0 {
                let b = e.to_ne_bytes();
                libc::write(spec.errpipe[1], b.as_ptr() as *const libc::c_void, 4);
            }
            libc::_exit(127);
        };
        // (exec would reset them anyway; the ignored ones stay ignored)
        for sig in 1..=libc::SIGRTMAX() {
            let mut sa: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(sig, std::ptr::null(), &mut sa) == 0
                && sa.sa_sigaction != libc::SIG_IGN
                && sa.sa_sigaction != libc::SIG_DFL
            {
                let mut dfl: libc::sigaction = std::mem::zeroed();
                dfl.sa_sigaction = libc::SIG_DFL;
                libc::sigaction(sig, &dfl, std::ptr::null_mut());
            }
        }
        if spec.errpipe[0] >= 0 {
            libc::close(spec.errpipe[0]);
        }
        for &(index, childend, parentend, has_parent) in spec.child_fds {
            if has_parent {
                libc::close(parentend);
            }
            if childend != index {
                if libc::dup2(childend, index) < 0 {
                    fail(*libc::__errno_location());
                }
                libc::close(childend);
            } else if spec.same_fd_inherited {
                let fl = libc::fcntl(childend, libc::F_GETFD);
                if fl >= 0 {
                    libc::fcntl(childend, libc::F_SETFD, fl & !libc::FD_CLOEXEC);
                }
            }
        }
        // SIGPIPE and goro's RLIMIT_AS go back to what goro started with.
        spec.child_state.apply_in_child_keep_mask();
        if !spec.cwd.is_null() && libc::chdir(spec.cwd) != 0 {
            fail(*libc::__errno_location());
        }
        if !spec.env.is_null() {
            environ = spec.env;
        }
        // The signal mask: the parent's for proc_open(), as in php-src
        // (Symfony Process blocks the signals a child should ignore around
        // proc_open()), an empty one otherwise. (`spec` is the parent's
        // memory: not changed.)
        let mut mask: libc::sigset_t = std::mem::zeroed();
        if spec.keep_mask {
            mask = spec.mask;
        } else {
            libc::sigemptyset(&mut mask);
        }
        libc::sigprocmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut());
        if spec.search_path {
            libc::execvp(spec.prog, spec.argv);
        } else {
            libc::execv(spec.prog, spec.argv);
        }
        fail(*libc::__errno_location());
    }
}

/// A command started by spawn_command(): std's `Child` for it.
#[cfg(unix)]
pub struct CmdChild {
    pub pid: libc::pid_t,
    pub stdin: Option<std::fs::File>,
    pub stdout: Option<std::fs::File>,
    status: Option<std::process::ExitStatus>,
}

#[cfg(unix)]
impl CmdChild {
    /// Wait for the child to exit (once: later calls give the same status).
    pub fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        use std::os::unix::process::ExitStatusExt;
        if let Some(s) = self.status {
            return Ok(s);
        }
        // (the parent's ends are closed first, as std's Child::wait() does)
        drop(self.stdin.take());
        let mut st = 0;
        loop {
            if unsafe { libc::waitpid(self.pid, &mut st, 0) } >= 0 {
                break;
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        let s = std::process::ExitStatus::from_raw(st);
        self.status = Some(s);
        Ok(s)
    }
}

/// Run `prog` (looked up in PATH unless it has a `/`) with `args`, its
/// stdin and/or stdout piped to the parent, the rest inherited: what
/// `prepare_command(Command::new(prog).args(args)...).spawn()` does
/// (an empty signal mask, SIGPIPE and RLIMIT_AS as goro started with),
/// without copying the address space (see spawn_child()).
#[cfg(unix)]
pub fn spawn_command(
    prog: &[u8],
    args: &[&[u8]],
    pipe_stdin: bool,
    pipe_stdout: bool,
) -> std::io::Result<CmdChild> {
    use std::ffi::CString;
    use std::os::unix::io::FromRawFd;
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
    let prog_c = CString::new(prog).map_err(|_| invalid())?;
    let mut argv_c = vec![prog_c.clone()];
    for a in args {
        argv_c.push(CString::new(*a).map_err(|_| invalid())?);
    }
    let mut argv: Vec<*const libc::c_char> = argv_c.iter().map(|c| c.as_ptr()).collect();
    argv.push(std::ptr::null());
    let pipe = || -> std::io::Result<[i32; 2]> {
        let mut p = [0i32; 2];
        if unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(p)
    };
    let close = |fds: &[i32]| {
        for &fd in fds {
            unsafe { libc::close(fd) };
        }
    };
    // (index, child end, parent end, has a parent end); the pipes are
    // close-on-exec, the descriptors dup2()ed from them are not.
    let mut fds: Vec<(i32, i32, i32, bool)> = Vec::new();
    let mut opened: Vec<i32> = Vec::new();
    if pipe_stdin {
        let p = pipe()?;
        opened.extend(p);
        fds.push((0, p[0], p[1], true));
    }
    if pipe_stdout {
        let p = match pipe() {
            Ok(p) => p,
            Err(e) => {
                close(&opened);
                return Err(e);
            }
        };
        opened.extend(p);
        fds.push((1, p[1], p[0], true));
    }
    let errpipe = match pipe() {
        Ok(p) => p,
        Err(e) => {
            close(&opened);
            return Err(e);
        }
    };
    let spec = ChildSpec {
        errpipe,
        child_fds: &fds,
        child_state: ChildState::capture(),
        cwd: std::ptr::null(),
        env: std::ptr::null(),
        prog: prog_c.as_ptr(),
        argv: argv.as_ptr(),
        search_path: !prog.contains(&b'/'),
        keep_mask: false,
        same_fd_inherited: true,
        mask: unsafe { std::mem::zeroed() },
    };
    let pid = unsafe { spawn_child(spec) };
    let spawn_err = std::io::Error::last_os_error();
    unsafe { libc::close(errpipe[1]) };
    // the child's ends
    for &(_, child_end, _, _) in &fds {
        unsafe { libc::close(child_end) };
    }
    let parent_ends: Vec<i32> = fds.iter().map(|f| f.2).collect();
    if pid < 0 {
        unsafe { libc::close(errpipe[0]) };
        close(&parent_ends);
        return Err(spawn_err);
    }
    // Did exec succeed? (the child reports its errno otherwise)
    let mut b = [0u8; 4];
    let n = loop {
        let n = unsafe { libc::read(errpipe[0], b.as_mut_ptr() as *mut libc::c_void, 4) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        break n;
    };
    unsafe { libc::close(errpipe[0]) };
    if n == 4 {
        let mut st = 0;
        unsafe { libc::waitpid(pid, &mut st, 0) };
        close(&parent_ends);
        return Err(std::io::Error::from_raw_os_error(i32::from_ne_bytes(b)));
    }
    let mut child = CmdChild {
        pid,
        stdin: None,
        stdout: None,
        status: None,
    };
    for &(index, _, parent_end, _) in &fds {
        let f = unsafe { std::fs::File::from_raw_fd(parent_end) };
        if index == 0 {
            child.stdin = Some(f);
        } else {
            child.stdout = Some(f);
        }
    }
    Ok(child)
}
