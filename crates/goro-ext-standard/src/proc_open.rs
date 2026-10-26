//! proc_open() and friends (ext/standard/proc_open.c, POSIX branch).

use super::*;
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, FromRawFd};

/// php_process_handle
pub(crate) struct ProcHandle {
    pub pid: libc::pid_t,
    pub command: Vec<u8>,
    /// Stream resources of the parent ends ($pipes).
    pub pipes: Vec<i64>,
    /// The references the handle holds on its pipes (GC_ADDREF in
    /// proc_open()): they stay open while the process resource lives.
    pub _pipe_refs: Vec<Value>,
    pub cached_status: Option<i32>,
}

thread_local! {
    pub(crate) static PROCS: RefCell<StdHashMap<i64, ProcHandle>> = RefCell::new(StdHashMap::new());
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DescType {
    Std,
    Pipe,
    Socket,
}

/// descriptorspec_item
struct Desc {
    index: i32,
    ty: DescType,
    childend: i32,
    parentend: i32,
    /// O_RDONLY / O_WRONLY / O_RDWR of the parent end.
    mode_flags: i32,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = if class == b"TypeError" {
        vm.throw_type_error(msg.to_string())
    } else {
        vm.create_exception(class, msg, vm.current_line)
    };
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

fn pending(vm: &Vm) -> VmError {
    VmError {
        message: String::new(),
        line: vm.current_line,
    }
}

/// zval_try_get_string(); Err when an exception was thrown.
fn try_get_string(vm: &mut Vm, v: &Value) -> Result<Vec<u8>, VmError> {
    let v = v.deref();
    match &v {
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            Ok(b"Array".to_vec())
        }
        Value::Object(_) => match vm.value_to_string_checked(&v) {
            Ok(s) => Ok(s.as_bytes().to_vec()),
            Err(e) => {
                if vm.current_exception.is_some() {
                    Err(pending(vm))
                } else {
                    Err(e)
                }
            }
        },
        _ => Ok(v.to_php_string().as_bytes().to_vec()),
    }
}

fn set_cloexec(fd: i32) {
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
}

/// A stream resource's descriptor for proc_open()/php_stream_cast(AS_FD).
fn stream_cast_fd(vm: &mut Vm, id: i64) -> Option<i32> {
    if (1..=3).contains(&id) {
        return Some((id - 1) as i32);
    }
    let info = FILE_HANDLES.with(|h| {
        h.borrow()
            .get(&id)
            .map(|fh| (fh.special, fh.file.as_raw_fd(), fh.user_wrapper.is_some()))
    });
    match info {
        Some((SpecialStream::Memory, _, _)) => {
            vm.emit_warning(
                "proc_open(): Cannot represent a stream of type MEMORY as a File Descriptor",
            );
            None
        }
        Some((SpecialStream::Output, _, _)) => {
            vm.emit_warning(
                "proc_open(): Cannot represent a stream of type Output as a File Descriptor",
            );
            None
        }
        Some((SpecialStream::Stdin, _, _)) => Some(0),
        Some((SpecialStream::Stdout, _, _)) => Some(1),
        Some((SpecialStream::Stderr, _, _)) => Some(2),
        Some((_, _, true)) => {
            vm.emit_warning(
                "proc_open(): Cannot represent a stream of type user-space as a File Descriptor",
            );
            None
        }
        Some((_, fd, false)) => {
            let buffered = FILE_HANDLES.with(|h| {
                h.borrow_mut()
                    .get_mut(&id)
                    .map(|fh| std::mem::take(&mut fh.filter_read_ready).len())
                    .unwrap_or(0)
            });
            if buffered > 0 {
                vm.emit_warning(&format!(
                    "proc_open(): {} bytes of buffered data lost during stream conversion!",
                    buffered
                ));
            }
            Some(fd)
        }
        None => None,
    }
}

/// The descriptor behind a php://stdin, php://stdout, php://stderr or
/// php://fd/N stream name.
fn php_fd_stream(file: &[u8]) -> Option<i32> {
    let rest = match file.get(..6) {
        Some(p) if p.eq_ignore_ascii_case(b"php://") => &file[6..],
        _ => return None,
    };
    if rest.eq_ignore_ascii_case(b"stdin") {
        Some(0)
    } else if rest.eq_ignore_ascii_case(b"stdout") {
        Some(1)
    } else if rest.eq_ignore_ascii_case(b"stderr") {
        Some(2)
    } else if rest.len() > 3 && rest[..3].eq_ignore_ascii_case(b"fd/") {
        std::str::from_utf8(&rest[3..]).ok()?.parse::<i32>().ok()
    } else {
        None
    }
}

/// php_stream_parse_fopen_modes() for "file" descriptors.
fn open_flags(mode: &[u8]) -> Option<i32> {
    let plus = mode.contains(&b'+');
    let base = match mode.first()? {
        b'r' => 0,
        b'w' => libc::O_CREAT | libc::O_TRUNC,
        b'a' => libc::O_CREAT | libc::O_APPEND,
        b'x' => libc::O_CREAT | libc::O_EXCL,
        b'c' => libc::O_CREAT,
        _ => return None,
    };
    let acc = if plus {
        libc::O_RDWR
    } else if mode[0] == b'r' {
        libc::O_RDONLY
    } else {
        libc::O_WRONLY
    };
    Some(base | acc)
}

fn get_string_param(vm: &mut Vm, ht: &PhpArray, idx: i64, what: &str) -> Result<Vec<u8>, VmError> {
    match ht.get_int(idx) {
        Some(v) => {
            let v = v.clone();
            try_get_string(vm, &v)
        }
        None => Err(throw(vm, b"ValueError", &format!("Missing {}", what))),
    }
}

/// Outcome of setting up one descriptor: Err(Some(e)) propagates an
/// exception, Err(None) is a plain failure (proc_open() returns false).
type DescResult = Result<(), Option<VmError>>;

/// The PTY shared by the 'pty' descriptors (master, slave), closed when the
/// child has been started (or proc_open() failed).
struct Pty([i32; 2]);

impl Drop for Pty {
    fn drop(&mut self) {
        for fd in self.0 {
            if fd >= 0 {
                unsafe { libc::close(fd) };
            }
        }
    }
}

/// openpty()
fn open_pty() -> Option<[i32; 2]> {
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if m < 0 {
            return None;
        }
        if libc::grantpt(m) != 0 || libc::unlockpt(m) != 0 {
            let e = errno();
            libc::close(m);
            *libc::__errno_location() = e;
            return None;
        }
        let mut name = [0 as libc::c_char; 128];
        if libc::ptsname_r(m, name.as_mut_ptr(), name.len()) != 0 {
            let e = errno();
            libc::close(m);
            *libc::__errno_location() = e;
            return None;
        }
        let sl = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
        if sl < 0 {
            let e = errno();
            libc::close(m);
            *libc::__errno_location() = e;
            return None;
        }
        set_cloexec(m);
        set_cloexec(sl);
        Some([m, sl])
    }
}

fn desc_from_array(
    vm: &mut Vm,
    ht: &PhpArray,
    descs: &mut Vec<Desc>,
    index: i32,
    pty: &mut Pty,
) -> DescResult {
    let ty = get_string_param(vm, ht, 0, "handle qualifier").map_err(Some)?;
    let mut d = Desc {
        index,
        ty: DescType::Std,
        childend: -1,
        parentend: -1,
        mode_flags: 0,
    };
    match ty.as_slice() {
        b"pipe" => {
            let mode = get_string_param(vm, ht, 1, "mode parameter for 'pipe'").map_err(Some)?;
            let mut p = [0i32; 2];
            if unsafe { libc::pipe(p.as_mut_ptr()) } != 0 {
                let e = errno();
                vm.emit_warning(&format!(
                    "proc_open(): Unable to create pipe {}",
                    xp_socket::strerror(e)
                ));
                return Err(None);
            }
            d.ty = DescType::Pipe;
            if !mode.starts_with(b"w") {
                d.parentend = p[1];
                d.childend = p[0];
                d.mode_flags = libc::O_WRONLY;
            } else {
                d.parentend = p[0];
                d.childend = p[1];
                d.mode_flags = libc::O_RDONLY;
            }
            set_cloexec(d.parentend);
        }
        b"socket" => {
            let mut s = [0i32; 2];
            if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, s.as_mut_ptr()) } != 0
            {
                let e = errno();
                vm.emit_warning(&format!(
                    "proc_open(): Unable to create socket pair: {}",
                    xp_socket::strerror(e)
                ));
                return Err(None);
            }
            d.ty = DescType::Socket;
            d.parentend = s[0];
            set_cloexec(d.parentend);
            d.childend = s[1];
        }
        b"file" => {
            let file =
                get_string_param(vm, ht, 1, "file name parameter for 'file'").map_err(Some)?;
            let mode = get_string_param(vm, ht, 2, "mode parameter for 'file'").map_err(Some)?;
            let flags = open_flags(&mode).unwrap_or(libc::O_RDONLY);
            let path = match CString::new(file.clone()) {
                Ok(p) => p,
                Err(_) => {
                    return Err(Some(throw(
                        vm,
                        b"ValueError",
                        "proc_open(): Argument #1 ($filename) must not contain any null bytes",
                    )));
                }
            };
            // The file is opened through the stream layer and cast to a
            // descriptor: php://stdin, php://stdout, php://stderr and
            // php://fd/N are the process's own descriptors.
            let fd = match php_fd_stream(&file) {
                Some(n) => unsafe { libc::dup(n) },
                None => {
                    let path = match file.get(..7) {
                        Some(p) if p.eq_ignore_ascii_case(b"file://") => {
                            CString::new(file[7..].to_vec()).unwrap_or(path)
                        }
                        _ => path,
                    };
                    unsafe { libc::open(path.as_ptr(), flags, 0o666) }
                }
            };
            if fd < 0 {
                let e = errno();
                vm.emit_warning(&format!(
                    "proc_open({}): Failed to open stream: {}",
                    goro_core::string::msg_str(&file),
                    xp_socket::strerror(e)
                ));
                return Err(None);
            }
            d.childend = fd;
        }
        b"redirect" => {
            let target = match ht.get_int(1).map(|v| v.deref()) {
                None => {
                    return Err(Some(throw(vm, b"ValueError", "Missing redirection target")));
                }
                Some(Value::Long(n)) => n as i32,
                Some(v) => {
                    return Err(Some(throw(
                        vm,
                        b"ValueError",
                        &format!(
                            "Redirection target must be of type int, {} given",
                            Vm::value_type_name(&v)
                        ),
                    )));
                }
            };
            let mut redirect_to = descs
                .iter()
                .find(|x| x.index == target)
                .map(|x| x.childend)
                .unwrap_or(-1);
            if redirect_to == -1 {
                if !(0..=2).contains(&target) {
                    vm.emit_warning(&format!(
                        "proc_open(): Redirection target {} not found",
                        target
                    ));
                    return Err(None);
                }
                redirect_to = target;
            }
            d.childend = dup_fd(vm, redirect_to, descs.len())?;
        }
        b"null" => {
            let fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR) };
            if fd < 0 {
                let e = errno();
                vm.emit_warning(&format!(
                    "proc_open(): Failed to open /dev/null: {}",
                    xp_socket::strerror(e)
                ));
                return Err(None);
            }
            d.childend = fd;
        }
        b"pty" => {
            // set_proc_descriptor_to_pty()
            if pty.0[0] == -1 {
                match open_pty() {
                    Some(fds) => pty.0 = fds,
                    None => {
                        let e = errno();
                        vm.emit_warning(&format!(
                            "proc_open(): Could not open PTY (pseudoterminal): {}",
                            xp_socket::strerror(e)
                        ));
                        return Err(None);
                    }
                }
            }
            d.ty = DescType::Pipe;
            d.childend = unsafe { libc::dup(pty.0[1]) };
            d.parentend = unsafe { libc::dup(pty.0[0]) };
            set_cloexec(d.parentend);
            d.mode_flags = libc::O_RDWR;
        }
        _ => {
            vm.emit_warning(&format!(
                "proc_open(): {} is not a valid descriptor spec/mode",
                goro_core::string::msg_str(&ty)
            ));
            return Err(None);
        }
    }
    descs.push(d);
    Ok(())
}

fn dup_fd(vm: &mut Vm, from: i32, nindex: usize) -> Result<i32, Option<VmError>> {
    let fd = unsafe { libc::dup(from) };
    if fd < 0 {
        let e = errno();
        vm.emit_warning(&format!(
            "proc_open(): Failed to dup() for descriptor {}: {}",
            nindex,
            xp_socket::strerror(e)
        ));
        return Err(None);
    }
    Ok(fd)
}

fn close_all(descs: &[Desc]) {
    for d in descs {
        if d.childend >= 0 {
            unsafe { libc::close(d.childend) };
        }
        if d.parentend >= 0 {
            unsafe { libc::close(d.parentend) };
        }
    }
}

/// php_array_to_envp()
fn array_to_envp(vm: &mut Vm, env: &PhpArray) -> Result<Vec<CString>, VmError> {
    let mut out = Vec::new();
    let entries: Vec<(ArrayKey, Value)> = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in entries {
        let s = try_get_string(vm, &v)?;
        if s.is_empty() {
            continue;
        }
        let mut item = Vec::new();
        if let ArrayKey::String(key) = &k {
            if !key.as_bytes().is_empty() {
                item.extend_from_slice(key.as_bytes());
                item.push(b'=');
            }
        }
        item.extend_from_slice(&s);
        // C strings stop at the first NUL.
        if let Some(p) = item.iter().position(|&c| c == 0) {
            item.truncate(p);
        }
        out.push(CString::new(item).unwrap_or_default());
    }
    Ok(out)
}

fn cstr_trunc(b: &[u8]) -> CString {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    CString::new(&b[..end]).unwrap_or_default()
}

unsafe extern "C" {
    static mut environ: *const *const libc::c_char;
}

/// PHP_FUNCTION(proc_open)
pub(crate) fn proc_open(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 3 || args.len() > 6 {
        let (q, c) = if args.len() < 3 {
            ("at least", 3)
        } else {
            ("at most", 6)
        };
        return Err(throw(
            vm,
            b"ArgumentCountError",
            &format!(
                "proc_open() expects {} {} arguments, {} given",
                q,
                c,
                args.len()
            ),
        ));
    }
    let cmd_v = args[0].deref();
    let spec_v = args[1].deref();
    // Command: string or array (no shell).
    let mut argv: Option<Vec<CString>> = None;
    let command: Vec<u8> = match &cmd_v {
        Value::Array(a) => {
            let items: Vec<Value> = a.borrow().values().cloned().collect();
            if items.is_empty() {
                return Err(throw(
                    vm,
                    b"ValueError",
                    "proc_open(): Argument #1 ($command) must not be empty",
                ));
            }
            let mut av = Vec::new();
            let mut first = Vec::new();
            for (i, it) in items.iter().enumerate() {
                let s = try_get_string(vm, it)?;
                if i == 0 && s.is_empty() {
                    return Err(throw(
                        vm,
                        b"ValueError",
                        "First element must contain a non-empty program name",
                    ));
                }
                if s.contains(&0) {
                    return Err(throw(
                        vm,
                        b"ValueError",
                        &format!("Command array element {} contains a null byte", i + 1),
                    ));
                }
                if i == 0 {
                    first = s.clone();
                }
                av.push(CString::new(s).unwrap_or_default());
            }
            argv = Some(av);
            first
        }
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            cmd_v.to_php_string().as_bytes().to_vec()
        }
        _ => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "proc_open(): Argument #1 ($command) must be of type array|string, {} given",
                    Vm::value_type_name(&cmd_v)
                ),
            ));
        }
    };
    let spec = match &spec_v {
        Value::Array(a) => a.clone(),
        _ => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "proc_open(): Argument #2 ($descriptor_spec) must be of type array, {} given",
                    Vm::value_type_name(&spec_v)
                ),
            ));
        }
    };
    let cwd: Option<Vec<u8>> = match args.get(3).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(v) => Some(try_get_string(vm, &v)?),
    };
    let env: Option<Vec<CString>> = match args.get(4).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(Value::Array(a)) => {
            let a = a.borrow().clone();
            Some(array_to_envp(vm, &a)?)
        }
        Some(v) => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "proc_open(): Argument #5 ($env_vars) must be of type ?array, {} given",
                    Vm::value_type_name(&v)
                ),
            ));
        }
    };

    // Walk the descriptor spec.
    let mut descs: Vec<Desc> = Vec::new();
    let mut pty = Pty([-1, -1]);
    let entries: Vec<(ArrayKey, Value)> = spec
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, item) in entries {
        let index = match k {
            ArrayKey::Int(n) => n as i32,
            ArrayKey::String(_) => {
                close_all(&descs);
                return Err(throw(
                    vm,
                    b"ValueError",
                    "proc_open(): Argument #2 ($descriptor_spec) must be an integer indexed array",
                ));
            }
        };
        let r: DescResult = match item.deref() {
            Value::Resource(id) => {
                let is_stream = matches!(
                    goro_core::value::resource_type_of(id.id()).as_deref(),
                    Some(b"stream") | Some(b"persistent stream")
                ) && goro_core::value::is_open_resource(id.id());
                if !is_stream {
                    close_all(&descs);
                    return Err(throw(
                        vm,
                        b"TypeError",
                        "proc_open(): supplied resource is not a valid stream resource",
                    ));
                }
                match stream_cast_fd(vm, id.id()) {
                    Some(fd) => match dup_fd(vm, fd, descs.len()) {
                        Ok(nfd) => {
                            descs.push(Desc {
                                index,
                                ty: DescType::Std,
                                childend: nfd,
                                parentend: -1,
                                mode_flags: 0,
                            });
                            Ok(())
                        }
                        Err(e) => Err(e),
                    },
                    None => Err(None),
                }
            }
            Value::Array(a) => {
                let a = a.borrow().clone();
                desc_from_array(vm, &a, &mut descs, index, &mut pty)
            }
            _ => {
                close_all(&descs);
                return Err(throw(
                    vm,
                    b"ValueError",
                    "proc_open(): Argument #2 ($descriptor_spec) must only contain arrays and streams",
                ));
            }
        };
        match r {
            Ok(()) => {}
            Err(Some(e)) => {
                close_all(&descs);
                return Err(e);
            }
            Err(None) => {
                close_all(&descs);
                return Ok(Value::False);
            }
        }
    }

    // Prepare everything the child needs before fork().
    let sh = c"/bin/sh";
    let shell_argv: Vec<CString> = vec![
        CString::new("sh").unwrap(),
        CString::new("-c").unwrap(),
        cstr_trunc(&command),
    ];
    let av = argv.as_ref().unwrap_or(&shell_argv);
    let mut av_ptrs: Vec<*const libc::c_char> = av.iter().map(|c| c.as_ptr()).collect();
    av_ptrs.push(std::ptr::null());
    let env_ptrs: Option<Vec<*const libc::c_char>> = env.as_ref().map(|e| {
        let mut v: Vec<*const libc::c_char> = e.iter().map(|c| c.as_ptr()).collect();
        v.push(std::ptr::null());
        v
    });
    let cwd_c = cwd.as_ref().map(|c| cstr_trunc(c));
    let prog: *const libc::c_char = match &argv {
        Some(a) => a[0].as_ptr(),
        None => sh.as_ptr(),
    };
    // Exec errors are reported back through a close-on-exec pipe.
    let mut errpipe = [0i32; 2];
    if unsafe { libc::pipe2(errpipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        errpipe = [-1, -1];
    }
    let child_fds: Vec<(i32, i32, i32, bool)> = descs
        .iter()
        .map(|d| (d.index, d.childend, d.parentend, d.ty != DescType::Std))
        .collect();

    super::flush_cli_output(vm);
    let child_state = goro_core::child_proc::ChildState::capture();
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // Child: only async-signal-safe calls from here on.
        unsafe {
            if errpipe[0] >= 0 {
                libc::close(errpipe[0]);
            }
            let fail = |e: i32| -> ! {
                if errpipe[1] >= 0 {
                    let b = e.to_ne_bytes();
                    libc::write(errpipe[1], b.as_ptr() as *const libc::c_void, 4);
                }
                libc::_exit(127);
            };
            for &(index, childend, parentend, has_parent) in &child_fds {
                if has_parent {
                    libc::close(parentend);
                }
                if childend != index {
                    if libc::dup2(childend, index) < 0 {
                        fail(errno());
                    }
                    libc::close(childend);
                }
            }
            // The signal mask is inherited as in php-src (Symfony Process
            // blocks the signals a child should ignore around proc_open());
            // SIGPIPE and goro's RLIMIT_AS go back to what goro started with.
            child_state.apply_in_child_keep_mask();
            if let Some(c) = &cwd_c {
                if libc::chdir(c.as_ptr()) != 0 {
                    fail(errno());
                }
            }
            if let Some(e) = &env_ptrs {
                environ = e.as_ptr();
            }
            if argv.is_some() {
                libc::execvp(prog, av_ptrs.as_ptr());
            } else {
                libc::execv(prog, av_ptrs.as_ptr());
            }
            fail(errno());
        }
    }
    if errpipe[1] >= 0 {
        unsafe { libc::close(errpipe[1]) };
    }
    if pid < 0 {
        let e = errno();
        if errpipe[0] >= 0 {
            unsafe { libc::close(errpipe[0]) };
        }
        close_all(&descs);
        vm.emit_warning(&format!(
            "proc_open(): Fork failed: {}",
            xp_socket::strerror(e)
        ));
        return Ok(Value::False);
    }
    // Did exec succeed?
    if errpipe[0] >= 0 {
        let mut b = [0u8; 4];
        let n = loop {
            let n = unsafe { libc::read(errpipe[0], b.as_mut_ptr() as *mut libc::c_void, 4) };
            if n < 0 && errno() == libc::EINTR {
                continue;
            }
            break n;
        };
        unsafe { libc::close(errpipe[0]) };
        if n == 4 {
            let e = i32::from_ne_bytes(b);
            let mut st = 0;
            unsafe { libc::waitpid(pid, &mut st, 0) };
            close_all(&descs);
            vm.emit_warning(&format!(
                "proc_open(): posix_spawn() failed: {}",
                xp_socket::strerror(e)
            ));
            return Ok(Value::False);
        }
    }

    // Parent: close the child ends and open streams on the parent ends.
    let mut pipes_arr = PhpArray::new();
    let mut pipe_ids = Vec::new();
    for d in &descs {
        unsafe { libc::close(d.childend) };
        let fid = match d.ty {
            DescType::Pipe => {
                let mode = match d.mode_flags {
                    libc::O_WRONLY => "w",
                    libc::O_RDONLY => "r",
                    _ => "r+",
                };
                let file = unsafe { std::fs::File::from_raw_fd(d.parentend) };
                let fid = alloc_file_handle(file, mode);
                FILE_HANDLES.with(|h| {
                    if let Some(fh) = h.borrow_mut().get_mut(&fid) {
                        fh.wrapper_type = String::new();
                        fh.seekable = false;
                    }
                });
                goro_core::value::register_resource(fid, b"stream");
                fid
            }
            DescType::Socket => xp_socket::stream_from_socket(vm, d.parentend),
            DescType::Std => continue,
        };
        // (the pipe is freed when its last reference goes away)
        goro_core::value::adopt_new_resource(fid);
        pipes_arr.set(ArrayKey::Int(d.index as i64), Value::resource(fid));
        pipe_ids.push(fid);
    }
    if let Some(Value::Reference(r)) = args.get(2) {
        *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(pipes_arr)));
    }
    let id = goro_core::value::alloc_resource_id();
    PROCS.with(|p| {
        p.borrow_mut().insert(
            id,
            ProcHandle {
                pid,
                command,
                _pipe_refs: pipe_ids.iter().map(Value::resource).collect(),
                pipes: pipe_ids,
                cached_status: None,
            },
        )
    });
    goro_core::value::register_resource(id, b"process");
    Ok(Value::resource(id))
}

/// zend_fetch_resource(..., "process", le_proc_open)
fn fetch_proc(vm: &mut Vm, func: &str, args: &[Value]) -> Result<i64, VmError> {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let id = match v {
        Value::Resource(id) => id.id(),
        _ => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #1 ($process) must be of type resource, {} given",
                    func,
                    Vm::value_type_name(&v)
                ),
            ));
        }
    };
    if PROCS.with(|p| p.borrow().contains_key(&id)) {
        Ok(id)
    } else {
        Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): supplied resource is not a valid process resource",
                func
            ),
        ))
    }
}

/// waitpid_cached()
fn waitpid_cached(id: i64, options: i32) -> (libc::pid_t, i32) {
    let (pid, cached) = PROCS.with(|p| {
        p.borrow()
            .get(&id)
            .map(|h| (h.pid, h.cached_status))
            .unwrap_or((-1, None))
    });
    if let Some(st) = cached {
        return (pid, st);
    }
    let mut st = 0;
    let r = unsafe { libc::waitpid(pid, &mut st, options) };
    if r > 0 && libc::WIFEXITED(st) {
        PROCS.with(|p| {
            if let Some(h) = p.borrow_mut().get_mut(&id) {
                h.cached_status = Some(st);
            }
        });
    }
    (r, st)
}

/// proc_open_rsrc_dtor(): close the pipes and reap the child.
fn proc_dtor(vm: &mut Vm, id: i64, wait: bool) -> i64 {
    let pipes = PROCS.with(|p| {
        p.borrow()
            .get(&id)
            .map(|h| h.pipes.clone())
            .unwrap_or_default()
    });
    for fid in pipes {
        if has_file_handle(fid) {
            let _ = fclose_fn(vm, &[Value::resource(fid)]);
        }
    }
    let opts = if wait { 0 } else { libc::WNOHANG };
    let (r, mut st) = loop {
        let (r, st) = waitpid_cached(id, opts);
        if r == -1 && errno() == libc::EINTR {
            continue;
        }
        break (r, st);
    };
    PROCS.with(|p| p.borrow_mut().remove(&id));
    goro_core::value::close_resource(id);
    if r <= 0 {
        -1
    } else {
        if libc::WIFEXITED(st) {
            st = libc::WEXITSTATUS(st);
        }
        st as i64
    }
}

/// proc_open_rsrc_dtor() when the process resource's last reference goes
/// away: the pipes are closed and the child is reaped if it exited already
/// (FG(pclose_wait) is only set by proc_close()).
pub(crate) fn proc_rsrc_dtor(vm: &mut Vm, id: i64) {
    if goro_core::value::is_open_resource(id) && PROCS.with(|p| p.borrow().contains_key(&id)) {
        proc_dtor(vm, id, false);
    }
}

/// PHP_FUNCTION(proc_close)
pub(crate) fn proc_close(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = fetch_proc(vm, "proc_close", args)?;
    super::flush_cli_output(vm);
    Ok(Value::Long(proc_dtor(vm, id, true)))
}

/// PHP_FUNCTION(proc_terminate)
pub(crate) fn proc_terminate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = fetch_proc(vm, "proc_terminate", args)?;
    let sig = args
        .get(1)
        .map(|v| v.to_long())
        .unwrap_or(libc::SIGTERM as i64);
    let pid = PROCS.with(|p| p.borrow().get(&id).map(|h| h.pid).unwrap_or(-1));
    Ok(if unsafe { libc::kill(pid, sig as i32) } == 0 {
        Value::True
    } else {
        Value::False
    })
}

/// PHP_FUNCTION(proc_get_status)
pub(crate) fn proc_get_status(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = fetch_proc(vm, "proc_get_status", args)?;
    let (pid, command) = PROCS.with(|p| {
        p.borrow()
            .get(&id)
            .map(|h| (h.pid, h.command.clone()))
            .unwrap_or((-1, Vec::new()))
    });
    let mut running = true;
    let mut signaled = false;
    let mut stopped = false;
    let mut exitcode = -1i64;
    let mut termsig = 0i64;
    let mut stopsig = 0i64;
    let (r, st) = waitpid_cached(id, libc::WNOHANG | libc::WUNTRACED);
    if r == pid {
        if libc::WIFEXITED(st) {
            running = false;
            exitcode = libc::WEXITSTATUS(st) as i64;
        }
        if libc::WIFSIGNALED(st) {
            running = false;
            signaled = true;
            termsig = libc::WTERMSIG(st) as i64;
        }
        if libc::WIFSTOPPED(st) {
            stopped = true;
            stopsig = libc::WSTOPSIG(st) as i64;
        }
    } else if r == -1 {
        running = false;
    }
    let cached = PROCS.with(|p| {
        p.borrow()
            .get(&id)
            .map(|h| h.cached_status.is_some())
            .unwrap_or(false)
    });
    let b = |x: bool| if x { Value::True } else { Value::False };
    let mut arr = PhpArray::new();
    let mut set = |k: &[u8], v: Value| arr.set(ArrayKey::String(PhpString::from_bytes(k)), v);
    set(b"command", Value::String(PhpString::from_vec(command)));
    set(b"pid", Value::Long(pid as i64));
    set(b"cached", b(cached));
    set(b"running", b(running));
    set(b"signaled", b(signaled));
    set(b"stopped", b(stopped));
    set(b"exitcode", Value::Long(exitcode));
    set(b"termsig", Value::Long(termsig));
    set(b"stopsig", Value::Long(stopsig));
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}
