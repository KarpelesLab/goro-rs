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
