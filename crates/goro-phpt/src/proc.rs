//! Running the PHP binary under test as a subprocess, with a timeout.
//!
//! The binary is started directly (no shell), in its own process group, so
//! that a timeout kills everything the test started. stdout and stderr share
//! one pipe, so their output interleaves in the order it was written: the
//! documentation says CAPTURE_STDIO defaults to capturing all of STDIN,
//! STDOUT and STDERR. A stream left out of CAPTURE_STDIO is inherited from
//! the runner (tests/output/stream_isatty_out.phpt expects an uncaptured
//! STDIN and STDERR to be terminals).
//!
//! Nothing a test starts outlives the runner or holds what the runner holds
//! (such as the `flock` file descriptor of a suite lock): children get no
//! file descriptor above stderr, and a watchdog process kills the running
//! tests' process groups when the runner dies, however it dies.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// Which standard streams are connected to the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capture {
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
}

impl Capture {
    pub const ALL: Capture = Capture {
        stdin: true,
        stdout: true,
        stderr: true,
    };
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
    Timeout,
    /// Output passed the size limit and the process was killed.
    TooMuchOutput,
}

#[derive(Debug)]
pub struct RunOutput {
    pub output: Vec<u8>,
    pub exit: Exit,
    pub elapsed: Duration,
}

/// Output beyond this is not kept and the process is killed: no test expects
/// this much, and a runaway loop must not exhaust memory.
pub const OUTPUT_LIMIT: usize = 64 << 20;

pub struct Run<'a> {
    pub program: &'a Path,
    pub args: &'a [OsString],
    pub cwd: &'a Path,
    pub env: &'a [(OsString, OsString)],
    pub stdin: Option<&'a [u8]>,
    pub capture: Capture,
    pub timeout: Duration,
    /// Address-space limit of the child in bytes (RLIMIT_AS); none: no
    /// limit. Keeps a runaway test from exhausting the machine's memory.
    pub mem_limit: Option<u64>,
}

/// The socket to the watchdog process, started by the first test.
static WATCHDOG: OnceLock<Option<libc::c_int>> = OnceLock::new();

/// Tells the watchdog that process group `pid` started (positive) or ended
/// (negative). A message is small enough to be written whole.
fn tell_watchdog(message: i32) {
    if let Some(Some(fd)) = WATCHDOG.get() {
        // SAFETY: writes four bytes from a live local. MSG_NOSIGNAL: a
        // watchdog that was killed must not take the runner with it.
        unsafe {
            libc::send(
                *fd,
                (&raw const message).cast(),
                size_of::<i32>(),
                libc::MSG_NOSIGNAL,
            );
        }
    }
}

/// Starts the watchdog once. None if it can't be started: the tests still
/// run, and their direct children still die with the runner.
fn start_watchdog() {
    WATCHDOG.get_or_init(|| {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: fds has room for the two descriptors. They are close-on-exec,
        // so test processes don't keep the runner's end open.
        let r = unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        };
        if r != 0 {
            return None;
        }
        // SAFETY: the child only makes system calls (see watchdog_main).
        match unsafe { libc::fork() } {
            -1 => {
                // SAFETY: both descriptors are ours.
                unsafe {
                    libc::close(fds[0]);
                    libc::close(fds[1]);
                }
                None
            }
            0 => watchdog_main(fds[1]),
            _ => {
                // SAFETY: the watchdog's end, unused here.
                unsafe { libc::close(fds[1]) };
                Some(fds[0])
            }
        }
    });
}

/// The watchdog: remembers the groups of running tests until the runner's
/// end of the socket closes, then kills them. It runs in a forked copy of a
/// threaded process, so it only makes system calls (no allocation).
fn watchdog_main(fd: libc::c_int) -> ! {
    const MAX_GROUPS: usize = 1024;
    let mut groups = [0i32; MAX_GROUPS];
    let mut count = 0;
    // SAFETY: plain system calls. The watchdog keeps only its socket: it must
    // hold neither the runner's stdout (a pipeline reading it would wait) nor
    // an inherited lock descriptor. Signals sent to the runner's process
    // group are not for it: it ends when the runner is gone.
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(sig, libc::SIG_IGN);
        }
        libc::setsid();
        libc::dup2(fd, 0);
        libc::syscall(libc::SYS_close_range, 1u32, u32::MAX, 0u32);
    }
    let mut buf = [0i32; 64];
    loop {
        // SAFETY: reads into a live local of that size.
        let n = unsafe { libc::read(0, buf.as_mut_ptr().cast(), size_of_val(&buf)) };
        if n == 0 {
            break;
        }
        if n < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        for &message in &buf[..n as usize / size_of::<i32>()] {
            if message > 0 {
                if count < MAX_GROUPS {
                    groups[count] = message;
                    count += 1;
                }
            } else if let Some(i) = groups[..count].iter().position(|&g| g == -message) {
                count -= 1;
                groups[i] = groups[count];
            }
        }
    }
    for &g in &groups[..count] {
        kill_group(g);
    }
    // SAFETY: ends this process without running the runner's exit handlers.
    unsafe { libc::_exit(0) }
}

fn kill_group(pid: i32) {
    // SAFETY: plain system call; a negative pid addresses the process group.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

/// Waits until `pid` has exited without reaping it, so its process group id
/// stays reserved until the caller has killed the group.
fn wait_exited_no_reap(pid: i32) {
    loop {
        // SAFETY: siginfo_t is plain data; waitid fills it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if r == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

pub fn run(r: &Run<'_>) -> std::io::Result<RunOutput> {
    let start = Instant::now();
    start_watchdog();
    let mut cmd = Command::new(r.program);
    cmd.args(r.args).current_dir(r.cwd).process_group(0);
    let limit = r.mem_limit;
    // SAFETY: getpid has no preconditions.
    let runner = unsafe { libc::getpid() };
    // SAFETY: only async-signal-safe system calls; only the child is affected.
    // The standard streams are already in place when this runs.
    unsafe {
        cmd.pre_exec(move || {
            if let Some(limit) = limit {
                let rl = libc::rlimit {
                    rlim_cur: limit as libc::rlim_t,
                    rlim_max: limit as libc::rlim_t,
                };
                libc::setrlimit(libc::RLIMIT_AS, &rl);
            }
            // Dies with the runner's thread, which waits for this process
            // (the signal can't be missed: check the parent after setting it).
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong);
            if libc::getppid() != runner {
                libc::_exit(127);
            }
            // Nothing above stderr reaches the test, whatever the runner
            // inherited (no test is given any such descriptor).
            libc::syscall(
                libc::SYS_close_range,
                3u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            );
            Ok(())
        });
    }
    for (k, v) in r.env {
        cmd.env(k, v);
    }
    let (mut reader, writer) = std::io::pipe()?;
    if r.capture.stdout {
        cmd.stdout(Stdio::from(writer.try_clone()?));
    } else {
        cmd.stdout(Stdio::inherit());
    }
    if r.capture.stderr {
        cmd.stderr(Stdio::from(writer));
    } else {
        drop(writer);
        cmd.stderr(Stdio::inherit());
    }
    cmd.stdin(if r.capture.stdin {
        Stdio::piped()
    } else {
        Stdio::inherit()
    });
    let mut child = cmd.spawn()?;
    // The Command still holds the write end of the pipe; drop it so the
    // reader sees end-of-file once the child's side closes.
    drop(cmd);
    let pid = child.id() as i32;
    tell_watchdog(pid);

    if let Some(mut stdin) = child.stdin.take() {
        let data = r.stdin.unwrap_or_default().to_vec();
        if !data.is_empty() {
            thread::spawn(move || {
                let _ = stdin.write_all(&data);
            });
        }
    }

    let (out_tx, out_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        let mut over = false;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len() + n > OUTPUT_LIMIT {
                        over = true;
                        kill_group(pid);
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = out_tx.send((buf, over));
    });

    let (exit_tx, exit_rx) = mpsc::channel();
    thread::spawn(move || {
        wait_exited_no_reap(pid);
        let _ = exit_tx.send(());
    });
    let timed_out = exit_rx.recv_timeout(r.timeout).is_err();
    // Kill the whole group: on timeout that stops the test, otherwise it
    // removes background processes the test left behind.
    kill_group(pid);
    if timed_out {
        let _ = exit_rx.recv();
    }
    // Before the wait below frees the group id for reuse.
    tell_watchdog(-pid);
    let status = child.wait()?;
    // A process that left the group (setsid) may keep the pipe open; don't
    // wait for it forever.
    let (output, over) = out_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    let exit = if over {
        Exit::TooMuchOutput
    } else if timed_out {
        Exit::Timeout
    } else if let Some(sig) = status.signal() {
        Exit::Signal(sig)
    } else {
        Exit::Code(status.code().unwrap_or(-1))
    };
    Ok(RunOutput {
        output,
        exit,
        elapsed: start.elapsed(),
    })
}
