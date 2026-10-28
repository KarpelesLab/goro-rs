//! Program execution (PHP manual: "Program execution Functions"): exec(),
//! system(), passthru(), shell_exec(), proc_open() and friends, popen()
//! and pclose(), plus stream_select() over descriptor-backed streams.
//!
//! Commands given as strings run through `/bin/sh -c`; command arrays are
//! executed directly with a PATH search. Children are started with
//! fork()+exec(); a close-on-exec pipe reports a failed exec back, which
//! PHP 8.5 words as "posix_spawn() failed: ..." (observed). Error texts
//! are those observed on PHP 8.5 and in the tests
//! ext/standard/tests/general_functions/proc_open*.phpt.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use goro_runtime::{Key, KeyRef, PhpArray, PhpStr, ResRef, Value};

use super::file::{Backing, Stream, new_stream_res};
use super::string::sv;
use crate::class::TypeName;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function exec(string $command, &$output = null, &$result_code = null): string|false",
            exec,
        ),
        (
            "function system(string $command, &$result_code = null): string|false",
            system,
        ),
        (
            "function passthru(string $command, &$result_code = null): false|null",
            passthru,
        ),
        (
            "function shell_exec(string $command): string|false|null",
            shell_exec,
        ),
        (
            "function proc_open(array|string $command, array $descriptor_spec, &$pipes, ?string $cwd = null, ?array $env_vars = null, ?array $options = null)",
            proc_open,
        ),
        ("function proc_close($process): int", proc_close),
        (
            "function proc_terminate($process, int $signal = 15): bool",
            proc_terminate,
        ),
        ("function proc_get_status($process): array", proc_get_status),
        ("function proc_nice(int $priority): bool", proc_nice),
        ("function popen(string $command, string $mode)", popen),
        ("function pclose($handle): int", pclose),
        (
            "function stream_select(?array &$read, ?array &$write, ?array &$except, ?int $seconds, ?int $microseconds = null): int|false",
            stream_select,
        ),
    ],
    listed: false,
    ..Module::EMPTY
};

// ---------------------------------------------------------------- spawning

/// What the child does with one of its descriptors.
enum ChildFd {
    /// dup2() of a parent descriptor.
    Dup(RawFd),
    /// dup2() of another child descriptor (proc_open "redirect").
    Redirect(i64),
}

struct SpawnSpec {
    argv: Vec<CString>,
    /// Search PATH for argv[0] (command arrays).
    path_search: bool,
    env: Option<Vec<CString>>,
    cwd: Option<CString>,
    fds: Vec<(i64, ChildFd)>,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

pub(crate) fn strerror(e: i32) -> String {
    let s = std::io::Error::from_raw_os_error(e).to_string();
    // "No such file or directory (os error 2)" -> the C library text.
    match s.rfind(" (os error") {
        Some(p) => s[..p].to_string(),
        None => s,
    }
}

fn cstr(b: &[u8]) -> CString {
    CString::new(b.iter().copied().filter(|&c| c != 0).collect::<Vec<u8>>()).unwrap_or_default()
}

fn sh_argv(cmd: &[u8]) -> Vec<CString> {
    vec![cstr(b"/bin/sh"), cstr(b"-c"), cstr(cmd)]
}

/// Starts the child; returns its pid or the errno of a failed start.
fn spawn(vm: &mut Vm, spec: &SpawnSpec) -> Result<libc::pid_t, i32> {
    // The child's output must follow what was echoed so far.
    vm.out.flush();
    let mut ep = [0 as RawFd; 2];
    if unsafe { libc::pipe2(ep.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(errno());
    }
    let mut argv: Vec<*const libc::c_char> = spec.argv.iter().map(|c| c.as_ptr()).collect();
    argv.push(std::ptr::null());
    let envp: Option<Vec<*const libc::c_char>> = spec.env.as_ref().map(|e| {
        let mut v: Vec<*const libc::c_char> = e.iter().map(|c| c.as_ptr()).collect();
        v.push(std::ptr::null());
        v
    });
    let high = spec.fds.iter().map(|(t, _)| *t).max().unwrap_or(2).max(2) as RawFd + 1;
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let e = errno();
        unsafe {
            libc::close(ep[0]);
            libc::close(ep[1]);
        }
        return Err(e);
    }
    if pid == 0 {
        // Child: only async-signal-safe calls from here on.
        unsafe {
            let fail = |e: i32| -> ! {
                let b = e.to_ne_bytes();
                libc::write(ep[1], b.as_ptr() as *const libc::c_void, 4);
                libc::_exit(127);
            };
            // Move the sources out of the way of the targets first.
            let mut moved: [RawFd; 256] = [-1; 256];
            for (i, (_, how)) in spec.fds.iter().enumerate().take(256) {
                if let ChildFd::Dup(src) = how {
                    let n = libc::fcntl(*src, libc::F_DUPFD_CLOEXEC, high.max(3));
                    if n < 0 {
                        fail(errno());
                    }
                    moved[i] = n;
                }
            }
            for (i, (t, how)) in spec.fds.iter().enumerate().take(256) {
                if let ChildFd::Dup(_) = how
                    && libc::dup2(moved[i], *t as RawFd) < 0
                {
                    fail(errno());
                }
            }
            for (t, how) in spec.fds.iter() {
                if let ChildFd::Redirect(from) = how
                    && libc::dup2(*from as RawFd, *t as RawFd) < 0
                {
                    fail(errno());
                }
            }
            if let Some(d) = &spec.cwd
                && libc::chdir(d.as_ptr()) != 0
            {
                fail(errno());
            }
            // Rust ignores SIGPIPE; the program gets the default.
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            match (&envp, spec.path_search) {
                (Some(e), true) => {
                    libc::execvpe(argv[0], argv.as_ptr(), e.as_ptr());
                }
                (Some(e), false) => {
                    libc::execve(argv[0], argv.as_ptr(), e.as_ptr());
                }
                (None, true) => {
                    libc::execvp(argv[0], argv.as_ptr());
                }
                (None, false) => {
                    libc::execv(argv[0], argv.as_ptr());
                }
            }
            fail(errno());
        }
    }
    unsafe { libc::close(ep[1]) };
    let mut b = [0u8; 4];
    let mut got = 0;
    let mut f = unsafe { File::from_raw_fd(ep[0]) };
    while got < 4 {
        match f.read(&mut b[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    if got == 4 {
        let mut st = 0;
        unsafe { libc::waitpid(pid, &mut st, 0) };
        return Err(i32::from_ne_bytes(b));
    }
    Ok(pid)
}

/// Waits for a child; returns the raw wait status.
fn wait_blocking(pid: libc::pid_t) -> Option<i32> {
    let mut st = 0;
    loop {
        let r = unsafe { libc::waitpid(pid, &mut st, 0) };
        if r == pid {
            return Some(st);
        }
        if r < 0 && errno() == libc::EINTR {
            continue;
        }
        return None;
    }
}

fn exit_code(st: i32) -> i64 {
    if libc::WIFEXITED(st) {
        libc::WEXITSTATUS(st) as i64
    } else if libc::WIFSIGNALED(st) {
        libc::WTERMSIG(st) as i64
    } else {
        -1
    }
}

fn pipe_pair() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut p = [0 as RawFd; 2];
    if unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(p[0]), OwnedFd::from_raw_fd(p[1])) })
}

fn fd_stream(fd: OwnedFd, mode: &[u8]) -> ResRef {
    new_stream_res(Stream::new(
        Backing::File(File::from(fd)),
        mode,
        b"",
        "",
        "STDIO",
    ))
}

/// The descriptor behind a stream resource, if it has one.
fn stream_fd(r: &ResRef) -> Result<Option<RawFd>, &'static str> {
    let data = r.res().data.borrow();
    let Some(d) = data.as_ref() else {
        return Err("closed");
    };
    let Some(cell) = d.downcast_ref::<RefCell<Stream>>() else {
        return Err("not a stream");
    };
    let s = cell.borrow();
    Ok(match &s.backing {
        Backing::File(f) => Some(f.as_raw_fd()),
        Backing::Stdin => Some(0),
        Backing::Stdout | Backing::Output => Some(1),
        Backing::Stderr => Some(2),
        Backing::Native(n) => n.raw_fd(),
        _ => None,
    })
}

fn check_command(vm: &mut Vm, a: &Args) -> R<Option<PhpStr>> {
    let c = a.str(0);
    if c.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($command) must not be empty");
    }
    if c.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($command) must not contain any null bytes",
        );
    }
    Ok(Some(c))
}

/// Runs `/bin/sh -c cmd` with its stdout on a pipe; `each` gets the output
/// as it arrives. Returns the wait status.
fn run_capture(
    vm: &mut Vm,
    a: &Args,
    cmd: &[u8],
    mut each: impl FnMut(&mut Vm, &[u8]),
) -> R<Option<i32>> {
    let (rd, wr) = match pipe_pair() {
        Ok(p) => p,
        Err(e) => {
            vm.fwarn(a, &format!("Unable to fork [{}]", cmd_lossy(cmd)))?;
            let _ = e;
            return Ok(None);
        }
    };
    let spec = SpawnSpec {
        argv: sh_argv(cmd),
        path_search: false,
        env: None,
        cwd: None,
        fds: vec![(1, ChildFd::Dup(wr.as_raw_fd()))],
    };
    let pid = match spawn(vm, &spec) {
        Ok(p) => p,
        Err(_) => {
            vm.fwarn(a, &format!("Unable to fork [{}]", cmd_lossy(cmd)))?;
            return Ok(None);
        }
    };
    drop(wr);
    let mut f = File::from(rd);
    let mut buf = vec![0u8; 8192];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => each(vm, &buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    Ok(wait_blocking(pid))
}

fn cmd_lossy(c: &[u8]) -> String {
    String::from_utf8_lossy(c).into_owned()
}

fn status_code(st: Option<i32>) -> i64 {
    match st {
        Some(s) if libc::WIFEXITED(s) => libc::WEXITSTATUS(s) as i64,
        Some(_) => -1,
        None => -1,
    }
}

fn rtrim_ws(b: &[u8]) -> &[u8] {
    let mut e = b.len();
    while e > 0 && matches!(b[e - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c | 0) {
        e -= 1;
    }
    &b[..e]
}

/// PHP manual: exec() appends each output line, without trailing
/// whitespace, to `$output` and returns the last line.
fn exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(cmd) = check_command(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let mut out = Vec::new();
    let st = run_capture(vm, a, &cmd, |_, b| out.extend_from_slice(b))?;
    if st.is_none() {
        return Ok(Value::Bool(false));
    }
    let mut lines: Vec<&[u8]> = out.split(|&c| c == b'\n').collect();
    if out.last() == Some(&b'\n') || out.is_empty() {
        lines.pop();
    }
    let last = lines
        .last()
        .map(|l| rtrim_ws(l).to_vec())
        .unwrap_or_default();
    if a.len() > 1 {
        let mut arr = match a.ref_at(1).map(|r| r.get()) {
            Some(Value::Array(arr)) => arr,
            _ => PhpArray::new(),
        };
        for l in &lines {
            let _ = arr.push(sv(rtrim_ws(l).to_vec()));
        }
        a.set_ref(1, Value::Array(arr));
    }
    if a.len() > 2 {
        a.set_ref(2, Value::Int(status_code(st)));
    }
    Ok(sv(last))
}

/// PHP manual: system() passes the output through and returns its last
/// line.
fn system(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(cmd) = check_command(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let mut all = Vec::new();
    let st = run_capture(vm, a, &cmd, |vm, b| {
        vm.echo(b);
        all.extend_from_slice(b);
    })?;
    if st.is_none() {
        return Ok(Value::Bool(false));
    }
    if a.len() > 1 {
        a.set_ref(1, Value::Int(status_code(st)));
    }
    let mut lines: Vec<&[u8]> = all.split(|&c| c == b'\n').collect();
    if all.last() == Some(&b'\n') || all.is_empty() {
        lines.pop();
    }
    Ok(sv(lines
        .last()
        .map(|l| rtrim_ws(l).to_vec())
        .unwrap_or_default()))
}

fn passthru(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(cmd) = check_command(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let st = run_capture(vm, a, &cmd, |vm, b| vm.echo(b))?;
    if st.is_none() {
        return Ok(Value::Bool(false));
    }
    if a.len() > 1 {
        a.set_ref(1, Value::Int(status_code(st)));
    }
    Ok(Value::Null)
}

/// PHP manual: shell_exec() (and the backtick operator) returns the
/// output, null when there is none.
fn shell_exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(cmd) = check_command(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let mut out = Vec::new();
    let st = run_capture(vm, a, &cmd, |_, b| out.extend_from_slice(b))?;
    if st.is_none() {
        return Ok(Value::Bool(false));
    }
    if out.is_empty() {
        return Ok(Value::Null);
    }
    Ok(sv(out))
}

// ---------------------------------------------------------------- proc_open

/// A process resource ("process").
struct Proc {
    pid: libc::pid_t,
    command: PhpStr,
    /// The parent ends of the pipes, closed with the process.
    pipes: Vec<ResRef>,
    /// The reaped wait status.
    status: Option<i32>,
}

impl Drop for Proc {
    fn drop(&mut self) {
        for p in &self.pipes {
            p.close();
        }
        if self.status.is_none() {
            self.status = wait_blocking(self.pid);
        }
    }
}

fn with_proc<T>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut Vm, &mut Proc) -> R<T>) -> R<T> {
    let v = a.val(0);
    let Value::Resource(r) = &v else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($process) must be of type resource, {} given",
                v.tname()
            ),
        );
    };
    let data = r.res().data.borrow();
    let cell = data
        .as_ref()
        .and_then(|d| d.downcast_ref::<RefCell<Proc>>())
        .map(|c| c as *const RefCell<Proc>);
    drop(data);
    let Some(cell) = cell else {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid process resource",
        );
    };
    // SAFETY: the resource is held by `v` for the call.
    let mut p = unsafe { (*cell).borrow_mut() };
    f(vm, &mut p)
}

/// The parent's side of a pipe or socket: a stream for `$pipes`.
enum Parent {
    Pipe(OwnedFd, &'static [u8]),
}

fn proc_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // The command.
    let cmdv = a.val(0);
    let (argv, path_search, shown) = match &cmdv {
        Value::Array(arr) => {
            if arr.is_empty() {
                return vm.fthrow(a, "ValueError", "Argument #1 ($command) must not be empty");
            }
            let mut argv = Vec::new();
            let mut first = PhpStr::empty();
            for (i, (_, v)) in arr.iter().enumerate() {
                let s = vm.to_php_str(&v.deref())?;
                if s.contains(&0) {
                    return vm.throw(
                        "ValueError",
                        &format!("Command array element {} contains a null byte", i + 1),
                    );
                }
                if i == 0 {
                    if s.is_empty() {
                        return vm.throw(
                            "ValueError",
                            "First element must contain a non-empty program name",
                        );
                    }
                    first = s.clone();
                }
                argv.push(cstr(&s));
            }
            (argv, true, first)
        }
        _ => {
            let s = vm.to_php_str(&cmdv)?;
            (sh_argv(&s), false, s)
        }
    };
    // The descriptors.
    let specs = a.array(1);
    let mut child: Vec<(i64, ChildFd)> = Vec::new();
    let mut parent: Vec<(i64, Parent)> = Vec::new();
    // Child ends to close once the child runs.
    let mut keep: Vec<OwnedFd> = Vec::new();
    for (k, v) in specs.iter() {
        let KeyRef::Int(idx) = k else {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($descriptor_spec) must be an integer indexed array",
            );
        };
        let v = v.deref();
        match &v {
            Value::Resource(r) => match stream_fd(r) {
                Ok(Some(fd)) => child.push((idx, ChildFd::Dup(fd))),
                Ok(None) => {
                    let kind = {
                        let data = r.res().data.borrow();
                        data.as_ref()
                            .and_then(|d| d.downcast_ref::<RefCell<Stream>>())
                            .map(|c| c.borrow().stream_type)
                            .unwrap_or("Unknown")
                    };
                    vm.fwarn(
                        a,
                        &format!(
                            "Cannot represent a stream of type {} as a File Descriptor",
                            kind
                        ),
                    )?;
                    return Ok(Value::Bool(false));
                }
                Err(_) => {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        "supplied resource is not a valid stream resource",
                    );
                }
            },
            Value::Array(d) => {
                let Some(kind) = d.get_int(0).map(|x| x.deref()) else {
                    return vm.throw("ValueError", "Missing handle qualifier");
                };
                let kind = vm.to_php_str(&kind)?;
                let arg = |n: i64| d.get_int(n).map(|x| x.deref());
                match kind.as_bytes() {
                    b"pipe" => {
                        let Some(m) = arg(1) else {
                            return vm.throw("ValueError", "Missing mode parameter for 'pipe'");
                        };
                        let m = vm.to_php_str(&m)?;
                        let (rd, wr) = match pipe_pair() {
                            Ok(p) => p,
                            Err(e) => {
                                vm.fwarn(a, &format!("Unable to create pipe {}", e))?;
                                return Ok(Value::Bool(false));
                            }
                        };
                        if m.first() == Some(&b'r') {
                            child.push((idx, ChildFd::Dup(rd.as_raw_fd())));
                            keep.push(rd);
                            parent.push((idx, Parent::Pipe(wr, b"w")));
                        } else {
                            child.push((idx, ChildFd::Dup(wr.as_raw_fd())));
                            keep.push(wr);
                            parent.push((idx, Parent::Pipe(rd, b"r")));
                        }
                    }
                    b"socket" => {
                        let (x, y) = match std::os::unix::net::UnixStream::pair() {
                            Ok(p) => p,
                            Err(e) => {
                                vm.fwarn(a, &format!("Unable to create socket pair: {}", e))?;
                                return Ok(Value::Bool(false));
                            }
                        };
                        let (x, y) = (OwnedFd::from(x), OwnedFd::from(y));
                        child.push((idx, ChildFd::Dup(y.as_raw_fd())));
                        keep.push(y);
                        parent.push((idx, Parent::Pipe(x, b"r+")));
                    }
                    b"file" => {
                        let Some(path) = arg(1) else {
                            return vm
                                .throw("ValueError", "Missing file name parameter for 'file'");
                        };
                        let Some(m) = arg(2) else {
                            return vm.throw("ValueError", "Missing mode parameter for 'file'");
                        };
                        let path = vm.to_php_str(&path)?;
                        let m = vm.to_php_str(&m)?;
                        match open_file(vm, &path, &m) {
                            Ok(f) => {
                                let fd = OwnedFd::from(f);
                                child.push((idx, ChildFd::Dup(fd.as_raw_fd())));
                                keep.push(fd);
                            }
                            Err(why) => {
                                vm.warn(&format!(
                                    "proc_open({}): Failed to open stream: {}",
                                    path.to_string_lossy(),
                                    why
                                ))?;
                                return Ok(Value::Bool(false));
                            }
                        }
                    }
                    b"null" => match std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open("/dev/null")
                    {
                        Ok(f) => {
                            let fd = OwnedFd::from(f);
                            child.push((idx, ChildFd::Dup(fd.as_raw_fd())));
                            keep.push(fd);
                        }
                        Err(e) => {
                            vm.fwarn(a, &format!("Failed to open /dev/null: {}", e))?;
                            return Ok(Value::Bool(false));
                        }
                    },
                    b"redirect" => {
                        let Some(t) = arg(1) else {
                            return vm.throw("ValueError", "Missing redirection target");
                        };
                        let Value::Int(t) = t else {
                            return vm.throw(
                                "ValueError",
                                &format!(
                                    "Redirection target must be of type int, {} given",
                                    t.tname()
                                ),
                            );
                        };
                        child.push((idx, ChildFd::Redirect(t)));
                    }
                    b"pty" => match open_pty() {
                        Ok((master, slave)) => {
                            child.push((idx, ChildFd::Dup(slave.as_raw_fd())));
                            keep.push(slave);
                            parent.push((idx, Parent::Pipe(master, b"r+")));
                        }
                        Err(e) => {
                            vm.fwarn(a, &format!("Failed to open PTY master: {}", e))?;
                            return Ok(Value::Bool(false));
                        }
                    },
                    other => {
                        vm.fwarn(
                            a,
                            &format!(
                                "{} is not a valid descriptor spec/mode",
                                String::from_utf8_lossy(other)
                            ),
                        )?;
                        return Ok(Value::Bool(false));
                    }
                }
            }
            _ => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #2 ($descriptor_spec) must only contain arrays and streams",
                );
            }
        }
    }
    // Redirection targets: another descriptor of the spec, or one the
    // child inherits.
    for (_, how) in &child {
        if let ChildFd::Redirect(t) = how
            && !(0..=2).contains(t)
            && !child
                .iter()
                .any(|(i, h)| i == t && matches!(h, ChildFd::Dup(_)))
        {
            vm.fwarn(a, &format!("Redirection target {} not found", t))?;
            return Ok(Value::Bool(false));
        }
    }
    if let Some((i, ChildFd::Dup(src))) = child.iter().find(|(i, _)| *i < 0) {
        vm.fwarn(
            a,
            &format!(
                "Unable to copy file descriptor {} (for pipe) into file descriptor {}: Bad file descriptor",
                src, i
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    // Redirections go after the copies.
    child.sort_by_key(|(_, h)| matches!(h, ChildFd::Redirect(_)));
    let cwd = a.opt_str(3).map(|d| cstr(&d));
    let env = match a.get(4).map(|v| v.deref()) {
        Some(Value::Array(e)) => {
            let mut out = Vec::new();
            for (k, v) in e.iter() {
                let v = vm.to_php_str(&v.deref())?;
                let item = match k {
                    KeyRef::Str(k) => {
                        if v.is_empty() {
                            continue;
                        }
                        let mut s = k.to_vec();
                        s.push(b'=');
                        s.extend_from_slice(&v);
                        s
                    }
                    KeyRef::Int(_) => v.to_vec(),
                };
                out.push(cstr(&item));
            }
            Some(out)
        }
        _ => None,
    };
    let spec = SpawnSpec {
        argv,
        path_search,
        env,
        cwd,
        fds: child,
    };
    let pid = match spawn(vm, &spec) {
        Ok(p) => p,
        Err(e) => {
            vm.fwarn(a, &format!("posix_spawn() failed: {}", strerror(e)))?;
            return Ok(Value::Bool(false));
        }
    };
    drop(keep);
    let mut pipes = PhpArray::new();
    let mut held = Vec::new();
    for (idx, p) in parent {
        let Parent::Pipe(fd, mode) = p;
        let r = fd_stream(fd, mode);
        pipes.insert(Key::Int(idx), Value::Resource(r.clone()));
        held.push(r);
    }
    a.set_ref(2, Value::Array(pipes));
    let proc = Proc {
        pid,
        command: shown,
        pipes: held,
        status: None,
    };
    Ok(Value::Resource(ResRef::new(
        PhpStr::intern(b"process"),
        Box::new(RefCell::new(proc)),
    )))
}

/// A pseudoterminal pair (master, slave) for a "pty" descriptor.
fn open_pty() -> std::io::Result<(OwnedFd, OwnedFd)> {
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        if m < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let master = OwnedFd::from_raw_fd(m);
        if libc::grantpt(m) != 0 || libc::unlockpt(m) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut name = [0 as libc::c_char; 128];
        if libc::ptsname_r(m, name.as_mut_ptr(), name.len()) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let sl = libc::open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        );
        if sl < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok((master, OwnedFd::from_raw_fd(sl)))
    }
}

/// Opens a file for a "file" descriptor with fopen() modes.
fn open_file(vm: &mut Vm, path: &[u8], mode: &[u8]) -> Result<File, String> {
    let mut o = std::fs::OpenOptions::new();
    let plus = mode.contains(&b'+');
    match mode.first() {
        Some(b'r') => o.read(true).write(plus),
        Some(b'w') => o.write(true).read(plus).create(true).truncate(true),
        Some(b'a') => o.append(true).read(plus).create(true),
        Some(b'x') => o.write(true).read(plus).create_new(true),
        Some(b'c') => o.write(true).read(plus).create(true),
        _ => {
            return Err(format!(
                "`{}' is not a valid mode for fopen",
                String::from_utf8_lossy(mode)
            ));
        }
    };
    o.open(vm.fs_path(path)).map_err(|e| {
        e.raw_os_error()
            .map(strerror)
            .unwrap_or_else(|| e.to_string())
    })
}

fn reap(p: &mut Proc, block: bool) {
    if p.status.is_some() {
        return;
    }
    let mut st = 0;
    let flags = if block {
        0
    } else {
        libc::WNOHANG | libc::WUNTRACED
    };
    loop {
        let r = unsafe { libc::waitpid(p.pid, &mut st, flags) };
        if r < 0 && errno() == libc::EINTR {
            continue;
        }
        if r == p.pid && !libc::WIFSTOPPED(st) {
            p.status = Some(st);
        }
        return;
    }
}

/// PHP manual: proc_close() closes the pipes, waits for the process and
/// returns its exit code (-1 on error).
fn proc_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = match a.val(0) {
        Value::Resource(r) => Some(r),
        _ => None,
    };
    let code = with_proc(vm, a, |_, p| {
        for pipe in &p.pipes {
            pipe.close();
        }
        // A status reported earlier by proc_get_status() (observed on PHP
        // 8.5: kept for normal exits, gone after a signal).
        let cached = p.status;
        if let Some(st) = cached {
            return Ok(if libc::WIFEXITED(st) {
                libc::WEXITSTATUS(st) as i64
            } else {
                -1
            });
        }
        let st = wait_blocking(p.pid);
        p.status = Some(st.unwrap_or(0));
        Ok(st.map(exit_code).unwrap_or(-1))
    })?;
    if let Some(r) = r {
        r.close();
    }
    Ok(Value::Int(code))
}

fn proc_terminate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let sig = a.int_or(1, 15) as i32;
    with_proc(vm, a, |_, p| {
        Ok(Value::Bool(unsafe { libc::kill(p.pid, sig) } == 0))
    })
}

/// PHP manual: proc_get_status().
fn proc_get_status(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_proc(vm, a, |_, p| {
        reap(p, false);
        let mut o = PhpArray::new();
        o.insert(Key::from("command"), Value::Str(p.command.clone()));
        o.insert(Key::from("pid"), Value::Int(p.pid as i64));
        let (cached, running, signaled, stopped, exitcode, termsig, stopsig) = match p.status {
            None => (false, true, false, false, -1, 0, 0),
            Some(st) if libc::WIFEXITED(st) => (
                true,
                false,
                false,
                false,
                libc::WEXITSTATUS(st) as i64,
                0,
                0,
            ),
            Some(st) if libc::WIFSIGNALED(st) => {
                (false, false, true, false, -1, libc::WTERMSIG(st) as i64, 0)
            }
            Some(_) => (false, false, false, false, -1, 0, 0),
        };
        o.insert(Key::from("cached"), Value::Bool(cached));
        o.insert(Key::from("running"), Value::Bool(running));
        o.insert(Key::from("signaled"), Value::Bool(signaled));
        o.insert(Key::from("stopped"), Value::Bool(stopped));
        o.insert(Key::from("exitcode"), Value::Int(exitcode));
        o.insert(Key::from("termsig"), Value::Int(termsig));
        o.insert(Key::from("stopsig"), Value::Int(stopsig));
        Ok(Value::Array(o))
    })
}

/// PHP manual: proc_nice() changes this process's priority.
fn proc_nice(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let inc = a.int(0) as libc::c_int;
    // Linux priorities are per thread and the script may not run on the
    // main one: change the process (its main thread, what `ps` shows) and
    // the current thread alike.
    let pid = unsafe { libc::getpid() } as libc::id_t;
    unsafe { *libc::__errno_location() = 0 };
    let cur = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid) };
    let r = if cur == -1 && errno() != 0 {
        -1
    } else {
        let target = (cur + inc).clamp(-20, 19);
        let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid, target) };
        if r == 0 {
            let tid = unsafe { libc::gettid() } as libc::id_t;
            if tid != pid {
                unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, target) };
            }
        }
        r
    };
    if r != 0 {
        let e = errno();
        if e == libc::EPERM || e == libc::EACCES {
            vm.fwarn(
                a,
                "Only a super user may attempt to increase the priority of a process",
            )?;
        } else {
            vm.fwarn(a, &format!("Error: {}", strerror(e)))?;
        }
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- popen

/// The children of popen() streams, by resource id.
#[derive(Default)]
struct PopenState {
    pids: HashMap<i64, libc::pid_t>,
}

fn popen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cmd = a.str(0);
    let mode = a.str(1);
    let read = match mode.as_bytes() {
        b"r" | b"rb" => true,
        b"w" | b"wb" => false,
        _ => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($mode) must be one of \"r\", \"rb\", \"w\", or \"wb\"",
            );
        }
    };
    if cmd.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($command) must not contain any null bytes",
        );
    }
    let (rd, wr) = match pipe_pair() {
        Ok(p) => p,
        Err(e) => {
            vm.fwarn(a, &e.to_string())?;
            return Ok(Value::Bool(false));
        }
    };
    let (child_end, parent_end, target) = if read { (wr, rd, 1) } else { (rd, wr, 0) };
    let spec = SpawnSpec {
        argv: sh_argv(&cmd),
        path_search: false,
        env: None,
        cwd: None,
        fds: vec![(target, ChildFd::Dup(child_end.as_raw_fd()))],
    };
    let pid = match spawn(vm, &spec) {
        Ok(p) => p,
        Err(e) => {
            vm.fwarn(a, &strerror(e))?;
            return Ok(Value::Bool(false));
        }
    };
    drop(child_end);
    let r = fd_stream(parent_end, if read { b"r" } else { b"w" });
    vm.ext::<PopenState>().pids.insert(r.id(), pid);
    Ok(Value::Resource(r))
}

/// PHP manual: pclose() returns the exit status of the process.
fn pclose(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let Value::Resource(r) = &v else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($handle) must be of type resource, {} given",
                v.tname()
            ),
        );
    };
    if r.is_closed() {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid stream resource",
        );
    }
    r.close();
    let Some(pid) = vm.ext::<PopenState>().pids.remove(&r.id()) else {
        return Ok(Value::Int(-1));
    };
    Ok(Value::Int(match wait_blocking(pid) {
        Some(st) if libc::WIFEXITED(st) => libc::WEXITSTATUS(st) as i64,
        Some(st) if libc::WIFSIGNALED(st) => libc::WTERMSIG(st) as i64,
        _ => -1,
    }))
}

// ---------------------------------------------------------------- stream_select

/// PHP manual: stream_select() over streams backed by descriptors
/// (memory streams and buffered input are always ready).
fn stream_select(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let sets: Vec<Option<PhpArray>> = (0..3)
        .map(|i| match a.ref_at(i).map(|r| r.get()) {
            Some(Value::Array(arr)) => Some(arr),
            _ => None,
        })
        .collect();
    if sets.iter().all(|s| s.is_none()) {
        return vm.fthrow(a, "ValueError", "No stream arrays were passed");
    }
    let secs = a.opt_int(3);
    let usecs = a.opt_int(4).unwrap_or(0);
    if let Some(s) = secs {
        if s < 0 {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #4 ($seconds) must be greater than or equal to 0",
            );
        }
        if usecs < 0 {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #5 ($microseconds) must be greater than or equal to 0",
            );
        }
    } else if a.opt_int(4).is_some() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #5 ($microseconds) must be null when argument #4 ($seconds) is null",
        );
    }
    // (set index, key, fd or None when always ready)
    let mut entries: Vec<(usize, Key, Option<RawFd>)> = Vec::new();
    for (si, set) in sets.iter().enumerate() {
        let Some(set) = set else { continue };
        for (k, v) in set.iter() {
            let Value::Resource(r) = v.deref() else {
                continue;
            };
            let key = match k {
                KeyRef::Int(i) => Key::Int(i),
                KeyRef::Str(s) => Key::Str(s.clone()),
            };
            let ready_now = {
                let data = r.res().data.borrow();
                data.as_ref()
                    .and_then(|d| d.downcast_ref::<RefCell<Stream>>())
                    .map(|c| {
                        let s = c.borrow();
                        si == 0
                            && (!s.pending.is_empty()
                                || matches!(&s.backing, Backing::Native(n) if n.buffered()))
                    })
                    .unwrap_or(false)
            };
            match stream_fd(&r) {
                Ok(Some(fd)) if !ready_now => entries.push((si, key, Some(fd))),
                Ok(_) => entries.push((si, key, None)),
                Err(_) => {
                    vm.fwarn(a, "supplied resource is not a valid stream resource")?;
                    return Ok(Value::Bool(false));
                }
            }
        }
    }
    let mut pfds: Vec<libc::pollfd> = entries
        .iter()
        .map(|(si, _, fd)| libc::pollfd {
            fd: fd.unwrap_or(-1),
            events: match si {
                0 => libc::POLLIN,
                1 => libc::POLLOUT,
                _ => libc::POLLPRI,
            },
            revents: 0,
        })
        .collect();
    let always = entries.iter().any(|(_, _, fd)| fd.is_none());
    let timeout = if always {
        0
    } else {
        match secs {
            None => -1,
            Some(s) => (s.saturating_mul(1000) + usecs / 1000).min(i32::MAX as i64) as i32,
        }
    };
    vm.out.flush();
    let n = loop {
        let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout) };
        if n < 0 && errno() == libc::EINTR {
            continue;
        }
        break n;
    };
    if n < 0 {
        vm.fwarn(
            a,
            &format!("Unable to select [{}]: {}", errno(), strerror(errno())),
        )?;
        return Ok(Value::Bool(false));
    }
    let mut out: [PhpArray; 3] = [PhpArray::new(), PhpArray::new(), PhpArray::new()];
    let mut count = 0;
    for (i, (si, key, fd)) in entries.iter().enumerate() {
        let ready = fd.is_none() || pfds[i].revents != 0;
        if ready {
            let set = sets[*si].as_ref().unwrap();
            if let Some(v) = set.get(key.as_ref()) {
                out[*si].insert(key.clone(), v.deref());
                count += 1;
            }
        }
    }
    for (si, set) in sets.iter().enumerate() {
        if set.is_some() {
            a.set_ref(si, Value::Array(std::mem::take(&mut out[si])));
        }
    }
    Ok(Value::Int(count))
}
