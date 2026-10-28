//! Running the PHP binary under test as a subprocess, with a timeout.
//!
//! The binary is started directly (no shell), in its own process group, so
//! that a timeout kills everything the test started. stdout and stderr share
//! one pipe, so their output interleaves in the order it was written: the
//! documentation says CAPTURE_STDIO defaults to capturing all of STDIN,
//! STDOUT and STDERR. A stream left out of CAPTURE_STDIO is inherited from
//! the runner (tests/output/stream_isatty_out.phpt expects an uncaptured
//! STDIN and STDERR to be terminals).

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
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
    let mut cmd = Command::new(r.program);
    cmd.args(r.args).current_dir(r.cwd).process_group(0);
    if let Some(limit) = r.mem_limit {
        use std::os::unix::process::CommandExt;
        // SAFETY: setrlimit is async-signal-safe; only the child is affected.
        unsafe {
            cmd.pre_exec(move || {
                let rl = libc::rlimit {
                    rlim_cur: limit as libc::rlim_t,
                    rlim_max: limit as libc::rlim_t,
                };
                libc::setrlimit(libc::RLIMIT_AS, &rl);
                Ok(())
            });
        }
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
